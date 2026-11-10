use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;

use crate::model::{Archive, Manifest, valid_name};
use crate::store::{Store, lock};

pub fn location(value: &str) -> Result<Url> {
    if value.starts_with("https://") || value.starts_with("http://") || value.starts_with("file://")
    {
        return Ok(Url::parse(value)?);
    }
    let path = Path::new(value);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Url::from_file_path(absolute).map_err(|()| anyhow::anyhow!("Invalid manifest path: {value}"))
}

pub fn reader(url: &Url) -> Result<Box<dyn Read>> {
    match url.scheme() {
        "file" => Ok(Box::new(File::open(
            url.to_file_path()
                .map_err(|()| anyhow::anyhow!("Invalid file URL"))?,
        )?)),
        "https" | "http" => Ok(Box::new(
            ureq::get(url.as_str())
                .call()
                .with_context(|| format!("Download {url}"))?
                .into_body()
                .into_reader(),
        )),
        scheme => anyhow::bail!("Unsupported download URL scheme: {scheme}"),
    }
}

fn text(url: &Url) -> Result<String> {
    let mut text = String::new();
    reader(url)?.read_to_string(&mut text)?;
    Ok(text)
}

#[derive(Deserialize)]
struct Latest {
    schema: u32,
    release: String,
    manifest: String,
}

impl Store {
    pub fn manifest(&self, version: &str, explicit: Option<&str>) -> Result<(Manifest, String)> {
        let (url, release) = if let Some(explicit) = explicit {
            (location(explicit)?, None)
        } else {
            let base = location(&format!("{}/", self.server.trim_end_matches('/')))?;
            if version == "latest" {
                let latest_url = base.join("latest.toml")?;
                let latest: Latest = toml::from_str(&text(&latest_url)?)?;
                ensure!(latest.schema == 1, "Unsupported latest manifest schema");
                (latest_url.join(&latest.manifest)?, Some(latest.release))
            } else {
                valid_name(version)?;
                (base.join(&format!("versions/{version}.toml"))?, None)
            }
        };
        let manifest: Manifest =
            toml::from_str(&text(&url)?).with_context(|| format!("Parse manifest {url}"))?;
        manifest.validate()?;
        ensure!(
            version == "latest" || version == manifest.version,
            "Requested {version}, manifest contains {}",
            manifest.version
        );
        if let Some(release) = release {
            ensure!(
                release == manifest.release,
                "Latest points to a different release"
            );
        }
        manifest.host(&self.host)?;
        Ok((manifest, url.to_string()))
    }

    pub fn archive(&self, manifest_url: &str, archive: &Archive) -> Result<PathBuf> {
        let path = self
            .home
            .join("downloads")
            .join(format!("{}.tar.xz", archive.sha256));
        if path.is_file() {
            return Ok(path);
        }
        let _guard = lock(&path.with_extension("lock"))?;
        if path.is_file() {
            return Ok(path);
        }
        let url = Url::parse(manifest_url)?.join(&archive.path)?;
        eprintln!("Downloading {url} ({} bytes)", archive.size);
        let mut staged = tempfile::NamedTempFile::new_in(self.home.join("downloads"))?;
        let mut source = reader(&url)?;
        let mut digest = Sha256::new();
        let mut size = 0;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = source.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            staged.write_all(&buffer[..count])?;
            digest.update(&buffer[..count]);
            size += count as u64;
        }
        ensure!(
            size == archive.size,
            "Incomplete archive {url}: {size} bytes, expected {}",
            archive.size
        );
        ensure!(
            digest
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
                .eq_ignore_ascii_case(&archive.sha256),
            "SHA-256 mismatch for {url}"
        );
        staged.persist(&path).map_err(|error| error.error)?;
        Ok(path)
    }
}
