//! Capture loop: grabs frames from Xvfb, encodes to VP8, and streams to server.

use anyhow::{Context, Result};
use sshx_core::proto::{
    browser_update::BrowserMessage,
    BrowserCommand, BrowserUpdate, VideoFrame,
};
use sshx_media::{
    capture::XcapSource,
    encode::{skip_ivf_file_header, Vp8Encoder},
    input::{InputEvent, XInput},
    FrameSource,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{error, info, warn};

use crate::Options;

/// Run the main capture + streaming loop.
///
/// Captures frames from the Xvfb display, encodes them, and streams to server.
/// Simultaneously receives input commands from the server and injects them.
pub async fn run(vid: u32, display: &str, opts: &Options) -> Result<()> {
    let mut client = crate::grpc_client::connect(&opts.server).await?;

    // Channel for sending encoded frames from the capture thread to the async runtime.
    let (frame_tx, frame_rx) = mpsc::channel::<BrowserUpdate>(32);

    // Spawn the capture + encoding thread (blocking I/O).
    let display_clone = display.to_string();
    let width = opts.width;
    let height = opts.height;
    let fps = opts.fps;
    let bitrate = opts.bitrate_kbps;
    let frame_tx_clone = frame_tx.clone();
    std::thread::spawn(move || {
        if let Err(e) = capture_thread(&display_clone, width, height, fps, bitrate, vid, frame_tx_clone) {
            error!("capture thread exited: {e:?}");
        }
    });

    // Connect the frame stream to the gRPC bidirectional stream.
    let frame_stream = ReceiverStream::new(frame_rx);
    let response = client
        .stream(frame_stream)
        .await
        .context("BrowserService.Stream RPC failed")?;

    // Open XInput connection for input injection.
    let xinput = XInput::connect(display).ok();
    if xinput.is_none() {
        warn!("could not open XTest connection; input injection disabled");
    }

    // Process incoming commands from the server.
    let mut inbound = response.into_inner();
    loop {
        use tokio_stream::StreamExt;
        match inbound.next().await {
            None => {
                info!("server closed BrowserService.Stream");
                break;
            }
            Some(Err(e)) => {
                error!("stream error: {e}");
                break;
            }
            Some(Ok(BrowserCommand {
                browser_command: Some(cmd),
            })) => match cmd {
                sshx_core::proto::browser_command::BrowserCommand::Input(input_event) => {
                    if let Some(ref xi) = xinput {
                        match serde_json::from_str::<InputEvent>(&input_event.json) {
                            Ok(event) => {
                                if let Err(e) = xi.inject(&event) {
                                    warn!("input injection failed: {e}");
                                }
                            }
                            Err(e) => warn!("failed to parse input event: {e}"),
                        }
                    }
                }
                sshx_core::proto::browser_command::BrowserCommand::Ping(ts) => {
                    // Send pong.
                    let _ = frame_tx.send(BrowserUpdate {
                        browser_message: Some(BrowserMessage::Pong(ts)),
                    }).await;
                }
            },
            Some(Ok(BrowserCommand { browser_command: None })) => {}
        }
    }

    Ok(())
}

/// Blocking thread: captures frames, encodes to VP8, sends via channel.
fn capture_thread(
    display: &str,
    width: u32,
    height: u32,
    fps: u32,
    bitrate_kbps: u32,
    vid: u32,
    tx: mpsc::Sender<BrowserUpdate>,
) -> Result<()> {
    let mut source = XcapSource::new(display, width, height, fps)
        .context("failed to create XcapSource")?;

    let mut encoder = Vp8Encoder::new(width, height, fps, bitrate_kbps)
        .context("failed to create Vp8Encoder")?;

    // ffmpeg only flushes the IVF file header after receiving the first input
    // frame (pipe output is buffered). Capture and push the first frame first,
    // then skip the header, then read back the encoded packet(s) for that frame.
    let first_frame = match source.next_frame()? {
        Some(f) => f,
        None => return Ok(()),
    };
    encoder.push_frame(&first_frame).context("failed to push first frame to ffmpeg")?;

    // Drop stdin to flush — NOT here; we still need it. ffmpeg should now have
    // enough data to write the IVF file header to stdout.
    skip_ivf_file_header(&mut encoder).context("failed to skip IVF header")?;

    // Process the encoded output for the first frame before entering the loop.
    let first_timestamp = first_frame.timestamp_us;
    while let Some(packet) = encoder.read_packet()? {
        let keyframe = Vp8Encoder::is_keyframe(&packet);
        let update = BrowserUpdate {
            browser_message: Some(BrowserMessage::Frame(VideoFrame {
                vid,
                data: packet.into(),
                timestamp: first_timestamp,
                keyframe,
            })),
        };
        if tx.blocking_send(update).is_err() {
            return Ok(());
        }
    }

    loop {
        // Capture next frame.
        let frame = match source.next_frame()? {
            Some(f) => f,
            None => break,
        };

        let timestamp_us = frame.timestamp_us;

        // Push to encoder.
        encoder.push_frame(&frame)?;

        // Read encoded packet(s).
        while let Some(packet) = encoder.read_packet()? {
            let keyframe = Vp8Encoder::is_keyframe(&packet);
            let update = BrowserUpdate {
                browser_message: Some(BrowserMessage::Frame(VideoFrame {
                    vid,
                    data: packet.into(),
                    timestamp: timestamp_us,
                    keyframe,
                })),
            };
            // Non-blocking send; drop frames if the channel is full (backpressure).
            if tx.blocking_send(update).is_err() {
                info!("frame channel closed; stopping capture");
                return Ok(());
            }
        }
    }

    Ok(())
}
