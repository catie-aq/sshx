//! X11 input injection via the XTest extension.
//!
//! Used by sshx-browser to forward mouse/keyboard events from browser clients
//! into the virtual Xvfb display that Chromium is running on.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
use x11rb::connection::Connection as _;

/// A mouse or keyboard event forwarded from a browser client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum InputEvent {
    /// Mouse movement event.
    MouseMove {
        /// X coordinate (0 = left edge of display).
        x: i32,
        /// Y coordinate (0 = top edge of display).
        y: i32,
    },
    /// Mouse button press or release.
    MouseButton {
        /// Button number: 1=left, 2=middle, 3=right, 4=scroll-up, 5=scroll-down.
        button: u32,
        /// `true` = press, `false` = release.
        pressed: bool,
    },
    /// Keyboard key press or release.
    Key {
        /// X11 keysym value (e.g. 0x0061 = 'a').
        keysym: u32,
        /// `true` = press, `false` = release.
        pressed: bool,
    },
    /// Mouse wheel scroll event.
    Scroll {
        /// Horizontal scroll delta (positive = right).
        delta_x: f32,
        /// Vertical scroll delta (positive = down).
        delta_y: f32,
    },
}

/// X11 input injector using the XTest extension.
///
/// Opens a connection to the specified X display and injects events via XTest.
#[cfg(target_os = "linux")]
pub struct XInput {
    conn: x11rb::rust_connection::RustConnection,
    screen: usize,
}

#[cfg(target_os = "linux")]
impl XInput {
    /// Connect to the given X display (e.g. ":99").
    pub fn connect(display: &str) -> Result<Self> {
        let (conn, screen) = x11rb::rust_connection::RustConnection::connect(Some(display))
            .context("failed to connect to X display for input injection")?;
        Ok(Self { conn, screen })
    }

    /// Inject an [`InputEvent`] into the X display.
    pub fn inject(&self, event: &InputEvent) -> Result<()> {
        use x11rb::protocol::xtest::ConnectionExt;

        match event {
            InputEvent::MouseMove { x, y } => {
                self.conn
                    .xtest_fake_input(
                        x11rb::protocol::xproto::MOTION_NOTIFY_EVENT,
                        0,           // detail: 0 = absolute move
                        x11rb::CURRENT_TIME,
                        x11rb::protocol::xproto::Window::from(0u32),
                        *x as i16,
                        *y as i16,
                        0,
                    )
                    .context("xtest_fake_input MouseMove")?;
            }
            InputEvent::MouseButton { button, pressed } => {
                let type_ = if *pressed {
                    x11rb::protocol::xproto::BUTTON_PRESS_EVENT
                } else {
                    x11rb::protocol::xproto::BUTTON_RELEASE_EVENT
                };
                self.conn
                    .xtest_fake_input(
                        type_,
                        *button as u8,
                        x11rb::CURRENT_TIME,
                        x11rb::protocol::xproto::Window::from(0u32),
                        0, 0, 0,
                    )
                    .context("xtest_fake_input MouseButton")?;
            }
            InputEvent::Key { keysym, pressed } => {
                let keycode = self.keysym_to_keycode(*keysym)?;
                let type_ = if *pressed {
                    x11rb::protocol::xproto::KEY_PRESS_EVENT
                } else {
                    x11rb::protocol::xproto::KEY_RELEASE_EVENT
                };
                self.conn
                    .xtest_fake_input(
                        type_,
                        keycode,
                        x11rb::CURRENT_TIME,
                        x11rb::protocol::xproto::Window::from(0u32),
                        0, 0, 0,
                    )
                    .context("xtest_fake_input Key")?;
            }
            InputEvent::Scroll { delta_y, .. } => {
                // Simulate scroll as repeated button 4 (up) or 5 (down) presses.
                let (button, count) = if *delta_y < 0.0 {
                    (4u32, (-delta_y / 3.0).ceil() as u32)
                } else {
                    (5u32, (delta_y / 3.0).ceil() as u32)
                };
                for _ in 0..count.min(10) {
                    self.conn.xtest_fake_input(
                        x11rb::protocol::xproto::BUTTON_PRESS_EVENT,
                        button as u8,
                        x11rb::CURRENT_TIME,
                        x11rb::protocol::xproto::Window::from(0u32),
                        0, 0, 0,
                    ).context("xtest scroll press")?;
                    self.conn.xtest_fake_input(
                        x11rb::protocol::xproto::BUTTON_RELEASE_EVENT,
                        button as u8,
                        x11rb::CURRENT_TIME,
                        x11rb::protocol::xproto::Window::from(0u32),
                        0, 0, 0,
                    ).context("xtest scroll release")?;
                }
            }
        }
        self.conn.flush().context("flush XTest events")?;
        Ok(())
    }

    /// Look up the X11 keycode for a keysym using the keyboard mapping.
    fn keysym_to_keycode(&self, keysym: u32) -> Result<u8> {
        use x11rb::protocol::xproto::ConnectionExt;
        use anyhow::bail;

        let setup = self.conn.setup();
        let min_keycode = setup.min_keycode;
        let max_keycode = setup.max_keycode;
        let count = (max_keycode - min_keycode + 1) as u8;

        let mapping = self.conn
            .get_keyboard_mapping(min_keycode, count)
            .context("GetKeyboardMapping")?
            .reply()
            .context("GetKeyboardMapping reply")?;

        let syms_per_keycode = mapping.keysyms_per_keycode as usize;
        for (i, chunk) in mapping.keysyms.chunks(syms_per_keycode).enumerate() {
            if chunk.contains(&keysym) {
                return Ok(min_keycode + i as u8);
            }
        }
        bail!("no keycode found for keysym 0x{keysym:04x}");
    }
}

/// Stub for non-Linux platforms.
#[cfg(not(target_os = "linux"))]
pub struct XInput;

#[cfg(not(target_os = "linux"))]
impl XInput {
    /// Not supported on non-Linux platforms.
    pub fn connect(_display: &str) -> Result<Self> {
        anyhow::bail!("XInput is only supported on Linux")
    }

    /// Not supported on non-Linux platforms.
    pub fn inject(&self, _event: &InputEvent) -> Result<()> {
        anyhow::bail!("XInput is only supported on Linux")
    }
}
