use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::{
    ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    Path, State,
};
use axum::response::IntoResponse;
use bytes::Bytes;
use futures_util::SinkExt;
use sshx_core::proto::{server_update::ServerMessage, ImageFile, NewShell, SetWidgetNameRequest, TerminalInput, TerminalSize};
use sshx_core::{Sid, Vid};
use subtle::ConstantTimeEq;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tracing::{error, info_span, warn, Instrument};

use crate::session::Session;
use crate::web::protocol::{WsClient, WsIceServer, WsServer, WsVideoStream, WsWidget, WsWidgetKind};
use crate::ServerState;

pub async fn get_session_ws(
    Path(name): Path<String>,
    ws: WebSocketUpgrade,
    State(state): State<Arc<ServerState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |mut socket| {
        let span = info_span!("ws", %name);
        async move {
            match state.frontend_connect(&name).await {
                Ok(Ok(session)) => {
                    if let Err(err) = handle_socket(&mut socket, session, state.ice_servers().to_vec()).await {
                        warn!(?err, "websocket exiting early");
                    } else {
                        socket.close().await.ok();
                    }
                }
                Ok(Err(Some(host))) => {
                    if let Err(err) = proxy_redirect(&mut socket, &host, &name).await {
                        error!(?err, "failed to proxy websocket");
                        let frame = CloseFrame {
                            code: 4500,
                            reason: format!("proxy redirect: {err}").into(),
                        };
                        socket.send(Message::Close(Some(frame))).await.ok();
                    } else {
                        socket.close().await.ok();
                    }
                }
                Ok(Err(None)) => {
                    let frame = CloseFrame {
                        code: 4404,
                        reason: "could not find the requested session".into(),
                    };
                    socket.send(Message::Close(Some(frame))).await.ok();
                }
                Err(err) => {
                    error!(?err, "failed to connect to frontend session");
                    let frame = CloseFrame {
                        code: 4500,
                        reason: format!("session connect: {err}").into(),
                    };
                    socket.send(Message::Close(Some(frame))).await.ok();
                }
            }
        }
        .instrument(span)
    })
}

/// Handle an incoming live WebSocket connection to a given session.
async fn handle_socket(socket: &mut WebSocket, session: Arc<Session>, ice_servers: Vec<WsIceServer>) -> Result<()> {
    /// Send a message to the client over WebSocket.
    async fn send(socket: &mut WebSocket, msg: WsServer) -> Result<()> {
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&msg, &mut buf)?;
        socket.send(Message::Binary(Bytes::from(buf))).await?;
        Ok(())
    }

    /// Receive a message from the client over WebSocket.
    async fn recv(socket: &mut WebSocket) -> Result<Option<WsClient>> {
        Ok(loop {
            match socket.recv().await.transpose()? {
                Some(Message::Text(_)) => warn!("ignoring text message over WebSocket"),
                Some(Message::Binary(msg)) => break Some(ciborium::de::from_reader(&*msg)?),
                Some(_) => (), // ignore other message types, keep looping
                None => break None,
            }
        })
    }

    let metadata = session.metadata();
    let user_id = session.counter().next_uid();
    session.sync_now();
    send(socket, WsServer::Hello(user_id, metadata.name.clone())).await?;
    send(socket, WsServer::IceServers(ice_servers)).await?;
    send(socket, WsServer::IdeAvailable(session.ide_available())).await?;

    let can_write = match recv(socket).await? {
        Some(WsClient::Authenticate(bytes, write_password_bytes)) => {
            // Constant-time comparison of bytes, converting Choice to bool
            if !bool::from(bytes.ct_eq(metadata.encrypted_zeros.as_ref())) {
                send(socket, WsServer::InvalidAuth()).await?;
                return Ok(());
            }

            match (write_password_bytes, &metadata.write_password_hash) {
                // No password needed, so all users can write (default).
                (_, None) => true,

                // Password stored but not provided, user is read-only.
                (None, Some(_)) => false,

                // Password stored and provided, compare them.
                (Some(provided), Some(stored)) => {
                    if !bool::from(provided.ct_eq(stored)) {
                        send(socket, WsServer::InvalidAuth()).await?;
                        return Ok(());
                    }
                    true
                }
            }
        }
        _ => {
            send(socket, WsServer::InvalidAuth()).await?;
            return Ok(());
        }
    };

    let _user_guard = session.user_scope(user_id, can_write)?;

    let update_tx = session.update_tx(); // start listening for updates before any state reads
    let mut broadcast_stream = session.subscribe_broadcast();
    send(socket, WsServer::Users(session.list_users())).await?;
    send(socket, WsServer::Notes(session.list_notes())).await?;
    send(socket, WsServer::TextBlocks(session.list_text_blocks())).await?;
    send(socket, WsServer::Drawings(session.list_drawings())).await?;
    send(socket, WsServer::Slides(session.list_slides())).await?;
    let (sf_root, sf_root_path, sf_files) = session.list_source_files();
    send(socket, WsServer::SourceFiles(sf_root, sf_root_path, sf_files)).await?;
    send(socket, WsServer::Widgets(session.list_widgets())).await?;
    send(socket, WsServer::ShellNames(session.list_shell_names())).await?;
    send(socket, WsServer::Shells(session.list_shells())).await?;
    // Replay stored Claude events so reconnecting browsers see recent history.
    for event in session.list_claude_events() {
        send(socket, WsServer::ClaudeEvent(event)).await?;
    }

    // Send video stream snapshot on connect.
    send(socket, WsServer::VideoStreams(session.list_video_streams())).await?;
    // Send browser controller status for all browser streams.
    for (vid, stream) in session.list_video_streams() {
        if stream.is_browser {
            let ctrl = session.get_browser_controller(vid).flatten();
            send(socket, WsServer::BrowserControlStatus(vid, ctrl)).await?;
        }
    }

    // Send IDE state snapshot and edit lock on connect.
    send(socket, WsServer::IdeStates(session.list_ide_states())).await?;
    send(socket, WsServer::EditLock(session.get_edit_lock())).await?;

    let mut subscribed = HashSet::new(); // prevent duplicate subscriptions
    let (chunks_tx, mut chunks_rx) = mpsc::channel::<(Sid, u64, Vec<Bytes>)>(1);
    let (browser_frame_tx, mut browser_frame_rx) = mpsc::channel::<(Vid, u64, Bytes, bool)>(256);

    let mut shells_stream = session.subscribe_shells();
    let mut notes_stream = session.subscribe_notes();
    let mut text_blocks_stream = session.subscribe_text_blocks();
    let mut drawings_stream = session.subscribe_drawings();
    let mut source_files_stream = session.subscribe_source_files();
    let mut widget_stream = session.subscribe_widgets();
    let mut video_streams_stream = session.subscribe_video_streams();
    loop {
        let msg = tokio::select! {
            _ = session.terminated() => break,
            Some(result) = broadcast_stream.next() => {
                let msg = match result {
                    Ok(msg) => msg,
                    Err(e) => {
                        tracing::warn!("broadcast recv error: {e}");
                        continue;
                    }
                };
                if let Err(e) = send(socket, msg).await {
                    tracing::warn!("broadcast send failed: {e}");
                    continue;
                }
                continue;
            }
            Some(shells) = shells_stream.next() => {
                send(socket, WsServer::Shells(shells)).await?;
                continue;
            }
            Some(notes) = notes_stream.next() => {
                send(socket, WsServer::Notes(notes)).await?;
                continue;
            }
            Some(text_blocks) = text_blocks_stream.next() => {
                send(socket, WsServer::TextBlocks(text_blocks)).await?;
                continue;
            }
            Some(drawings) = drawings_stream.next() => {
                send(socket, WsServer::Drawings(drawings)).await?;
                continue;
            }
            Some((root, root_path, files)) = source_files_stream.next() => {
                send(socket, WsServer::SourceFiles(root, root_path, files)).await?;
                continue;
            }
            Some(widgets) = widget_stream.next() => {
                send(socket, WsServer::Widgets(widgets)).await?;
                continue;
            }
            Some(video_streams) = video_streams_stream.next() => {
                send(socket, WsServer::VideoStreams(video_streams)).await?;
                continue;
            }
            Some((id, seqnum, chunks)) = chunks_rx.recv() => {
                send(socket, WsServer::Chunks(id, seqnum, chunks)).await?;
                continue;
            }
            Some((vid, timestamp, data, keyframe)) = browser_frame_rx.recv() => {
                send(socket, WsServer::BrowserFrame(vid, timestamp, data, keyframe)).await?;
                continue;
            }
            result = recv(socket) => {
                match result? {
                    Some(msg) => msg,
                    None => break,
                }
            }
        };

        match msg {
            WsClient::Authenticate(_, _) => {}
            WsClient::SetName(name) => {
                if !name.is_empty() {
                    session.update_user(user_id, |user| user.name = name)?;
                }
            }
            WsClient::SetCursor(cursor) => {
                session.update_user(user_id, |user| user.cursor = cursor)?;
            }
            WsClient::SetFocus(id) => {
                session.update_user(user_id, |user| user.focus = id)?;
            }
            WsClient::Create(x, y) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                let id = session.counter().next_sid();
                session.sync_now();
                let new_shell = NewShell { id: id.0, x, y };
                update_tx
                    .send(ServerMessage::CreateShell(new_shell))
                    .await?;
            }
            WsClient::Close(id) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                update_tx.send(ServerMessage::CloseShell(id.0)).await?;
            }
            WsClient::Move(id, winsize) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                if let Err(err) = session.move_shell(id, winsize) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                    continue;
                }
                if let Some(winsize) = winsize {
                    let msg = ServerMessage::Resize(TerminalSize {
                        id: id.0,
                        rows: winsize.rows as u32,
                        cols: winsize.cols as u32,
                    });
                    session.update_tx().send(msg).await?;
                }
            }
            WsClient::Data(id, data, offset) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                let input = TerminalInput {
                    id: id.0,
                    data,
                    offset,
                };
                update_tx.send(ServerMessage::Input(input)).await?;
            }
            WsClient::Subscribe(id, chunknum) => {
                if subscribed.contains(&id) {
                    continue;
                }
                subscribed.insert(id);
                let session = Arc::clone(&session);
                let chunks_tx = chunks_tx.clone();
                tokio::spawn(async move {
                    let stream = session.subscribe_chunks(id, chunknum);
                    tokio::pin!(stream);
                    while let Some((seqnum, chunks)) = stream.next().await {
                        if chunks_tx.send((id, seqnum, chunks)).await.is_err() {
                            break;
                        }
                    }
                });
            }
            WsClient::Chat(msg) => {
                session.send_chat(user_id, &msg)?;
            }
            WsClient::Ping(ts) => {
                send(socket, WsServer::Pong(ts)).await?;
            }
            WsClient::CreateNote(x, y) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                let id = session.counter().next_nid();
                if let Err(err) = session.add_note(id, x, y) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::UpdateNote(id, note) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                if let Err(err) = session.update_note(id, note) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::DeleteNote(id) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                if let Err(err) = session.delete_note(id) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::OpenFileTree(x, y, root) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                let id = session.counter().next_wid();
                let widget = WsWidget { x, y, w: 250, h: 400, kind: WsWidgetKind::FileTree { root }, collapsed: false, name: None };
                if let Err(err) = session.add_widget(id, widget) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::OpenFileCard(x, y, path) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                let id = session.counter().next_wid();
                let widget = WsWidget { x, y, w: 320, h: 400, kind: WsWidgetKind::FileCard { path }, collapsed: false, name: None };
                if let Err(err) = session.add_widget(id, widget) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::MoveWidget(id, x, y) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                if let Err(err) = session.move_widget(id, x, y) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::ResizeWidget(id, w, h) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                if let Err(err) = session.resize_widget(id, w, h) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::CloseWidget(id) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                if let Err(err) = session.remove_widget(id) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::SetWidgetCollapsed(id, collapsed) => {
                if let Err(err) = session.set_widget_collapsed(id, collapsed) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::SetShellName(id, name) => {
                session.set_shell_name(id, name);
            }
            WsClient::SetWidgetName(id, name) => {
                // Get the instanceId before mutating (needed for CLI persistence).
                let instance_id = session.get_widget_instance_id(id);
                if let Err(err) = session.set_widget_name(id, name.clone()) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                } else if let Some(iid) = instance_id {
                    // Notify CLI to persist the name.
                    session.update_tx().try_send(
                        ServerMessage::SetWidgetName(SetWidgetNameRequest {
                            instance_id: iid,
                            name,
                        })
                    ).ok();
                }
            }
            WsClient::DescribeFiles(paths) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                session.request_describe_files(paths);
            }
            WsClient::UpdateFileMetadata(path, update) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                let proto_msg = session.update_source_file_metadata(&path, &update);
                session
                    .update_tx()
                    .send(ServerMessage::UpdateFileMetadata(proto_msg))
                    .await?;
                // If an image name was provided, push the image bytes to the CLI.
                if let Some(ref name) = update.image_name {
                    if !name.is_empty() {
                        if let Some(img_path) = session.get_source_file_image_path(&path) {
                            // img_path is like "/uploads/{session}/{filename}" — strip leading /
                            let local = img_path.trim_start_matches('/');
                            match tokio::fs::read(local).await {
                                Ok(data) => {
                                    session.update_tx()
                                        .send(ServerMessage::ImageFile(ImageFile {
                                            name: name.clone(),
                                            data: data.into(),
                                        }))
                                        .await
                                        .ok();
                                }
                                Err(e) => warn!("image push: could not read {local}: {e}"),
                            }
                        }
                    }
                }
            }
            WsClient::OpenClaudeFeed(x, y, instance_id) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                // Deduplicate: silently ignore if a feed for this instance already exists.
                if session.claude_feed_exists(&instance_id) {
                    continue;
                }
                let id = session.counter().next_wid();
                let widget = WsWidget {
                    x,
                    y,
                    w: 320,
                    h: 400,
                    kind: WsWidgetKind::ClaudeFeed { instance_id },
                    collapsed: false,
                    name: None,
                };
                if let Err(err) = session.add_widget(id, widget) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::StartScreenShare => {
                let vid = session.counter().next_vid();
                let users = session.list_users();
                let label = users
                    .iter()
                    .find(|(uid, _)| *uid == user_id)
                    .map(|(_, u)| format!("{}'s screen", u.name))
                    .unwrap_or_else(|| "Screen Share".to_string());
                let stream = WsVideoStream {
                    owner_uid: Some(user_id),
                    label,
                    is_browser: false,
                    x: 100,
                    y: 100,
                    w: 640,
                    h: 400,
                };
                session.add_video_stream(vid, stream);
                // Tell the sharer their assigned vid so they can start the RTCPeerConnection.
                send(socket, WsServer::VideoStreamDiff(vid, session.list_video_streams().into_iter().find(|(v, _)| *v == vid).map(|(_, s)| s))).await?;
            }
            WsClient::StopScreenShare => {
                // Remove all video streams owned by this user.
                let owned: Vec<Vid> = session
                    .list_video_streams()
                    .into_iter()
                    .filter(|(_, s)| s.owner_uid == Some(user_id) && !s.is_browser)
                    .map(|(v, _)| v)
                    .collect();
                for vid in owned {
                    session.remove_video_stream(vid);
                }
            }
            WsClient::CloseStream(vid) => {
                // Any user can close any video stream for everyone.
                session.remove_video_stream(vid);
            }
            WsClient::WatchStream(vid) => {
                let stream_info = session.list_video_streams().into_iter().find(|(v, _)| *v == vid);
                if let Some((_, info)) = stream_info {
                    if info.is_browser {
                        // Browser stream: subscribe to VP8 frames and relay them over WebSocket.
                        let (backlog, mut frame_sub) = session.subscribe_browser_frames(vid);
                        let ftx = browser_frame_tx.clone();
                        // Send GOP backlog so the decoder can start immediately.
                        for frame in backlog {
                            ftx.send((vid, frame.timestamp, Bytes::from(frame.data), frame.keyframe)).await.ok();
                        }
                        tokio::spawn(async move {
                            while let Some(frame) = frame_sub.recv().await {
                                if ftx.send((vid, frame.timestamp, Bytes::from(frame.data), frame.keyframe)).await.is_err() {
                                    break;
                                }
                            }
                        });
                    } else {
                        // P2P screen share: notify the stream owner via WebRTC signaling.
                        if let Some(owner) = info.owner_uid {
                            session.relay_rtc_offer(vid, owner, format!("watch:{}", user_id.0));
                        }
                    }
                }
            }
            WsClient::UnwatchStream(_vid) => {
                // Nothing to do server-side for P2P streams; cleanup is handled by WebRTC.
            }
            WsClient::SendRtcOffer(vid, target_uid, sdp) => {
                session.relay_rtc_offer(vid, target_uid, sdp);
            }
            WsClient::SendRtcAnswer(vid, target_uid, sdp) => {
                session.relay_rtc_answer(vid, target_uid, user_id, sdp);
            }
            WsClient::SendRtcIce(vid, target_uid, candidate) => {
                session.relay_rtc_ice(vid, target_uid, user_id, candidate);
            }
            WsClient::MoveVideoStream(vid, x, y) => {
                session.move_video_stream(vid, x, y);
            }
            WsClient::ResizeVideoStream(vid, w, h) => {
                session.resize_video_stream(vid, w, h);
            }
            WsClient::BrowserInput(vid, event_json) => {
                // Forward to the sshx-browser gRPC stream only if this user is the controller.
                if let Some(Some(ctrl)) = session.get_browser_controller(vid) {
                    if ctrl == user_id {
                        session.send_browser_input(vid, event_json);
                    }
                }
            }
            WsClient::RequestBrowserControl(vid) => {
                // Grant control if no one currently has it.
                let current = session.get_browser_controller(vid);
                if let Some(None) = current {
                    session.set_browser_controller(vid, Some(user_id));
                } else if current.is_none() {
                    send(socket, WsServer::Error("video stream not found".into())).await?;
                }
            }
            WsClient::ReleaseBrowserControl(vid) => {
                // Release only if this user is the current controller.
                if let Some(Some(ctrl)) = session.get_browser_controller(vid) {
                    if ctrl == user_id {
                        session.set_browser_controller(vid, None);
                    }
                }
            }
            WsClient::HighlightComponent(name) => {
                session.broadcast_highlight(name);
            }
            WsClient::OpenAppOverlay(x, y) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                let id = session.counter().next_wid();
                let widget = WsWidget {
                    x,
                    y,
                    w: 320,
                    h: 280,
                    kind: WsWidgetKind::AppOverlay {
                        url: String::new(),
                        allow_open_file: true,
                        allow_open_claude: true,
                    },
                    collapsed: false,
                    name: None,
                };
                if let Err(err) = session.add_widget(id, widget) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::UpdateAppOverlay(wid, url, allow_open_file, allow_open_claude) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                let kind = WsWidgetKind::AppOverlay { url, allow_open_file, allow_open_claude };
                if let Err(err) = session.update_widget_kind(wid, kind) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::CreateTextBlock(x, y) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                let id = session.counter().next_tid();
                if let Err(err) = session.add_text_block(id, x, y) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::UpdateTextBlock(id, block) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                if let Err(err) = session.update_text_block(id, block) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::DeleteTextBlock(id) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                if let Err(err) = session.delete_text_block(id) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::CreateDrawing(drawing) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                let id = session.counter().next_did();
                if let Err(err) = session.add_drawing(id, drawing) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::DeleteDrawing(id) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                if let Err(err) = session.delete_drawing(id) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::CreateSlide(slide) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                let id = session.counter().next_slid();
                if let Err(err) = session.add_slide(id, slide) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::UpdateSlide(id, slide) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                if let Err(err) = session.update_slide(id, slide) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::DeleteSlide(id) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                if let Err(err) = session.delete_slide(id) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::ReorderSlides(orders) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                if let Err(err) = session.reorder_slides(orders) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::OpenIdeEditor(x, y, workspace_label) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                if !session.ide_available() {
                    send(socket, WsServer::Error("IDE not available".into())).await?;
                    continue;
                }
                let id = session.counter().next_wid();
                let widget = WsWidget {
                    x,
                    y,
                    w: 900,
                    h: 600,
                    kind: WsWidgetKind::IdeEditor { workspace_label, ide_id: id.0 },
                    collapsed: false,
                    name: None,
                };
                if let Err(err) = session.add_widget(id, widget) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                }
            }
            WsClient::UpdateIdeState(wid, state) => {
                session.update_ide_state(wid, state);
            }
            WsClient::RequestEditLock(file) => {
                match session.request_edit_lock(user_id, file) {
                    Ok(_) => {}
                    Err(e) => {
                        send(socket, WsServer::Error(e.to_string())).await?;
                    }
                }
            }
            WsClient::ReleaseEditLock => {
                session.release_edit_lock(user_id);
            }
            WsClient::CreateImageWidget(x, y, url, alt, auto_name) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                let id = session.counter().next_wid();
                let widget = WsWidget {
                    x,
                    y,
                    w: 400,
                    h: 300,
                    kind: WsWidgetKind::Image { url: url.clone(), alt },
                    collapsed: false,
                    name: auto_name.clone().filter(|n| !n.is_empty()),
                };
                if let Err(err) = session.add_widget(id, widget) {
                    send(socket, WsServer::Error(err.to_string())).await?;
                } else if let Some(name) = auto_name.filter(|n| !n.is_empty()) {
                    // Auto-push image bytes to CLI (use .send().await to avoid silent drops).
                    let local = url.trim_start_matches('/');
                    match tokio::fs::read(local).await {
                        Ok(data) => {
                            session.update_tx()
                                .send(ServerMessage::ImageFile(ImageFile {
                                    name,
                                    data: data.into(),
                                }))
                                .await
                                .ok();
                        }
                        Err(e) => warn!("image push: could not read {local}: {e}"),
                    }
                }
            }
            WsClient::RequestContextSnapshot => {
                session.request_context_snapshot();
            }
            WsClient::PushImageWidget(wid, new_name) => {
                if let Err(e) = session.check_write_permission(user_id) {
                    send(socket, WsServer::Error(e.to_string())).await?;
                    continue;
                }
                if new_name.is_empty() { continue; }
                // Look up the widget's image URL and current name.
                let info = session.list_widgets().into_iter()
                    .find(|(id, _)| *id == wid)
                    .and_then(|(_, w)| match w.kind {
                        WsWidgetKind::Image { url, .. } => Some((url, w.name)),
                        _ => None,
                    });
                if let Some((url, _old_name)) = info {
                    // Rename the file on disk if the name changed.
                    let local = url.trim_start_matches('/');
                    let new_url = rename_upload_file(local, &new_name).await.unwrap_or_else(|| url.clone());
                    // Update the widget kind with the new URL and name.
                    session.update_image_widget(wid, new_url, new_name.clone()).ok();
                    // Push image bytes to CLI (use .send().await to avoid silent drops).
                    let new_local = session.list_widgets().into_iter()
                        .find(|(id, _)| *id == wid)
                        .and_then(|(_, w)| match w.kind {
                            WsWidgetKind::Image { url, .. } => Some(url),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let read_path = new_local.trim_start_matches('/');
                    match tokio::fs::read(read_path).await {
                        Ok(data) => {
                            session.update_tx()
                                .send(ServerMessage::ImageFile(ImageFile {
                                    name: new_name,
                                    data: data.into(),
                                }))
                                .await
                                .ok();
                        }
                        Err(e) => warn!("image push: could not read {read_path}: {e}"),
                    }
                }
            }
        }
    }
    Ok(())
}

/// Rename an uploaded file on disk to match a new user-chosen name.
/// Returns the new URL path (e.g. `/uploads/session/newname.png`) or None on failure.
async fn rename_upload_file(current_path: &str, new_name: &str) -> Option<String> {
    use rand::Rng;

    let path = std::path::Path::new(current_path);
    if !path.exists() {
        return None;
    }
    let parent = path.parent()?;
    // Sanitize: keep alphanumeric, dot, dash, underscore.
    let sanitized: String = new_name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
        .collect();
    if sanitized.is_empty() {
        return None;
    }
    let new_path = parent.join(&sanitized);
    // Don't rename if it's the same path.
    if new_path == path {
        return None;
    }
    let final_path = if new_path.exists() {
        // Add a short random prefix to avoid collisions.
        let prefix: u32 = rand::thread_rng().gen();
        parent.join(format!("{:08x}_{sanitized}", prefix))
    } else {
        new_path
    };
    if let Err(e) = tokio::fs::rename(path, &final_path).await {
        warn!("rename upload: {e}");
        return None;
    }
    Some(format!("/{}", final_path.display()))
}

/// Transparently reverse-proxy a WebSocket connection to a different host.
async fn proxy_redirect(socket: &mut WebSocket, host: &str, name: &str) -> Result<()> {
    use tokio_tungstenite::{
        connect_async,
        tungstenite::protocol::{CloseFrame as TCloseFrame, Message as TMessage},
    };

    let (mut upstream, _) = connect_async(format!("ws://{host}/api/s/{name}")).await?;
    loop {
        // Due to axum having its own WebSocket API types, we need to manually translate
        // between it and tungstenite's message type.
        tokio::select! {
            Some(client_msg) = socket.recv() => {
                let msg = match client_msg {
                    Ok(Message::Text(s)) => Some(TMessage::Text(s.as_str().into())),
                    Ok(Message::Binary(b)) => Some(TMessage::Binary(b)),
                    Ok(Message::Close(frame)) => {
                        let frame = frame.map(|frame| TCloseFrame {
                            code: frame.code.into(),
                            reason: frame.reason.as_str().into(),
                        });
                        Some(TMessage::Close(frame))
                    }
                    Ok(_) => None,
                    Err(_) => break,
                };
                if let Some(msg) = msg {
                    if upstream.send(msg).await.is_err() {
                        break;
                    }
                }
            }
            Some(server_msg) = upstream.next() => {
                let msg = match server_msg {
                    Ok(TMessage::Text(s)) => Some(Message::Text(s.as_str().into())),
                    Ok(TMessage::Binary(b)) => Some(Message::Binary(b)),
                    Ok(TMessage::Close(frame)) => {
                        let frame = frame.map(|frame| CloseFrame {
                            code: frame.code.into(),
                            reason: frame.reason.as_str().into(),
                        });
                        Some(Message::Close(frame))
                    }
                    Ok(_) => None,
                    Err(_) => break,
                };
                if let Some(msg) = msg {
                    if socket.send(msg).await.is_err() {
                        break;
                    }
                }
            }
            else => break,
        }
    }

    Ok(())
}
