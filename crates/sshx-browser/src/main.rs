//! sshx-browser — offscreen Chromium browser streamer for sshx sessions.
//!
//! This binary:
//! 1. Starts an Xvfb virtual display.
//! 2. Launches Chromium headed on that virtual display.
//! 3. Connects to an sshx-server via gRPC (`BrowserService`).
//! 4. Captures the Xvfb display at ~30 fps, encodes VP8 via ffmpeg.
//! 5. Streams encoded frames to the server.
//! 6. Receives mouse/keyboard input events from the server and injects them
//!    via the X11 XTest extension.

#![forbid(unsafe_code)]

mod browser;
mod capture_loop;
mod grpc_client;

use anyhow::Result;
use clap::Parser;
use tracing::info;
use tracing_subscriber::EnvFilter;

/// CLI options for sshx-browser.
#[derive(Parser, Debug)]
#[command(author, version, about = "Offscreen browser streamer for sshx")]
pub struct Options {
    /// URL of the sshx server (e.g. http://localhost:8051).
    #[arg(long, env = "SSHX_SERVER")]
    pub server: String,

    /// Name of the sshx session to join.
    #[arg(long)]
    pub session: String,

    /// HMAC token authenticating this binary as the session owner.
    #[arg(long)]
    pub token: String,

    /// Initial URL to open in Chromium.
    #[arg(long, default_value = "https://example.com")]
    pub url: String,

    /// Virtual display width in pixels.
    #[arg(long, default_value_t = 1280)]
    pub width: u32,

    /// Virtual display height in pixels.
    #[arg(long, default_value_t = 720)]
    pub height: u32,

    /// Target frame rate for video capture.
    #[arg(long, default_value_t = 30)]
    pub fps: u32,

    /// Xvfb display number (e.g. 99 → DISPLAY=:99).
    #[arg(long, default_value_t = 99)]
    pub display_num: u32,

    /// Video bitrate in kbps.
    #[arg(long, default_value_t = 2000)]
    pub bitrate_kbps: u32,

    /// Browser binary to use (e.g. google-chrome-stable, chromium, /usr/bin/chrome).
    /// If not set, the first browser found on PATH is used.
    #[arg(long, env = "SSHX_BROWSER")]
    pub browser: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let opts = Options::parse();
    info!(?opts, "sshx-browser starting");

    // Start Xvfb.
    let xvfb = sshx_media::Xvfb::start(opts.display_num, opts.width, opts.height, 24)?;
    let disp = xvfb.display().to_string();
    info!("Xvfb started on DISPLAY={}", disp.as_str());

    // Launch browser.
    let _chrome = browser::launch_chromium(&disp, &opts.url, opts.width, opts.height, opts.browser.as_deref())?;
    info!("browser launched at {}", opts.url.as_str());

    // Connect to the sshx server and get our assigned vid.
    let vid = grpc_client::join_session(&opts).await?;
    info!("joined session as vid={}", vid);

    // Run the capture + streaming loop (blocks until the session ends).
    capture_loop::run(vid, &disp, &opts).await?;

    Ok(())
}
