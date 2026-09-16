//! Self-update from the sshx server the client is configured to use.
//!
//! The server publishes `/releases/manifest.json` (written by
//! `scripts/publish-release.sh`), which lists the latest build for each
//! platform. `sshx update` installs it by replacing the running executable;
//! sessions also check for updates in the background.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Version of this binary; `scripts/publish-release.sh` bumps it on each release.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Platform label of this binary, matching manifest keys (e.g. `linux-x86_64`).
pub const PLATFORM: &str = env!("SSHX_PLATFORM");

/// Minimum delay between two background update checks.
const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Release manifest published by the server.
#[derive(Deserialize, Debug)]
struct Manifest {
    platforms: HashMap<String, Release>,
}

/// A published build of the sshx client for one platform.
#[derive(Deserialize, Debug, Clone)]
pub struct Release {
    /// Version string, in the same format as [`VERSION`].
    pub version: String,
    /// Gzipped executable, absolute or relative to the server URL.
    pub url: String,
    /// Hex-encoded SHA-256 of the file at `url`.
    pub sha256: String,
}

/// Parse `major.minor.patch`, ignoring any `-pre` or `+build` suffix.
fn parse_version(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.split(['+', '-']).next()?;
    let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
    let parsed = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(parsed)
}

/// Whether `candidate` is a strictly newer version than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(10))
        .build()?)
}

/// Fetch the latest release for this platform, or `None` if the server does
/// not publish one.
pub async fn latest_release(server: &str) -> Result<Option<Release>> {
    let url = format!("{}/releases/manifest.json", server.trim_end_matches('/'));
    let resp = http_client()?
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to fetch {url}"))?;
    // Servers without releases return 404, or the web app's HTML fallback.
    let is_html = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/html"));
    if resp.status() == reqwest::StatusCode::NOT_FOUND || is_html {
        return Ok(None);
    }
    let body = resp
        .error_for_status()
        .with_context(|| format!("failed to fetch {url}"))?
        .bytes()
        .await
        .with_context(|| format!("failed to fetch {url}"))?;
    let manifest: Manifest =
        serde_json::from_slice(&body).context("invalid release manifest")?;
    Ok(manifest.platforms.get(PLATFORM).cloned())
}

/// Download `release`, verify it and replace the running executable.
///
/// Returns the path of the replaced executable. Running processes keep using
/// the old binary; the update applies on the next launch.
pub async fn install(server: &str, release: &Release) -> Result<PathBuf> {
    let url = if release.url.starts_with("http://") || release.url.starts_with("https://") {
        release.url.clone()
    } else {
        format!(
            "{}/{}",
            server.trim_end_matches('/'),
            release.url.trim_start_matches('/')
        )
    };

    let compressed = http_client()?
        .get(&url)
        .send()
        .await
        .and_then(|resp| resp.error_for_status())
        .with_context(|| format!("failed to download {url}"))?
        .bytes()
        .await
        .context("error reading download body")?;

    let digest = hex_encode(&Sha256::digest(&compressed));
    if !digest.eq_ignore_ascii_case(release.sha256.trim()) {
        bail!("checksum mismatch for {url} (expected {}, got {digest})", release.sha256);
    }

    let exe = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .context("could not locate the running executable")?;
    let exe_clone = exe.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut contents = Vec::new();
        flate2::read::GzDecoder::new(&compressed[..])
            .read_to_end(&mut contents)
            .context("failed to decompress update")?;
        replace_exe(&exe_clone, &contents)
    })
    .await??;

    Ok(exe)
}

/// Atomically replace the executable at `exe` with `contents`.
fn replace_exe(exe: &Path, contents: &[u8]) -> Result<()> {
    let dir = exe.parent().context("executable has no parent directory")?;
    let tmp = dir.join(format!(".sshx-update-{}", std::process::id()));
    std::fs::write(&tmp, contents).with_context(|| {
        format!("cannot write to {} (try again with sudo)", dir.display())
    })?;

    let result = (|| -> std::io::Result<()> {
        std::fs::set_permissions(&tmp, std::fs::metadata(exe)?.permissions())?;
        // A running executable can be renamed on Windows, but not overwritten.
        #[cfg(windows)]
        {
            let old = old_exe_path(exe);
            let _ = std::fs::remove_file(&old);
            std::fs::rename(exe, &old)?;
        }
        std::fs::rename(&tmp, exe)
    })();

    if let Err(err) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(err).with_context(|| format!("failed to replace {}", exe.display()));
    }
    Ok(())
}

#[cfg(windows)]
fn old_exe_path(exe: &Path) -> PathBuf {
    exe.with_extension("old.exe")
}

/// Remove the previous executable left behind by an update on Windows.
pub fn cleanup_old_exe() {
    #[cfg(windows)]
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_exe_path(&exe));
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Check for an update and install it, at most once per hour.
///
/// Skipped for binaries run from a cargo `target/` directory, so development
/// builds are never overwritten. Returns the installed version, if any.
pub async fn auto_update(server: &str) -> Result<Option<String>> {
    let exe = std::env::current_exe()?;
    if exe.components().any(|c| c.as_os_str() == "target") {
        return Ok(None);
    }

    let stamp = dirs::home_dir()
        .context("could not determine home directory")?
        .join(".sshx")
        .join("last-update-check");
    let recently_checked = std::fs::metadata(&stamp)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|elapsed| elapsed < CHECK_INTERVAL);
    if recently_checked {
        return Ok(None);
    }
    if let Some(parent) = stamp.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&stamp, VERSION)?;

    match latest_release(server).await? {
        Some(release) if is_newer(&release.version, VERSION) => {
            install(server, &release).await?;
            Ok(Some(release.version))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_version("0.5.1"), Some((0, 5, 1)));
        assert_eq!(parse_version("0.5.0+332.e5e43d1"), Some((0, 5, 0)));
        assert_eq!(parse_version("1.2.3-beta"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.5"), None);
        assert_eq!(parse_version("0.5.0.1"), None);
    }

    #[test]
    fn test_is_newer() {
        assert!(is_newer("0.5.1", "0.5.0"));
        assert!(is_newer("0.5.10", "0.5.9"));
        assert!(is_newer("0.6.0", "0.5.9"));
        assert!(is_newer("0.5.1", "0.5.0+332.e5e43d1"));
        assert!(!is_newer("0.5.1", "0.5.1"));
        assert!(!is_newer("0.5.0", "0.5.1"));
        assert!(!is_newer("garbage", "0.5.0"));
    }

    #[test]
    fn test_version_and_platform() {
        assert!(parse_version(VERSION).is_some(), "unexpected version: {VERSION}");
        assert!(!PLATFORM.starts_with("unknown"), "unexpected platform: {PLATFORM}");
    }
}
