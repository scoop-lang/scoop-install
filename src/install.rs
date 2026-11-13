use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, ensure};
use liblzma::read::XzDecoder;
use serde::{Deserialize, Serialize};

use crate::model::State;
use crate::store::{FILES, STATE, Store, lock, write_toml};

#[derive(Default, Deserialize, Serialize)]
struct Inventory {
    files: BTreeMap<String, Vec<PathBuf>>,
}

fn unpack(archive: &Path, root: &Path, owned: &mut BTreeSet<PathBuf>) -> Result<Vec<PathBuf>> {
    let mut archive = tar::Archive::new(XzDecoder::new(File::open(archive)?));
    archive.set_preserve_permissions(true);
    let mut files = Vec::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let raw = entry.path()?.into_owned();
        let mut path = PathBuf::new();
        for component in raw.components() {
            match component {
                Component::CurDir => (),
                Component::Normal(name) => path.push(name),
                _ => anyhow::bail!("Archive path escapes installation: {}", raw.display()),
            }
        }
        if path.as_os_str().is_empty() {
            continue;
        }
        ensure!(
            !path.starts_with(".scoop-install"),
            "Archive uses reserved installation state path"
        );
        let kind = entry.header().entry_type();
        ensure!(
            kind.is_dir() || kind.is_file() || kind.is_symlink() || kind.is_hard_link(),
            "Unsupported archive entry {}",
            path.display()
        );
        if !kind.is_dir() {
            ensure!(
                owned.insert(path.clone()),
                "Components both own {}",
                path.display()
            );
            files.push(path);
        }
        ensure!(
            entry.unpack_in(root)?,
            "Archive entry is outside installation"
        );
    }
    Ok(files)
}

fn move_files(source: &Path, destination: &Path, files: &[PathBuf]) -> Result<()> {
    let mut moved: Vec<&PathBuf> = Vec::new();
    let result = (|| {
        for relative in files {
            let output = destination.join(relative);
            match output.symlink_metadata() {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(error.into()),
                Ok(_) => anyhow::bail!("Installation would overwrite {}", output.display()),
            }
            fs::create_dir_all(output.parent().context("File has no parent")?)?;
            fs::rename(source.join(relative), &output)
                .with_context(|| format!("Install {}", relative.display()))?;
            moved.push(relative);
        }
        Ok(())
    })();
    if let Err(error) = result {
        for relative in moved.into_iter().rev() {
            fs::create_dir_all(
                source
                    .join(relative)
                    .parent()
                    .context("File has no parent")?,
            )?;
            fs::rename(destination.join(relative), source.join(relative))
                .context("Restore files after failed installation")?;
        }
        return Err(error);
    }
    Ok(())
}

fn prune(root: &Path, files: &[PathBuf]) {
    let mut directories = BTreeSet::new();
    for file in files {
        let mut parent = file.parent();
        while let Some(path) = parent {
            if path.as_os_str().is_empty() {
                break;
            }
            directories.insert(path.to_path_buf());
            parent = path.parent();
        }
    }
    for directory in directories.iter().rev() {
        let _ = fs::remove_dir(root.join(directory));
    }
}

impl Store {
    pub fn install(
        &self,
        version: &str,
        explicit: Option<&str>,
        targets: &[String],
        components: &[String],
        skip_docs: bool,
    ) -> Result<State> {
        let existing = version != "latest" && self.root(version)?.join(STATE).is_file();
        let (manifest, manifest_url) = if existing && explicit.is_none() {
            let old = self.state(version)?;
            (old.manifest, old.manifest_url)
        } else {
            self.manifest(version, explicit)?
        };
        let _guard = self.toolchain_lock(&manifest.version)?;
        let mut state = State {
            host: self.host.clone(),
            manifest,
            manifest_url,
            components: BTreeSet::new(),
        };
        let root = self.root(&state.manifest.version)?;
        let is_new = !root.join(STATE).is_file();
        if !is_new {
            let old = self.state(&state.manifest.version)?;
            ensure!(
                old.manifest.revision == state.manifest.revision
                    && old.manifest.release == state.manifest.release,
                "Installed version has a different release identity; use a new version for a new build"
            );
            state = old;
        }
        let selected = if targets.is_empty() {
            if is_new {
                vec![state.manifest.host(&self.host)?.default_target.clone()]
            } else {
                state.targets()
            }
        } else {
            targets.to_vec()
        };
        let mut desired = state.components.clone();
        desired.insert(state.manifest.host(&self.host)?.bin.clone());
        desired.insert(state.manifest.sources.clone());
        for target in &selected {
            desired.extend(state.manifest.target_components(
                &self.host,
                target,
                components.iter().any(|c| c == "gui"),
                components.iter().any(|c| c == "profiling"),
            )?);
        }
        for component in components {
            if component != "gui" && component != "profiling" {
                desired.insert(state.manifest.component(&self.host, component, None)?);
            }
        }
        let changed = self.apply(&mut state, desired)?;
        self.proxies(&root)?;
        if changed && !skip_docs {
            self.try_docs(&state, &selected);
        }
        self.set_default(&state.manifest.version, true)?;
        Ok(state)
    }

    pub fn update(&self, version: &str, explicit: Option<&str>, skip_docs: bool) -> Result<()> {
        let current = self
            .settings()?
            .default
            .context("No default toolchain to update")?;
        let old = self.state(&current)?;
        let (manifest, manifest_url) = self.manifest(version, explicit)?;
        let _guard = self.toolchain_lock(&manifest.version)?;
        let mut state = State {
            host: self.host.clone(),
            manifest,
            manifest_url,
            components: BTreeSet::new(),
        };
        let root = self.root(&state.manifest.version)?;
        if root.join(STATE).is_file() {
            let installed = self.state(&state.manifest.version)?;
            ensure!(
                installed.manifest.release == state.manifest.release
                    && installed.manifest.revision == state.manifest.revision,
                "Installed version has a different release identity"
            );
            state = installed;
        }
        let targets = old.targets();
        let mut desired = state.components.clone();
        desired.insert(state.manifest.host(&self.host)?.bin.clone());
        desired.insert(state.manifest.sources.clone());
        if old.components.contains(&old.manifest.vendor) {
            desired.insert(state.manifest.vendor.clone());
        }
        for target in &targets {
            let gui = old.manifest.targets[target]
                .gui
                .as_ref()
                .is_some_and(|gui| old.components.contains(gui));
            let profiling = old.manifest.targets[target]
                .profiling
                .as_ref()
                .is_some_and(|component| old.components.contains(component));
            desired.extend(
                state
                    .manifest
                    .target_components(&self.host, target, gui, profiling)?,
            );
        }
        let changed = self.apply(&mut state, desired)?;
        self.proxies(&root)?;
        if changed && !skip_docs {
            self.try_docs(&state, &targets);
        }
        self.set_default(&state.manifest.version, false)
    }

    fn apply(&self, state: &mut State, desired: BTreeSet<String>) -> Result<bool> {
        state.validate_selection(&desired)?;
        let root = self.root(&state.manifest.version)?;
        let is_new = !root.join(STATE).is_file();
        let added: Vec<_> = desired.difference(&state.components).cloned().collect();
        let removed: Vec<_> = state.components.difference(&desired).cloned().collect();
        if added.is_empty() && removed.is_empty() {
            println!("Toolchain {} is already installed", state.manifest.version);
            return Ok(false);
        }
        let size: u64 = added
            .iter()
            .map(|name| state.manifest.archives[name].size)
            .sum();
        println!(
            "Toolchain {} ({})\nAdd: {}\nRemove: {}\nArchive bytes: {size}",
            state.manifest.version,
            self.host,
            added.join(", "),
            removed.join(", ")
        );
        for (key, value) in &state.manifest.host(&self.host)?.requirements {
            println!("  {key}: {value}");
        }
        let mut inventory: Inventory = if is_new {
            Inventory::default()
        } else {
            toml::from_str(&fs::read_to_string(root.join(FILES))?)?
        };
        let old_inventory = toml::to_string(&inventory)?;
        let temporary = tempfile::tempdir_in(self.home.join("tmp"))?;
        let staged = temporary.path().join("new");
        fs::create_dir(&staged)?;
        let mut owned = inventory.files.values().flatten().cloned().collect();
        let mut additions = Vec::new();
        for name in &added {
            let archive = self.archive(&state.manifest_url, &state.manifest.archives[name])?;
            let files =
                unpack(&archive, &staged, &mut owned).with_context(|| format!("Extract {name}"))?;
            additions.extend(files.iter().cloned());
            inventory.files.insert(name.clone(), files);
        }
        let mut removals = Vec::new();
        for name in &removed {
            removals.extend(
                inventory
                    .files
                    .remove(name)
                    .context("Missing component file inventory")?,
            );
        }
        let mut next = state.clone();
        next.components = desired;
        if is_new {
            ensure!(
                !root.exists(),
                "Installation directory already exists: {}",
                root.display()
            );
            write_toml(&staged.join(FILES), &inventory)?;
            write_toml(&staged.join(STATE), &next)?;
            fs::rename(&staged, &root)?;
        } else {
            let backup = temporary.path().join("removed");
            fs::create_dir(&backup)?;
            move_files(&root, &backup, &removals)?;
            if let Err(error) = move_files(&staged, &root, &additions) {
                move_files(&backup, &root, &removals)?;
                return Err(error);
            }
            let commit = write_toml(&root.join(FILES), &inventory)
                .and_then(|()| write_toml(&root.join(STATE), &next));
            if let Err(error) = commit {
                move_files(&root, &staged, &additions)?;
                move_files(&backup, &root, &removals)?;
                write_toml(
                    &root.join(FILES),
                    &toml::from_str::<Inventory>(&old_inventory)?,
                )?;
                return Err(error);
            }
            prune(&root, &removals);
        }
        *state = next;
        println!("Installed {}", state.manifest.version);
        Ok(true)
    }

    pub fn targets_change(
        &self,
        version: &str,
        targets: &[String],
        add: bool,
        skip_docs: bool,
    ) -> Result<()> {
        let _guard = self.toolchain_lock(version)?;
        let mut state = self.state(version)?;
        let mut desired = state.components.clone();
        for target in targets {
            let selection = state.manifest.target(&self.host, target)?;
            let mut components = state
                .manifest
                .target_components(&self.host, target, false, false)?;
            if !add {
                components.extend(selection.gui.clone());
                components.extend(selection.profiling.clone());
            }
            if add {
                desired.extend(components);
            } else {
                desired.retain(|item| !components.contains(item));
            }
        }
        let changed = self.apply(&mut state, desired)?;
        if changed && add && !skip_docs {
            self.try_docs(&state, targets);
        }
        if changed && !add {
            self.clear_docs(&state, targets)?;
        }
        Ok(())
    }

    pub fn component_change(
        &self,
        version: &str,
        name: &str,
        target: Option<&str>,
        add: bool,
        skip_docs: bool,
    ) -> Result<()> {
        let _guard = self.toolchain_lock(version)?;
        let mut state = self.state(version)?;
        let component = state.manifest.component(&self.host, name, target)?;
        let mut desired = state.components.clone();
        if add {
            desired.insert(component);
        } else {
            desired.remove(&component);
        }
        let changed = self.apply(&mut state, desired)?;
        if changed
            && name == "gui"
            && let Some(target) = target
        {
            self.clear_docs(&state, &[target.into()])?;
            if !skip_docs {
                self.try_docs(&state, &[target.into()]);
            }
        }
        Ok(())
    }

    pub fn remove(&self, version: &str) -> Result<()> {
        let _guard = self.toolchain_lock(version)?;
        let _settings = lock(&self.home.join("locks/settings.lock"))?;
        ensure!(
            self.settings()?.default.as_deref() != Some(version),
            "Select another default before removing {version}"
        );
        self.state(version)?;
        let root = self.root(version)?;
        let temporary = tempfile::tempdir_in(self.home.join("tmp"))?;
        fs::rename(root, temporary.path().join("removed-toolchain"))?;
        println!("Removed toolchain {version}");
        Ok(())
    }

    pub fn docs(&self, state: &State, targets: &[String]) -> Result<()> {
        let root = self.root(&state.manifest.version)?;
        let selected = if targets.is_empty() {
            state.targets()
        } else {
            targets.to_vec()
        };
        ensure!(!selected.is_empty(), "No installed targets to document");
        let mut command;
        if cfg!(windows) {
            command = Command::new("powershell.exe");
            command
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(root.join("post-install.ps1"));
        } else {
            command = Command::new("sh");
            command.arg(root.join("post-install.sh"));
        }
        for target in &selected {
            command.arg("--target").arg(target);
        }
        let status = command
            .current_dir(&root)
            .status()
            .context("Run API documentation generator")?;
        ensure!(status.success(), "API documentation exited with {status}");
        Ok(())
    }

    fn try_docs(&self, state: &State, targets: &[String]) {
        if let Err(error) = self.docs(state, targets) {
            eprintln!(
                "Toolchain installed; API documentation failed: {error:#}\nRetry: scoop-install docs --toolchain {}",
                state.manifest.version
            );
        }
    }

    fn clear_docs(&self, state: &State, targets: &[String]) -> Result<()> {
        let docs = self.root(&state.manifest.version)?.join("docs");
        for target in targets {
            let path = docs.join(target);
            if path.is_dir() {
                fs::remove_dir_all(path)?;
            }
        }
        if docs.is_dir() {
            let mut index = String::from(
                "<!doctype html><meta charset=\"utf-8\"><title>Scoop API</title><h1>Scoop API</h1><ul>",
            );
            for target in state.targets() {
                if docs.join(&target).join("index.html").is_file() {
                    index.push_str(&format!(
                        "<li><a href=\"{target}/index.html\">{target}</a></li>"
                    ));
                }
            }
            index.push_str("</ul>\n");
            fs::write(docs.join("index.html"), index)?;
        }
        Ok(())
    }
}
