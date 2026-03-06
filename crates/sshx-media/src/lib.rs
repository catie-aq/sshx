//! Shared media capture and encoding library for sshx screen sharing.
//!
//! Provides:
//! - [`FrameSource`] trait for pluggable screen capture backends
//! - [`Vp8Encoder`] for encoding raw frames to VP8 via ffmpeg subprocess
//! - [`Xvfb`] for managing a virtual X11 display lifecycle
//! - [`XInput`] for injecting mouse/keyboard events via XTest (Linux only)

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod capture;
pub mod encode;
pub mod input;
pub mod xvfb;

pub use capture::{Frame, FrameSource};
pub use encode::Vp8Encoder;
pub use input::XInput;
pub use xvfb::Xvfb;
