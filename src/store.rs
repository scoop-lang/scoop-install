use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::model::{State, valid_name};

pub const STATE: &str = ".scoop-install/state.toml";
pub const FILES: &str = ".scoop-install/files.toml";

#[derive(Default, Deserialize, Serialize)]
pub struct Settings {
    pub default: Option<String>,
}

pub struct Store {
    pub home: PathBuf,
    pub host: String,
    pub server: String,
}

pub fn host() -> Result<String> {
    let name = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "x86_64") if cfg!(target_env = "musl") => "x86_64-unknown-linux-musl",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("windows", "x86_64") => "x86_64-pc-windows-gnullvm",
        (os, arch) => anyhow::bail!("No Scoop host is defined for {arch}-{os}"),
    };
    Ok(name.into())
}

pub fn home() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("SCOOP_HOME") {
        return Ok(path.into());
    }
    let executable = std::env::current_exe()?;
    if let Some(bin) = executable.parent()
        && bin.file_name().is_some_and(|name| name == "bin")
        && let Some(root) = bin.parent()
        && root.join("settings.toml").is_file()
    {
        return Ok(root.to_owned());
    }
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(|path| PathBuf::from(path).join(".scoop"))
        .context("Set SCOOP_HOME or your user home directory")
}

pub fn lock(path: &Path) -> Result<File> {
    fs::create_dir_all(path.parent().context("Lock has no parent")?)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    file.lock()
        .with_context(|| format!("Lock {}", path.display()))?;
    Ok(file)
}

pub fn write_toml(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("Output has no parent")?;
    fs::create_dir_all(parent)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(toml::to_string_pretty(value)?.as_bytes())?;
    staged.persist(path).map_err(|error| error.error)?;
    Ok(())
}

impl Store {
    pub fn new(home: PathBuf, host: String, server: String) -> Result<Self> {
        valid_name(&host)?;
        let home = if home.is_absolute() {
            home
        } else {
            std::env::current_dir()?.join(home)
        };
        for directory in ["toolchains", "downloads", "tmp", "locks"] {
            fs::create_dir_all(home.join(directory))?;
        }
        Ok(Self { home, host, server })
    }

    pub fn root(&self, version: &str) -> Result<PathBuf> {
        valid_name(version)?;
        Ok(self
            .home
            .join("toolchains")
            .join(format!("{version}-{}", self.host)))
    }

    pub fn state(&self, version: &str) -> Result<State> {
        let root = self.root(version)?;
        let text = fs::read_to_string(root.join(STATE)).with_context(|| format!("Toolchain {version} is not installed; run scoop-install toolchain install {version}"))?;
        Ok(toml::from_str(&text)?)
    }

    pub fn toolchain_lock(&self, version: &str) -> Result<File> {
        valid_name(version)?;
        lock(
            &self
                .home
                .join("locks")
                .join(format!("{version}-{}.lock", self.host)),
        )
    }

    pub fn settings(&self) -> Result<Settings> {
        let path = self.home.join("settings.toml");
        match fs::read_to_string(path) {
            Ok(text) => Ok(toml::from_str(&text)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn set_default(&self, version: &str, only_if_empty: bool) -> Result<()> {
        let _guard = lock(&self.home.join("locks/settings.lock"))?;
        ensure!(
            self.root(version)?.join(STATE).is_file(),
            "Toolchain {version} is not installed"
        );
        let mut settings = self.settings()?;
        if !only_if_empty || settings.default.is_none() {
            settings.default = Some(version.into());
            write_toml(&self.home.join("settings.toml"), &settings)?;
            println!("Default toolchain: {version}");
        }
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<State>> {
        let mut result = Vec::new();
        for entry in fs::read_dir(self.home.join("toolchains"))? {
            let path = entry?.path().join(STATE);
            if path.is_file() {
                let state: State = toml::from_str(&fs::read_to_string(path)?)?;
                if state.host == self.host {
                    result.push(state);
                }
            }
        }
        result.sort_by(|a, b| a.manifest.version.cmp(&b.manifest.version));
        Ok(result)
    }
}
