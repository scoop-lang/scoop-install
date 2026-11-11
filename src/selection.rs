use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::model::valid_name;
use crate::store::Store;

#[derive(Deserialize)]
struct Project {
    toolchain: ProjectToolchain,
}

#[derive(Deserialize)]
struct ProjectToolchain {
    version: String,
    #[serde(default)]
    targets: Vec<String>,
}

pub struct Selection {
    pub version: String,
    pub reason: String,
}

impl Store {
    pub fn select(&self, explicit: Option<&str>, cwd: &Path) -> Result<Selection> {
        let mut required = Vec::new();
        let selection = if let Some(version) = explicit {
            Selection {
                version: version.into(),
                reason: "command line".into(),
            }
        } else if let Ok(version) = std::env::var("SCOOP_TOOLCHAIN") {
            Selection {
                version,
                reason: "SCOOP_TOOLCHAIN".into(),
            }
        } else {
            let mut project = None;
            for parent in cwd.ancestors() {
                let path = parent.join("scoop-toolchain.toml");
                match fs::read_to_string(&path) {
                    Ok(text) => {
                        let config: Project = toml::from_str(&text)
                            .with_context(|| format!("Read {}", path.display()))?;
                        required = config.toolchain.targets;
                        project = Some(Selection {
                            version: config.toolchain.version,
                            reason: path.display().to_string(),
                        });
                        break;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                    Err(error) => {
                        return Err(error).with_context(|| format!("Read {}", path.display()));
                    }
                }
            }
            if let Some(selection) = project {
                selection
            } else {
                Selection {
                    version: self.settings()?.default.context(
                        "No default toolchain; run scoop-install toolchain install latest",
                    )?,
                    reason: "default".into(),
                }
            }
        };
        valid_name(&selection.version)?;
        let state = self.state(&selection.version)?;
        for target in required {
            let resource = state.manifest.target(&self.host, &target)?;
            ensure!(
                state.components.contains(&resource.base),
                "Project requires target {target}; run scoop-install target add {target} --toolchain {}",
                selection.version
            );
        }
        Ok(selection)
    }

    pub fn proxies(&self, root: &Path) -> Result<()> {
        let bin = self.home.join("bin");
        fs::create_dir_all(&bin)?;
        let executable = std::env::current_exe()?;
        let manager_name = if cfg!(windows) {
            "scoop-install.exe"
        } else {
            "scoop-install"
        };
        let manager = bin.join(manager_name);
        let fresh = !manager.is_file();
        if fresh {
            let temporary = tempfile::tempdir_in(self.home.join("tmp"))?;
            fs::copy(executable, temporary.path().join(manager_name))?;
            fs::rename(temporary.path().join(manager_name), &manager)?;
        }
        let notices = self
            .home
            .join("share/licenses/scoop-install/THIRD-PARTY.txt");
        if !notices.is_file() {
            fs::create_dir_all(notices.parent().context("License directory is missing")?)?;
            fs::write(notices, include_str!("../licenses/THIRD-PARTY.txt"))?;
        }
        for entry in fs::read_dir(root.join("bin"))? {
            let entry = entry?;
            let name = entry.file_name();
            let text = name.to_string_lossy();
            if !(text == "scoop"
                || text == "sco.exe"
                || text == "scoopc"
                || text == "scoopc.exe"
                || text.starts_with("scoop-"))
                || name == manager_name
            {
                continue;
            }
            if cfg!(windows) && !text.ends_with(".exe") {
                continue;
            }
            let destination = bin.join(&name);
            if !fresh && destination.exists() {
                continue;
            }
            let temporary = tempfile::tempdir_in(self.home.join("tmp"))?;
            let proxy = temporary.path().join(&name);
            #[cfg(unix)]
            std::os::unix::fs::symlink(manager_name, &proxy)?;
            #[cfg(windows)]
            if fs::hard_link(&manager, &proxy).is_err() {
                fs::copy(&manager, &proxy)?;
            }
            // The first managed install replaces the former flat public launchers.
            if destination.exists() {
                fs::remove_file(&destination)?;
            }
            fs::rename(proxy, destination)?;
        }
        Ok(())
    }

    pub fn run(&self, version: &str, name: &str, args: &[OsString]) -> Result<i32> {
        ensure!(
            !name.contains(['/', '\\']) && name != "." && name != "..",
            "Expected a tool name, not a path"
        );
        let state = self.state(version)?;
        let root = self.root(version)?;
        let name = if cfg!(windows) {
            if name == "scoop" {
                "sco.exe".into()
            } else if name.ends_with(".exe") {
                name.into()
            } else {
                format!("{name}.exe")
            }
        } else {
            name.to_owned()
        };
        let path = root.join("bin").join(&name);
        ensure!(path.is_file(), "Tool {name} is not available in {version}");
        let mut command = Command::new(path);
        command
            .args(args)
            .env("SCOOP_TOOLCHAIN", version)
            .env("SCOOP_HOME", &self.home)
            .env("SCOOP_INSTALL_HOST", &self.host)
            .env(
                "SCOOP_TOOLCHAIN_ID",
                format!(
                    "{}:{}:{}:{}",
                    state.manifest.release, version, state.manifest.revision, self.host
                ),
            );
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            Err(command.exec()).context("Execute selected toolchain")
        }
        #[cfg(windows)]
        {
            Ok(command.status()?.code().unwrap_or(1))
        }
    }
}
