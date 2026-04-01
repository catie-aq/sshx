//! Auto-download and manage the OpenVSCode Server binary.
//!
//! When `--ide` is passed, sshx checks `~/.sshx/openvscode-server/` for a
//! matching version. If missing or outdated, it downloads the correct release
//! tarball from GitHub, extracts it, and returns the binary path.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use tracing::info;

/// Pinned OpenVSCode Server version.
const OPENVSCODE_VERSION: &str = "1.109.5";

/// GitHub release download base URL.
const DOWNLOAD_BASE: &str =
    "https://github.com/gitpod-io/openvscode-server/releases/download";

/// Filename written inside the install directory to track the installed version.
const VERSION_MARKER: &str = ".version";

/// Return the platform slug used in the Gitpod release tarball name.
///
/// Supported: `linux-x64`, `linux-arm64`, `darwin-x64`, `darwin-arm64`.
fn platform_slug() -> Result<&'static str> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    { return Ok("linux-x64"); }

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    { return Ok("linux-arm64"); }

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    { return Ok("darwin-x64"); }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    { return Ok("darwin-arm64"); }

    #[allow(unreachable_code)]
    {
        bail!(
            "OpenVSCode Server is not available for this platform ({}/{})",
            std::env::consts::OS,
            std::env::consts::ARCH,
        );
    }
}

/// Base install directory: `~/.sshx/openvscode-server/`.
fn install_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("could not determine home directory")?;
    Ok(home.join(".sshx").join("openvscode-server"))
}

/// Read the version marker file, returning `None` if missing/unreadable.
fn installed_version(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(VERSION_MARKER))
        .ok()
        .map(|s| s.trim().to_string())
}

/// Ensure the OpenVSCode Server binary is available at the expected path.
///
/// Downloads and extracts the release tarball on first use or version mismatch.
/// Returns the path to `bin/openvscode-server`.
pub async fn ensure_binary() -> Result<PathBuf> {
    let dir = install_dir()?;
    let bin = dir.join("bin").join("openvscode-server");

    // Fast path: already installed and version matches.
    if bin.exists() {
        if let Some(v) = installed_version(&dir) {
            if v == OPENVSCODE_VERSION {
                info!("OpenVSCode Server v{OPENVSCODE_VERSION} already installed");
                return Ok(bin);
            }
            info!("version mismatch (installed: {v}, expected: {OPENVSCODE_VERSION}), upgrading");
        }
    }

    let platform = platform_slug()?;
    let tag = format!("openvscode-server-v{OPENVSCODE_VERSION}");
    let tarball_name = format!("{tag}-{platform}.tar.gz");
    let url = format!("{DOWNLOAD_BASE}/{tag}/{tarball_name}");

    // Clean up any previous install or leftover temp directory.
    let tmp_dir = dir.with_extension("tmp");
    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir)
            .context("failed to remove leftover .tmp directory")?;
    }
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .context("failed to remove old installation")?;
    }
    std::fs::create_dir_all(&tmp_dir)
        .context("failed to create temp directory")?;

    // Download.
    eprintln!("\n  Downloading OpenVSCode Server v{OPENVSCODE_VERSION} ({platform})...");
    let bytes = download_with_progress(&url).await?;
    eprintln!("  Extracting...");

    // Extract (sync — tar/flate2 are blocking).
    let tmp_dir_clone = tmp_dir.clone();
    let nested_name = format!("{tag}-{platform}");
    tokio::task::spawn_blocking(move || -> Result<()> {
        let decoder = flate2::read::GzDecoder::new(&bytes[..]);
        let mut archive = tar::Archive::new(decoder);
        archive.unpack(&tmp_dir_clone)?;

        // The tarball extracts to a nested directory — flatten it.
        let nested = tmp_dir_clone.join(&nested_name);
        if nested.exists() {
            for entry in std::fs::read_dir(&nested)? {
                let entry = entry?;
                let dest = tmp_dir_clone.join(entry.file_name());
                std::fs::rename(entry.path(), dest)?;
            }
            std::fs::remove_dir(&nested)?;
        }
        Ok(())
    })
    .await??;

    // Write version marker.
    std::fs::write(tmp_dir.join(VERSION_MARKER), OPENVSCODE_VERSION)?;

    // Verify the binary exists.
    let tmp_bin = tmp_dir.join("bin").join("openvscode-server");
    if !tmp_bin.exists() {
        bail!(
            "extraction succeeded but bin/openvscode-server not found in {}",
            tmp_dir.display()
        );
    }

    // Atomic rename .tmp → final.
    std::fs::rename(&tmp_dir, &dir).with_context(|| {
        format!(
            "failed to rename {} → {}",
            tmp_dir.display(),
            dir.display()
        )
    })?;

    eprintln!(
        "  Installed to {}\n",
        dir.display()
    );

    Ok(bin)
}

/// Download a URL, printing the total size when known.
async fn download_with_progress(url: &str) -> Result<Vec<u8>> {
    let resp = reqwest::get(url)
        .await
        .with_context(|| format!("failed to GET {url}"))?;

    if !resp.status().is_success() {
        bail!("download failed: HTTP {}", resp.status());
    }

    if let Some(total) = resp.content_length() {
        let mb = total as f64 / 1_048_576.0;
        eprintln!("  Downloading {mb:.1} MB...");
    }

    let bytes = resp
        .bytes()
        .await
        .context("error reading download body")?;

    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_slug() {
        let slug = platform_slug().expect("should detect platform");
        assert!(
            ["linux-x64", "linux-arm64", "darwin-x64", "darwin-arm64"].contains(&slug),
            "unexpected slug: {slug}"
        );
    }

    #[test]
    fn test_install_dir() {
        let dir = install_dir().expect("should resolve home dir");
        assert!(dir.ends_with("openvscode-server"));
        assert!(dir.to_string_lossy().contains(".sshx"));
    }
}
