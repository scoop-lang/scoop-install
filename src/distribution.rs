use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;

use crate::model::{Archive, Manifest, valid_name};
use crate::progress::DownloadProgress;
use crate::store::{Store, lock};

const DOWNLOAD_ATTEMPTS: u32 = 4;
const NETWORK_TIMEOUT: Duration = Duration::from_secs(30);

// Mark only request/response failures, so local filesystem errors never retry.
#[derive(Debug)]
struct TransientDownload;

impl std::fmt::Display for TransientDownload {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Temporary download failure")
    }
}

fn download_error(error: ureq::Error) -> anyhow::Error {
    let retryable = matches!(
        error,
        ureq::Error::StatusCode(408 | 429 | 500..=599)
            | ureq::Error::Io(_)
            | ureq::Error::Timeout(_)
            | ureq::Error::HostNotFound
            | ureq::Error::ConnectionFailed
            | ureq::Error::ConnectProxyFailed(_)
            | ureq::Error::Protocol(_)
    );
    let error = anyhow::Error::new(error);
    if retryable {
        error.context(TransientDownload)
    } else {
        error
    }
}

fn with_retry<T>(url: &Url, mut download: impl FnMut() -> Result<T>) -> Result<T> {
    for attempt in 1..=DOWNLOAD_ATTEMPTS {
        match download() {
            Ok(value) => return Ok(value),
            Err(error) => {
                if !matches!(url.scheme(), "http" | "https") || !error.is::<TransientDownload>() {
                    return Err(error).with_context(|| format!("Download {url}"));
                }
                if attempt == DOWNLOAD_ATTEMPTS {
                    return Err(error).with_context(|| {
                        format!("Download {url} failed after {attempt} attempts")
                    });
                }
                let delay = Duration::from_secs(1 << (attempt - 1));
                eprintln!(
                    "Download {url} failed (attempt {attempt}/{DOWNLOAD_ATTEMPTS}): {error:#}; retrying in {}s",
                    delay.as_secs()
                );
                std::thread::sleep(delay);
            }
        }
    }
    unreachable!()
}

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
                .config()
                .timeout_resolve(Some(NETWORK_TIMEOUT))
                .timeout_connect(Some(NETWORK_TIMEOUT))
                .timeout_send_request(Some(NETWORK_TIMEOUT))
                .timeout_recv_response(Some(NETWORK_TIMEOUT))
                .timeout_per_read(Some(NETWORK_TIMEOUT))
                .build()
                .call()
                .map_err(download_error)?
                .into_body()
                .into_reader(),
        )),
        scheme => anyhow::bail!("Unsupported download URL scheme: {scheme}"),
    }
}

fn text(url: &Url) -> Result<String> {
    with_retry(url, || {
        let mut bytes = Vec::new();
        reader(url)?
            .read_to_end(&mut bytes)
            .map_err(|error| download_error(error.into()))?;
        String::from_utf8(bytes).context("Manifest is not valid UTF-8")
    })
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
        with_retry(&url, || {
            let mut staged = tempfile::NamedTempFile::new_in(self.home.join("downloads"))?;
            let mut source = reader(&url)?;
            let mut progress = DownloadProgress::new(archive.size);
            let mut digest = Sha256::new();
            let mut size = 0;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let count = match source.read(&mut buffer) {
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    result => result.map_err(|error| download_error(error.into()))?,
                };
                if count == 0 {
                    break;
                }
                staged.write_all(&buffer[..count])?;
                digest.update(&buffer[..count]);
                size += count as u64;
                progress.set_position(size);
            }
            if size != archive.size {
                return Err(anyhow::anyhow!(
                    "Incomplete archive {url}: {size} bytes, expected {}",
                    archive.size
                )
                .context(TransientDownload));
            }
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
            progress.finish();
            Ok(())
        })?;
        Ok(path)
    }
}
