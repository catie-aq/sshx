use std::collections::HashMap;

use anyhow::Result;
use sshx::encrypt::Encrypt;
use sshx_core::proto::*;
use sshx_server::{
    session::snapshot::JsonSessionSnapshot,
    web::protocol::{
        WsClient, WsDrawing, WsNote, WsSlide, WsTextBlock, WsWidget, WsWidgetKind,
    },
};

use crate::common::*;

pub mod common;

/// Test that a session with notes, text blocks, drawings, slides, and widgets
/// can be serialized to a JSON snapshot and restored on a new session.
#[tokio::test]
async fn test_json_snapshot_roundtrip() -> Result<()> {
    let server = TestServer::new().await;
    let encrypt = Encrypt::new("testkey1234567");

    // Create a session via gRPC.
    let mut grpc = server.grpc_client().await;
    let resp = grpc
        .open(OpenRequest {
            origin: server.endpoint(),
            encrypted_zeros: encrypt.zeros().into(),
            name: "persistence-test".into(),
            write_password_hash: None,
            restore_snapshot: None,
        })
        .await?
        .into_inner();

    let name = resp.name.clone();
    let session = server.state().lookup(&name).unwrap();

    // Add a sticky note.
    let nid = session.counter().next_nid();
    session.add_note(nid, 100, 200)?;
    session.update_note(
        nid,
        WsNote {
            x: 100,
            y: 200,
            text: "<p>Hello from a note!</p>".into(),
            color: "pink".into(),
            pinned: false,
            w: 300,
            h: 250,
            font: "caveat".into(),
        },
    )?;

    // Add a text block.
    let tid = session.counter().next_tid();
    session.add_text_block(tid, 400, 500)?;
    session.update_text_block(
        tid,
        WsTextBlock {
            x: 400,
            y: 500,
            content: "<p>Rich text block content</p>".into(),
            font_size: "lg".into(),
            color: "#ff0000".into(),
            align: "center".into(),
            font: "merriweather".into(),
        },
    )?;

    // Add a drawing.
    let did = session.counter().next_did();
    session.add_drawing(
        did,
        WsDrawing {
            tool: "pencil".into(),
            points: vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0],
            color: "#000000".into(),
            width: 2.0,
            opacity: 1.0,
        },
    )?;

    // Add a slide.
    let slid = session.counter().next_slid();
    session.add_slide(
        slid,
        WsSlide {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
            order: 1,
            label: "Intro Slide".into(),
        },
    )?;

    // Add a widget (image).
    let wid = session.counter().next_wid();
    session.add_widget(
        wid,
        WsWidget {
            x: 700,
            y: 300,
            w: 400,
            h: 300,
            kind: WsWidgetKind::Image {
                url: "/uploads/test.png".into(),
                alt: "test image".into(),
            },
            collapsed: false,
            name: Some("My Image".into()),
        },
    )?;

    // Set a shell name.
    let sid = session.counter().next_sid();
    session.add_shell(sid, (50, 60))?;
    session.set_shell_name(sid, "main-terminal".into());

    // Generate JSON snapshot.
    let snap = session.json_snapshot();

    // Verify snapshot content.
    assert!(snap.notes.contains_key(&nid.0));
    assert_eq!(snap.notes[&nid.0].text, "<p>Hello from a note!</p>");
    assert_eq!(snap.notes[&nid.0].color, "pink");
    assert_eq!(snap.notes[&nid.0].font, "caveat");

    assert!(snap.text_blocks.contains_key(&tid.0));
    assert_eq!(
        snap.text_blocks[&tid.0].content,
        "<p>Rich text block content</p>"
    );
    assert_eq!(snap.text_blocks[&tid.0].font, "merriweather");

    assert!(snap.drawings.contains_key(&did.0));
    assert_eq!(snap.drawings[&did.0].tool, "pencil");
    assert_eq!(snap.drawings[&did.0].points.len(), 6);

    assert!(snap.slides.contains_key(&slid.0));
    assert_eq!(snap.slides[&slid.0].label, "Intro Slide");
    assert_eq!(snap.slides[&slid.0].w, 1920);

    assert!(snap.widgets.contains_key(&wid.0));
    assert!(snap.shell_names.contains_key(&sid.0));
    assert_eq!(snap.shell_names[&sid.0], "main-terminal");

    // Serialize to JSON and parse back.
    let json_bytes = serde_json::to_vec(&snap)?;
    let restored_snap: JsonSessionSnapshot = serde_json::from_slice(&json_bytes)?;
    assert_eq!(restored_snap.notes.len(), snap.notes.len());
    assert_eq!(restored_snap.text_blocks.len(), snap.text_blocks.len());
    assert_eq!(restored_snap.drawings.len(), snap.drawings.len());
    assert_eq!(restored_snap.slides.len(), snap.slides.len());
    assert_eq!(restored_snap.widgets.len(), snap.widgets.len());
    assert_eq!(restored_snap.shell_names.len(), snap.shell_names.len());

    Ok(())
}

/// Test that a JSON snapshot sent as restore_snapshot in OpenRequest
/// correctly rebuilds session state visible to WebSocket clients.
#[tokio::test]
async fn test_restore_from_json_snapshot() -> Result<()> {
    let server = TestServer::new().await;
    let encrypt = Encrypt::new("restorekey12345");

    // Create a snapshot manually (simulating what the CLI would load).
    let mut notes = HashMap::new();
    notes.insert(
        1,
        WsNote {
            x: 50,
            y: 60,
            text: "<p>Restored note</p>".into(),
            color: "blue".into(),
            pinned: true,
            w: 280,
            h: 200,
            font: "inter".into(),
        },
    );

    let mut text_blocks = HashMap::new();
    text_blocks.insert(
        1,
        WsTextBlock {
            x: 300,
            y: 400,
            content: "<p>Restored text</p>".into(),
            font_size: "md".into(),
            color: "#ffffff".into(),
            align: "left".into(),
            font: "".into(),
        },
    );

    let mut drawings = HashMap::new();
    drawings.insert(
        1,
        WsDrawing {
            tool: "highlighter".into(),
            points: vec![5.0, 10.0, 15.0, 20.0],
            color: "#ffaa00".into(),
            width: 24.0,
            opacity: 0.35,
        },
    );

    let mut slides = HashMap::new();
    slides.insert(
        1,
        WsSlide {
            x: 0,
            y: 0,
            w: 1280,
            h: 720,
            order: 1,
            label: "Slide 1".into(),
        },
    );

    let snap = JsonSessionSnapshot {
        notes,
        text_blocks,
        drawings,
        slides,
        widgets: HashMap::new(),
        shells: HashMap::new(),
        shell_names: HashMap::new(),
        counters: sshx_core::AllCounterValues {
            next_sid: 5,
            next_uid: 3,
            next_nid: 10,
            next_wid: 8,
            next_vid: 2,
            next_tid: 7,
            next_did: 4,
            next_slid: 6,
        },
    };

    let snapshot_bytes = serde_json::to_vec(&snap)?;

    // Open a session with the restore snapshot.
    let mut grpc = server.grpc_client().await;
    let resp = grpc
        .open(OpenRequest {
            origin: server.endpoint(),
            encrypted_zeros: encrypt.zeros().into(),
            name: "restore-test".into(),
            write_password_hash: None,
            restore_snapshot: Some(snapshot_bytes.into()),
        })
        .await?
        .into_inner();

    let name = resp.name.clone();

    // Connect a WebSocket client and verify the restored state.
    let mut ws = ClientSocket::connect(&server.ws_endpoint(&name), "restorekey12345", None).await?;
    ws.flush().await;

    // The session should have been restored with our data.
    let session = server.state().lookup(&name).unwrap();

    // Verify notes.
    let notes = session.list_notes();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].1.text, "<p>Restored note</p>");
    assert_eq!(notes[0].1.color, "blue");
    assert!(notes[0].1.pinned);

    // Verify text blocks.
    let text_blocks = session.list_text_blocks();
    assert_eq!(text_blocks.len(), 1);
    assert_eq!(text_blocks[0].1.content, "<p>Restored text</p>");

    // Verify drawings.
    let drawings = session.list_drawings();
    assert_eq!(drawings.len(), 1);
    assert_eq!(drawings[0].1.tool, "highlighter");
    assert_eq!(drawings[0].1.points.len(), 4);

    // Verify slides.
    let slides = session.list_slides();
    assert_eq!(slides.len(), 1);
    assert_eq!(slides[0].1.label, "Slide 1");
    assert_eq!(slides[0].1.w, 1280);

    // Verify counters were restored (next IDs should be high enough to avoid conflicts).
    let next_nid = session.counter().next_nid();
    assert!(next_nid.0 >= 10, "next_nid should be >= 10, got {}", next_nid.0);

    Ok(())
}

/// End-to-end test: create a session via Controller, add canvas objects via WebSocket,
/// take a snapshot, then restore it on a fresh session and verify everything is present.
#[tokio::test]
async fn test_e2e_session_persistence() -> Result<()> {
    use sshx::{controller::Controller, runner::Runner};

    let server = TestServer::new().await;

    // 1. Create a session with a Controller (simulating the CLI).
    let mut controller =
        Controller::new(&server.endpoint(), "persistence-user", Runner::Echo, false, None, None, None).await?;
    let name = controller.name().to_owned();
    let key = controller.encryption_key().to_owned();
    tokio::spawn(async move { controller.run().await });

    // 2. Connect a WebSocket client and create objects.
    let mut ws = ClientSocket::connect(&server.ws_endpoint(&name), &key, None).await?;
    ws.flush().await;
    assert!(ws.user_id.0 > 0, "should get a valid user ID");

    // Create a terminal.
    ws.send(WsClient::Create(100, 200)).await;
    ws.flush().await;
    assert!(!ws.shells.is_empty(), "should have at least one shell");

    // Get the actual shell ID.
    let shell_id = *ws.shells.keys().next().unwrap();

    // Send some terminal input.
    ws.send_input(shell_id, b"echo hello").await;
    ws.flush().await;

    // Create a note.
    ws.send(WsClient::CreateNote(300, 400)).await;
    ws.flush().await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Get the note ID from the session.
    let session_ref = server.state().lookup(&name).unwrap();
    let note_list = session_ref.list_notes();
    assert!(!note_list.is_empty(), "note should have been created");
    let note_id = note_list[0].0;

    // Update the note.
    ws.send(WsClient::UpdateNote(
        note_id,
        WsNote {
            x: 300,
            y: 400,
            text: "<p>E2E Test Note</p>".into(),
            color: "green".into(),
            pinned: false,
            w: 260,
            h: 0,
            font: "permanent-marker".into(),
        },
    ))
    .await;
    ws.flush().await;

    // Create a text block.
    ws.send(WsClient::CreateTextBlock(500, 600)).await;
    ws.flush().await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Get the text block ID from the session.
    let tb_list = session_ref.list_text_blocks();
    assert!(!tb_list.is_empty(), "text block should have been created");
    let tb_id = tb_list[0].0;

    // Update the text block.
    ws.send(WsClient::UpdateTextBlock(
        tb_id,
        WsTextBlock {
            x: 500,
            y: 600,
            content: "<p>E2E Text Block</p>".into(),
            font_size: "xl".into(),
            color: "#00ff00".into(),
            align: "right".into(),
            font: "space-mono".into(),
        },
    ))
    .await;
    ws.flush().await;

    // Create a drawing.
    ws.send(WsClient::CreateDrawing(WsDrawing {
        tool: "pencil".into(),
        points: vec![1.0, 2.0, 3.0, 4.0],
        color: "#ff0000".into(),
        width: 2.0,
        opacity: 1.0,
    }))
    .await;
    ws.flush().await;

    // Create a slide.
    ws.send(WsClient::CreateSlide(WsSlide {
        x: 0,
        y: 0,
        w: 1920,
        h: 1080,
        order: 1,
        label: "E2E Slide".into(),
    }))
    .await;
    ws.flush().await;

    // Wait for the server to process all messages.
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let notes = session_ref.list_notes();
        let tbs = session_ref.list_text_blocks();
        let has_note_text = notes.iter().any(|(_, n)| !n.text.is_empty());
        let has_tb_text = tbs.iter().any(|(_, t)| !t.content.is_empty());
        if has_note_text && has_tb_text {
            break;
        }
    }

    // 3. Take a JSON snapshot of the session.
    let session = server.state().lookup(&name).unwrap();
    let snap = session.json_snapshot();

    // Verify the snapshot has all the objects we created.
    assert!(!snap.notes.is_empty(), "snapshot should have notes");
    assert!(
        !snap.text_blocks.is_empty(),
        "snapshot should have text blocks"
    );
    assert!(!snap.drawings.is_empty(), "snapshot should have drawings");
    assert!(!snap.slides.is_empty(), "snapshot should have slides");
    assert!(!snap.shells.is_empty(), "snapshot should have shells");

    // Check that at least one note has the expected content.
    assert!(
        snap.notes
            .values()
            .any(|n| n.text == "<p>E2E Test Note</p>" && n.font == "permanent-marker"),
        "should have a note with expected content"
    );

    // Check that at least one text block has the expected content.
    assert!(
        snap.text_blocks
            .values()
            .any(|t| t.content == "<p>E2E Text Block</p>" && t.font == "space-mono"),
        "should have a text block with expected content"
    );

    // Check drawing.
    assert!(
        snap.drawings.values().any(|d| d.tool == "pencil"),
        "should have a pencil drawing"
    );

    // Check slide.
    assert!(
        snap.slides.values().any(|s| s.label == "E2E Slide"),
        "should have a slide with expected label"
    );

    // 4. Serialize the snapshot and create a new session with it.
    let snapshot_json = serde_json::to_vec(&snap)?;

    let encrypt2 = Encrypt::new("newrestorekey14");
    let mut grpc = server.grpc_client().await;
    let resp2 = grpc
        .open(OpenRequest {
            origin: server.endpoint(),
            encrypted_zeros: encrypt2.zeros().into(),
            name: "restored-e2e".into(),
            write_password_hash: None,
            restore_snapshot: Some(snapshot_json.into()),
        })
        .await?
        .into_inner();

    let name2 = resp2.name.clone();

    // 5. Verify the restored session has all the objects.
    let session2 = server.state().lookup(&name2).unwrap();

    let restored_notes = session2.list_notes();
    assert_eq!(
        restored_notes.len(),
        snap.notes.len(),
        "restored notes count should match"
    );
    assert!(
        restored_notes
            .iter()
            .any(|(_, n)| n.text == "<p>E2E Test Note</p>" && n.font == "permanent-marker"),
        "restored session should have the note with expected content"
    );

    let restored_text_blocks = session2.list_text_blocks();
    assert_eq!(
        restored_text_blocks.len(),
        snap.text_blocks.len(),
        "restored text blocks count should match"
    );
    assert!(
        restored_text_blocks
            .iter()
            .any(|(_, t)| t.content == "<p>E2E Text Block</p>"),
        "restored session should have the text block with expected content"
    );

    let restored_drawings = session2.list_drawings();
    assert_eq!(
        restored_drawings.len(),
        snap.drawings.len(),
        "restored drawings count should match"
    );

    let restored_slides = session2.list_slides();
    assert_eq!(
        restored_slides.len(),
        snap.slides.len(),
        "restored slides count should match"
    );

    // 6. Connect a WebSocket client to the restored session and verify via WS.
    let mut ws2 =
        ClientSocket::connect(&server.ws_endpoint(&name2), "newrestorekey14", None).await?;
    ws2.flush().await;
    assert!(
        ws2.user_id.0 > 0,
        "should get a valid user ID on restored session"
    );

    Ok(())
}
