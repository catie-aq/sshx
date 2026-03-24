//! Xvfb (X virtual framebuffer) lifecycle management.

use std::process::{Child, Command};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tracing::{info, warn};

/// Manages the lifecycle of an Xvfb virtual display process.
///
/// The Xvfb process is killed when this struct is dropped.
pub struct Xvfb {
    child: Child,
    display: String,
}

impl Xvfb {
    /// Start an Xvfb process on the given display number.
    ///
    /// `display_num` should be a number ≥ 1, e.g. `99` for `:99`.
    /// `width`, `height`, and `depth` configure the virtual display geometry.
    pub fn start(display_num: u32, width: u32, height: u32, depth: u8) -> Result<Self> {
        let display = format!(":{display_num}");

        // Clean up stale lock files from previous runs so Xvfb can start.
        let lock_file = format!("/tmp/.X{display_num}-lock");
        let socket_file = format!("/tmp/.X11-unix/X{display_num}");
        if std::path::Path::new(&lock_file).exists() {
            warn!("removing stale lock file: {lock_file}");
            let _ = std::fs::remove_file(&lock_file);
        }
        if std::path::Path::new(&socket_file).exists() {
            warn!("removing stale socket: {socket_file}");
            let _ = std::fs::remove_file(&socket_file);
        }

        let child = Command::new("Xvfb")
            .arg(&display)
            .arg("-screen")
            .arg("0")
            .arg(format!("{width}x{height}x{depth}"))
            .arg("-nolisten")
            .arg("tcp")
            .spawn()
            .context("failed to spawn Xvfb; ensure it is installed")?;

        let disp = &display;
        info!("started Xvfb on display {disp} at {width}x{height}x{depth}");

        // Wait until the X11 socket appears (up to 3 seconds).
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            if std::path::Path::new(&socket_file).exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if !std::path::Path::new(&socket_file).exists() {
            bail!("Xvfb failed to create socket {socket_file} within 3 seconds");
        }

        Ok(Self { child, display })
    }

    /// Return the DISPLAY string for this virtual display (e.g. ":99").
    pub fn display(&self) -> &str {
        &self.display
    }

    /// Block until Xvfb exits.
    pub fn wait(mut self) -> Result<()> {
        let status = self.child.wait()?;
        if !status.success() {
            bail!("Xvfb exited with status: {status}");
        }
        Ok(())
    }
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        if let Err(e) = self.child.kill() {
            warn!("failed to kill Xvfb: {e}");
        }
        let _ = self.child.wait();
        let disp = &self.display;
        info!("Xvfb on display {disp} terminated");
    }
}
