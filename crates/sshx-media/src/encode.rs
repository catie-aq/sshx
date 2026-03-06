//! VP8 video encoding via ffmpeg subprocess.

use std::io::{BufRead, Write};
use std::process::{Child, ChildStdout, Command, Stdio};

use anyhow::{Context, Result};
use tracing::warn;

use crate::capture::Frame;

/// VP8 encoder that pipes raw frames to an `ffmpeg` subprocess.
///
/// Input format: raw BGRA frames at a fixed resolution.
/// Output format: IVF-wrapped VP8, read back from ffmpeg's stdout.
pub struct Vp8Encoder {
    child: Child,
    /// Wrapped in Option so we can take it in Drop without unsafe.
    stdin: Option<std::process::ChildStdin>,
    stdout: ChildStdout,
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
        let stdout = child.stdout.take().expect("stdout configured");

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

        Ok(Self { child, stdin, stdout, width, height })
    }

    /// Feed a raw frame to the encoder.
    ///
    /// Returns `Ok(())` if the frame was written; the encoded output is read
    /// via [`read_packet`](Self::read_packet).
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

    /// Read the next encoded packet from ffmpeg's stdout.
    ///
    /// Returns `None` if ffmpeg has closed its output (e.g. on error or end of input).
    /// The returned bytes are a single IVF frame (header + VP8 data).
    pub fn read_packet(&mut self) -> Result<Option<Vec<u8>>> {
        use std::io::Read;

        // IVF frame header: 12 bytes.
        //   bytes 0-3: frame size (little-endian u32)
        //   bytes 4-11: timestamp (little-endian u64)
        let mut header = [0u8; 12];
        match self.stdout.read_exact(&mut header) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        }

        let frame_size = u32::from_le_bytes(header[0..4].try_into().unwrap()) as usize;
        let mut frame_data = vec![0u8; frame_size];
        self.stdout
            .read_exact(&mut frame_data)
            .context("failed to read VP8 frame from ffmpeg")?;

        Ok(Some(frame_data))
    }

    /// Determine if the next encoded frame is a VP8 keyframe.
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

/// Read the IVF file header (32 bytes) from stdout to synchronize the stream.
///
/// Must be called once after creating a [`Vp8Encoder`] before reading frames.
pub fn skip_ivf_file_header(encoder: &mut Vp8Encoder) -> Result<()> {
    use std::io::Read;
    let mut header = [0u8; 32];
    encoder
        .stdout
        .read_exact(&mut header)
        .context("failed to read IVF file header from ffmpeg")?;
    Ok(())
}
