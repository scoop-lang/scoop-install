use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Archive {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Host {
    #[serde(rename = "default-target")]
    pub default_target: String,
    pub targets: Vec<String>,
    pub bin: String,
    #[serde(default)]
    pub requirements: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Target {
    pub base: String,
    pub gui: Option<String>,
    pub native: Option<String>,
    #[serde(default)]
    pub requirements: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Manifest {
    pub schema: u32,
    pub release: String,
    pub version: String,
    pub revision: String,
    pub sources: String,
    pub vendor: String,
    pub hosts: BTreeMap<String, Host>,
    pub targets: BTreeMap<String, Target>,
    pub archives: BTreeMap<String, Archive>,
}

pub fn valid_name(value: &str) -> Result<()> {
    ensure!(
        value.starts_with(|c: char| c.is_ascii_alphanumeric())
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b)),
        "Invalid version or component name: {value}"
    );
    Ok(())
}

impl Manifest {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema == 1,
            "Unsupported manifest schema {}",
            self.schema
        );
        valid_name(&self.version)?;
        valid_name(&self.release)?;
        ensure!(!self.revision.is_empty(), "Manifest has no source revision");
        ensure!(!self.hosts.is_empty(), "Manifest has no hosts");
        let mut references = BTreeSet::from([self.sources.as_str(), self.vendor.as_str()]);
        for (name, host) in &self.hosts {
            valid_name(name)?;
            ensure!(
                host.targets.contains(&host.default_target),
                "Default target missing for {name}"
            );
            references.insert(&host.bin);
            for target in &host.targets {
                ensure!(self.targets.contains_key(target), "Missing target {target}");
            }
        }
        for (name, target) in &self.targets {
            valid_name(name)?;
            references.insert(&target.base);
            references.extend(target.gui.as_deref());
            references.extend(target.native.as_deref());
        }
        for component in references {
            ensure!(
                self.archives.contains_key(component),
                "Missing archive {component}"
            );
        }
        for (name, archive) in &self.archives {
            valid_name(name)?;
            ensure!(
                archive.sha256.len() == 64 && archive.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
                "Invalid SHA-256 for {name}"
            );
            ensure!(!archive.path.is_empty(), "Missing archive path for {name}");
        }
        Ok(())
    }

    pub fn host(&self, host: &str) -> Result<&Host> {
        self.hosts
            .get(host)
            .ok_or_else(|| anyhow::anyhow!("Release {} has no tools for host {host}", self.version))
    }

    pub fn target(&self, host: &str, name: &str) -> Result<&Target> {
        ensure!(
            self.host(host)?.targets.iter().any(|target| target == name),
            "Target {name} is unavailable for {host} in {}",
            self.version
        );
        Ok(&self.targets[name])
    }

    pub fn target_components(&self, host: &str, name: &str, gui: bool) -> Result<BTreeSet<String>> {
        let target = self.target(host, name)?;
        let mut components = BTreeSet::from([target.base.clone()]);
        components.extend(target.native.clone());
        if gui {
            components.insert(target.gui.clone().ok_or_else(|| {
                anyhow::anyhow!("Target {name} has no GUI component in {}", self.version)
            })?);
        }
        Ok(components)
    }

    pub fn component(&self, host: &str, name: &str, target: Option<&str>) -> Result<String> {
        match name {
            "sources" if target.is_none() => Ok(self.sources.clone()),
            "vendor" if target.is_none() => Ok(self.vendor.clone()),
            "gui" => {
                let name =
                    target.ok_or_else(|| anyhow::anyhow!("GUI requires --target <target>"))?;
                self.target(host, name)?
                    .gui
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("Target {name} has no GUI component"))
            }
            _ => bail!("Expected sources, vendor, or gui with --target"),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct State {
    pub host: String,
    pub manifest_url: String,
    pub manifest: Manifest,
    pub components: BTreeSet<String>,
}

impl State {
    pub fn targets(&self) -> Vec<String> {
        self.manifest.hosts[&self.host]
            .targets
            .iter()
            .filter(|name| self.components.contains(&self.manifest.targets[*name].base))
            .cloned()
            .collect()
    }

    pub fn validate_selection(&self, components: &BTreeSet<String>) -> Result<()> {
        ensure!(
            components.contains(&self.manifest.host(&self.host)?.bin),
            "Host tools cannot be removed"
        );
        ensure!(
            components.contains(&self.manifest.sources),
            "Sources include required FFI headers and cannot be removed"
        );
        for target in self.manifest.targets.values() {
            if components.contains(&target.base) {
                if let Some(native) = &target.native {
                    ensure!(
                        components.contains(native),
                        "{} requires {native}",
                        target.base
                    );
                }
            } else if let Some(gui) = &target.gui {
                ensure!(!components.contains(gui), "{gui} requires {}", target.base);
            }
        }
        Ok(())
    }
}
