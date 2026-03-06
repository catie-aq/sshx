//! Chromium process management for sshx-browser.

use std::process::{Child, Command};

use anyhow::{Context, Result};
use tracing::info;

/// Launch a Chromium instance on the given X display.
///
/// Chromium runs in a kiosk-like mode with GPU disabled (appropriate for Xvfb)
/// and opens the specified `url` at the given display resolution.
///
/// If `browser_bin` is `Some`, that exact binary is used; otherwise the first
/// candidate found on PATH is selected.
pub fn launch_chromium(
    display: &str,
    url: &str,
    width: u32,
    height: u32,
    browser_bin: Option<&str>,
) -> Result<Child> {
    let binary = match browser_bin {
        Some(b) => b.to_string(),
        None => find_chromium_binary()
            .context("could not find a browser; install chromium or google-chrome-stable, or use --browser")?,
    };

    info!("using browser binary: {binary}");

    // Use a per-process temp dir for user data so --disable-web-security is accepted
    // (Chrome requires a non-default --user-data-dir alongside that flag).
    let user_data_dir = format!("/tmp/sshx-browser-{}", std::process::id());

    let child = Command::new(&binary)
        .env("DISPLAY", display)
        .args([
            "--no-sandbox",
            "--disable-dev-shm-usage",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-extensions",
            "--disable-translate",
            "--disable-infobars",
            "--disable-notifications",
            "--disable-popup-blocking",
            "--disable-session-crashed-bubble",
            "--disable-background-networking",
            "--disable-sync",
            "--disable-default-apps",
            "--disable-web-security",
            "--ignore-certificate-errors",
            &format!("--user-data-dir={user_data_dir}"),
            "--start-maximized",
            &format!("--window-size={width},{height}"),
            url,
        ])
        .spawn()
        .with_context(|| format!("failed to spawn {binary}"))?;

    // Give Chromium a moment to start and render.
    std::thread::sleep(std::time::Duration::from_millis(1500));

    Ok(child)
}

/// Try to find a Chromium or Chrome binary on the PATH.
fn find_chromium_binary() -> Option<String> {
    let candidates = [
        "google-chrome-stable",
        "google-chrome",
        "chromium",
        "chromium-browser",
        "chromium-freeworld",
    ];

    for name in candidates {
        if which_exists(name) {
            return Some(name.to_string());
        }
    }
    None
}

/// Check if a binary exists on the PATH using `which`.
fn which_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
