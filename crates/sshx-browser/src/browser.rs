//! Browser process management for sshx-browser.
//!
//! Supports Firefox (preferred) and Chromium/Chrome as fallback.

use std::process::{Child, Command};

use anyhow::{Context, Result};
use tracing::info;

/// Launch a browser instance on the given X display.
///
/// The browser opens the specified `url` at the given display resolution.
/// Firefox is preferred (more reliable in Xvfb); Chromium/Chrome is used as fallback.
///
/// If `browser_bin` is `Some`, that exact binary is used; otherwise the first
/// candidate found on PATH is selected.
pub fn launch_browser(
    display: &str,
    url: &str,
    width: u32,
    height: u32,
    browser_bin: Option<&str>,
) -> Result<Child> {
    let binary = match browser_bin {
        Some(b) => b.to_string(),
        None => find_browser_binary()
            .context("could not find a browser; install firefox, chromium, or google-chrome-stable, or use --browser")?,
    };

    info!("using browser binary: {binary}");

    if is_firefox(&binary) {
        launch_firefox(display, url, width, height, &binary)
    } else {
        launch_chromium(display, url, width, height, &binary)
    }
}

/// Check if the binary name looks like Firefox.
fn is_firefox(binary: &str) -> bool {
    let name = binary.rsplit('/').next().unwrap_or(binary);
    name.starts_with("firefox")
}

/// Launch Firefox on the given X display.
fn launch_firefox(
    display: &str,
    url: &str,
    width: u32,
    height: u32,
    binary: &str,
) -> Result<Child> {
    let profile_dir = format!("/tmp/sshx-browser-ff-{}", std::process::id());
    std::fs::create_dir_all(&profile_dir).ok();

    // Write prefs to suppress first-run UI and set window size.
    let prefs_path = format!("{profile_dir}/user.js");
    std::fs::write(
        &prefs_path,
        format!(
            r#"user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("datareporting.policy.dataSubmissionEnabled", false);
user_pref("toolkit.telemetry.reportingpolicy.firstRun", false);
user_pref("browser.aboutwelcome.enabled", false);
user_pref("browser.tabs.warnOnClose", false);
user_pref("browser.sessionstore.resume_from_crash", false);
"#
        ),
    )
    .ok();

    let child = Command::new(binary)
        .env("DISPLAY", display)
        .args([
            "--no-remote",
            "-profile",
            &profile_dir,
            &format!("--width={width}"),
            &format!("--height={height}"),
            url,
        ])
        .spawn()
        .with_context(|| format!("failed to spawn {binary}"))?;

    // Give Firefox a moment to start and render.
    std::thread::sleep(std::time::Duration::from_millis(2000));

    Ok(child)
}

/// Launch Chromium/Chrome on the given X display.
fn launch_chromium(
    display: &str,
    url: &str,
    width: u32,
    height: u32,
    binary: &str,
) -> Result<Child> {
    // Use a per-process temp dir for user data so --disable-web-security is accepted
    // (Chrome requires a non-default --user-data-dir alongside that flag).
    let user_data_dir = format!("/tmp/sshx-browser-{}", std::process::id());

    let child = Command::new(binary)
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
            // Use the system DNS resolver instead of Chrome's async one, which
            // can hang when IPv6 is unreachable (tries AAAA records first).
            "--disable-features=AsyncDns",
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

/// Try to find a browser binary on the PATH.
/// Prefers Firefox over Chromium/Chrome for better Xvfb compatibility.
fn find_browser_binary() -> Option<String> {
    let candidates = [
        "firefox",
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
