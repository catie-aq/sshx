//! VP8 video encoding via ffmpeg subprocess.

use std::io::{BufRead, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;

use anyhow::{Context, Result};
use tracing::warn;

use crate::capture::Frame;

/// An encoded VP8 packet read from ffmpeg output.
pub struct EncodedPacket {
    /// Raw VP8 frame data (without IVF frame header).
    pub data: Vec<u8>,
    /// Whether this is a VP8 keyframe.
    pub keyframe: bool,
}

/// VP8 encoder that pipes raw frames to an `ffmpeg` subprocess.
///
/// Input format: raw BGRA frames at a fixed resolution.
/// Output format: VP8 packets delivered via an internal reader thread
/// to avoid pipe deadlocks.
pub struct Vp8Encoder {
    child: Child,
    /// Wrapped in Option so we can take it in Drop without unsafe.
    stdin: Option<std::process::ChildStdin>,
    /// Receives encoded packets from the reader thread.
    packet_rx: mpsc::Receiver<Vec<u8>>,
    width: u32,
    height: u32,
}

impl Vp8Encoder {
    /// Start an ffmpeg encoder process for the given resolution and frame rate.
    pub fn new(width: u32, height: u32, fps: u32, bitrate_kbps: u32) -> Result<Self> {
        let mut child = Command::new("ffmpeg")
            .args([
                "-f", "rawvideo",
                "-pixel_format", "bgra",
                "-video_size", &format!("{width}x{height}"),
                "-framerate", &fps.to_string(),
                "-i", "pipe:0",                          // read raw frames from stdin
                "-c:v", "libvpx",                        // VP8 codec
                "-b:v", &format!("{bitrate_kbps}k"),
                "-quality", "realtime",
                "-cpu-used", "5",
                "-auto-alt-ref", "0",                    // required for BGRA input (alpha channel)
                "-f", "ivf",                             // IVF container
                "pipe:1",                                // write to stdout
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to spawn ffmpeg; ensure it is installed and in PATH")?;

        let stdin = Some(child.stdin.take().expect("stdin configured"));
        let mut stdout = child.stdout.take().expect("stdout configured");

        // Log ffmpeg stderr in a background thread so errors are visible.
        let stderr = child.stderr.take().expect("stderr configured");
        std::thread::spawn(move || {
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines().map_while(|l| l.ok()) {
                // ffmpeg is very chatty; only log lines that look like errors.
                let lower = line.to_ascii_lowercase();
                if lower.contains("error") || lower.contains("invalid") || lower.contains("failed") {
                    warn!("ffmpeg: {line}");
                }
            }
        });

        // Spawn a reader thread that continuously reads IVF packets from stdout.
        // This prevents deadlocks: the main thread can push frames to stdin while
        // the reader thread independently consumes encoded output from stdout.
        let (packet_tx, packet_rx) = mpsc::sync_channel::<Vec<u8>>(64);
        std::thread::spawn(move || {
            use std::io::Read;

            // Read and discard the 32-byte IVF file header.
            let mut file_header = [0u8; 32];
            if stdout.read_exact(&mut file_header).is_err() {
                return;
            }

            // Continuously read IVF frame packets.
            loop {
                // IVF frame header: 12 bytes.
                //   bytes 0-3: frame size (little-endian u32)
                //   bytes 4-11: timestamp (little-endian u64)
                let mut header = [0u8; 12];
                match stdout.read_exact(&mut header) {
                    Ok(_) => {}
                    Err(_) => break, // EOF or error
                }

                let frame_size = u32::from_le_bytes(header[0..4].try_into().unwrap()) as usize;
                let mut frame_data = vec![0u8; frame_size];
                if stdout.read_exact(&mut frame_data).is_err() {
                    break;
                }

                if packet_tx.send(frame_data).is_err() {
                    break; // receiver dropped
                }
            }
        });

        Ok(Self { child, stdin, packet_rx, width, height })
    }

    /// Feed a raw frame to the encoder.
    pub fn push_frame(&mut self, frame: &Frame) -> Result<()> {
        debug_assert_eq!(frame.width, self.width);
        debug_assert_eq!(frame.height, self.height);
        if let Some(ref mut stdin) = self.stdin {
            stdin
                .write_all(&frame.data)
                .context("failed to write frame to ffmpeg stdin")?;
        }
        Ok(())
    }

    /// Try to receive the next encoded packet (non-blocking).
    ///
    /// Returns `None` if no packet is available yet.
    pub fn try_read_packet(&mut self) -> Option<EncodedPacket> {
        match self.packet_rx.try_recv() {
            Ok(data) => {
                let keyframe = Self::is_keyframe(&data);
                Some(EncodedPacket { data, keyframe })
            }
            Err(_) => None,
        }
    }

    /// Wait for the next encoded packet (blocking).
    ///
    /// Returns `None` if the reader thread has exited (ffmpeg closed).
    pub fn read_packet(&mut self) -> Option<EncodedPacket> {
        match self.packet_rx.recv() {
            Ok(data) => {
                let keyframe = Self::is_keyframe(&data);
                Some(EncodedPacket { data, keyframe })
            }
            Err(_) => None,
        }
    }

    /// Determine if the data is a VP8 keyframe.
    ///
    /// VP8 keyframes have their partition_start bit (bit 0 of byte 0) set to 0.
    pub fn is_keyframe(data: &[u8]) -> bool {
        data.first().map(|b| b & 0x01 == 0).unwrap_or(false)
    }
}

impl Drop for Vp8Encoder {
    fn drop(&mut self) {
        // Close stdin so ffmpeg knows to flush and exit.
        drop(self.stdin.take());
        if let Err(e) = self.child.wait() {
            warn!("ffmpeg exited with error: {e}");
        }
    }
}
