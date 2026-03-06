//! Screen capture abstraction.

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// A raw video frame captured from a display.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Raw BGRA pixel data.
    pub data: Vec<u8>,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Capture timestamp (microseconds since an arbitrary epoch).
    pub timestamp_us: u64,
}

/// Trait for screen capture backends.
///
/// Implementors provide a stream of raw [`Frame`]s from a display source.
pub trait FrameSource: Send + 'static {
    /// Capture the next frame from the display.
    ///
    /// Blocks until a frame is available; returns `None` if the source has ended.
    fn next_frame(&mut self) -> Result<Option<Frame>>;

    /// Return the native resolution of this capture source.
    fn resolution(&self) -> (u32, u32);
}

/// A simple X11 screen capture source using the `xcap` crate.
///
/// This wraps the `xcap::Monitor` API to capture the primary monitor on
/// a given DISPLAY. On Linux with Xvfb this can target a virtual display.
#[cfg(target_os = "linux")]
pub struct XcapSource {
    display: String,
    width: u32,
    height: u32,
    frame_interval_us: u64,
    last_capture: std::time::Instant,
    start: std::time::Instant,
}

#[cfg(target_os = "linux")]
impl XcapSource {
    /// Create a new capture source for the given DISPLAY (e.g. ":99").
    ///
    /// `fps` controls the target frame rate. Actual rate depends on system load.
    pub fn new(display: &str, width: u32, height: u32, fps: u32) -> Result<Self> {
        Ok(Self {
            display: display.to_string(),
            width,
            height,
            frame_interval_us: 1_000_000 / fps.max(1) as u64,
            last_capture: std::time::Instant::now(),
            start: std::time::Instant::now(),
        })
    }

    /// Get the DISPLAY string for this source.
    pub fn display(&self) -> &str {
        &self.display
    }
}

#[cfg(target_os = "linux")]
impl FrameSource for XcapSource {
    fn next_frame(&mut self) -> Result<Option<Frame>> {
        // Rate-limit to the configured fps.
        let elapsed = self.last_capture.elapsed();
        let interval = std::time::Duration::from_micros(self.frame_interval_us);
        if elapsed < interval {
            std::thread::sleep(interval - elapsed);
        }
        self.last_capture = std::time::Instant::now();
        let timestamp_us = self.start.elapsed().as_micros() as u64;

        // Use the `xcap` crate to grab the X11 display.
        // We set DISPLAY in the environment before calling xcap.
        //
        // NOTE: xcap uses XGetImage internally; it must be called with the
        // correct DISPLAY set. We temporarily override the env var here.
        // This is not thread-safe; sshx-browser runs capture on a dedicated thread.
        let prev_display = std::env::var("DISPLAY").ok();
        std::env::set_var("DISPLAY", &self.display);

        let result = xcap_capture(self.width, self.height, timestamp_us);

        // Restore DISPLAY.
        match prev_display {
            Some(d) => std::env::set_var("DISPLAY", d),
            None => std::env::remove_var("DISPLAY"),
        }

        result
    }

    fn resolution(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// Capture a screenshot using the `xcap` crate.
///
/// This is in a free function so it can be called without `self`.
#[cfg(target_os = "linux")]
fn xcap_capture(width: u32, height: u32, timestamp_us: u64) -> anyhow::Result<Option<Frame>> {
    use anyhow::Context;

    let monitors = xcap::Monitor::all().context("failed to enumerate monitors")?;
    let monitor = monitors.into_iter().next().context("no monitors found")?;
    let image = monitor.capture_image().context("capture failed")?;

    // xcap returns an RGBA image; convert to BGRA for ffmpeg.
    let mut data = image.into_raw();
    // Swap R and B channels (RGBA → BGRA).
    for pixel in data.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    Ok(Some(Frame {
        data,
        width,
        height,
        timestamp_us,
    }))
}

/// Metadata describing a video stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    /// Display width in pixels.
    pub width: u32,
    /// Display height in pixels.
    pub height: u32,
    /// Target frame rate.
    pub fps: u32,
}
