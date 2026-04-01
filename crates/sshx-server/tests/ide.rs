use std::path::PathBuf;

use anyhow::Result;
use sshx::controller::Controller;
use sshx::runner::Runner;
use sshx_server::web::protocol::{WsClient, WsWidget, WsWidgetKind};
use tokio::time::{self, Duration};

pub mod common;
use crate::common::*;

/// Controller with no IDE binary — IDE unavailable.
async fn make_controller_no_ide(server: &TestServer) -> Controller {
    Controller::new(&server.endpoint(), "", Runner::Echo, false, None, None, None)
        .await
        .unwrap()
}

/// Controller with a fake IDE binary path — IDE available (lazy spawn, never triggered).
async fn make_controller_with_ide(server: &TestServer) -> Controller {
    Controller::new(
        &server.endpoint(),
        "",
        Runner::Echo,
        false,
        None,
        Some(PathBuf::from("/nonexistent/openvscode-server")),
        None,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn test_ide_not_available_at_start() -> Result<()> {
    let server = TestServer::new().await;

    let mut controller = make_controller_no_ide(&server).await;
    let name = controller.name().to_owned();
    let key = controller.encryption_key().to_owned();
    tokio::spawn(async move { controller.run().await });

    let mut s = ClientSocket::connect(&server.ws_endpoint(&name), &key, None).await?;
    s.flush().await;

    assert!(!s.ide_available, "IDE should not be available without IDE binary");
    assert!(
        !s.widgets.values().any(|w| matches!(w.kind, WsWidgetKind::IdeEditor { .. })),
        "no IDE editor widgets should exist"
    );

    Ok(())
}

#[tokio::test]
async fn test_ide_available_after_cli_connects() -> Result<()> {
    let server = TestServer::new().await;

    let mut controller = make_controller_with_ide(&server).await;
    let name = controller.name().to_owned();
    let key = controller.encryption_key().to_owned();
    tokio::spawn(async move { controller.run().await });

    // Wait for the gRPC channel to establish so the server has ide_available=true
    // before the WS client connects (IdeAvailable is sent once at connect time).
    time::sleep(Duration::from_millis(100)).await;

    let mut s = ClientSocket::connect(&server.ws_endpoint(&name), &key, None).await?;
    s.flush().await;

    assert!(s.ide_available, "IDE should be available when CLI connects with IDE binary");

    Ok(())
}

#[tokio::test]
async fn test_ide_create_new_widget() -> Result<()> {
    let server = TestServer::new().await;

    let mut controller = make_controller_with_ide(&server).await;
    let name = controller.name().to_owned();
    let key = controller.encryption_key().to_owned();
    tokio::spawn(async move { controller.run().await });

    // Wait for the gRPC channel to establish so the server has ide_available=true
    // before the WS client connects (IdeAvailable is sent once at connect time).
    time::sleep(Duration::from_millis(100)).await;

    let mut s = ClientSocket::connect(&server.ws_endpoint(&name), &key, None).await?;
    s.flush().await;

    assert!(s.ide_available);
    assert!(
        !s.widgets.values().any(|w| matches!(w.kind, WsWidgetKind::IdeEditor { .. })),
        "no IDE editor widgets at start"
    );

    // Request a new IDE editor widget
    s.send(WsClient::OpenIdeEditor(100, 200, "my-project".into())).await;
    s.flush().await;

    // Find the IDE editor widget
    let ide_widgets: Vec<_> = s
        .widgets
        .iter()
        .filter(|(_, w)| matches!(w.kind, WsWidgetKind::IdeEditor { .. }))
        .collect();

    assert_eq!(ide_widgets.len(), 1, "should have exactly one IDE editor widget");

    let (wid, widget) = ide_widgets[0];
    assert_eq!(widget.x, 100, "widget x should match requested position");
    assert_eq!(widget.y, 200, "widget y should match requested position");

    match &widget.kind {
        WsWidgetKind::IdeEditor { workspace_label, ide_id } => {
            assert_eq!(workspace_label, "my-project");
            assert_eq!(*ide_id, wid.0, "ide_id should match widget Wid");
            assert!(*ide_id > 0, "ide_id should be non-zero");
        }
        _ => panic!("expected IdeEditor kind"),
    }

    Ok(())
}

#[tokio::test]
async fn test_ide_widget_broadcast_to_second_client() -> Result<()> {
    let server = TestServer::new().await;

    let mut controller = make_controller_with_ide(&server).await;
    let name = controller.name().to_owned();
    let key = controller.encryption_key().to_owned();
    tokio::spawn(async move { controller.run().await });

    // Wait for the gRPC channel to establish so ide_available=true before WS connects.
    time::sleep(Duration::from_millis(100)).await;

    let endpoint = server.ws_endpoint(&name);

    let mut alice = ClientSocket::connect(&endpoint, &key, None).await?;
    alice.flush().await;

    let mut bob = ClientSocket::connect(&endpoint, &key, None).await?;
    bob.flush().await;

    // Alice opens an IDE editor
    alice.send(WsClient::OpenIdeEditor(50, 75, "shared-project".into())).await;
    alice.flush().await;

    // Bob should also see the new widget
    bob.flush().await;

    let bob_ide_widgets: Vec<_> = bob
        .widgets
        .values()
        .filter(|w| matches!(w.kind, WsWidgetKind::IdeEditor { .. }))
        .collect();

    assert_eq!(bob_ide_widgets.len(), 1, "Bob should see the IDE editor widget created by Alice");

    match &bob_ide_widgets[0].kind {
        WsWidgetKind::IdeEditor { workspace_label, .. } => {
            assert_eq!(workspace_label, "shared-project");
        }
        _ => panic!("expected IdeEditor kind"),
    }

    Ok(())
}

#[tokio::test]
async fn test_ide_open_without_ide_returns_error() -> Result<()> {
    let server = TestServer::new().await;

    let mut controller = make_controller_no_ide(&server).await;
    let name = controller.name().to_owned();
    let key = controller.encryption_key().to_owned();
    tokio::spawn(async move { controller.run().await });

    let mut s = ClientSocket::connect(&server.ws_endpoint(&name), &key, None).await?;
    s.flush().await;

    assert!(!s.ide_available, "IDE should not be available");

    // Try to open an IDE editor — should fail
    s.send(WsClient::OpenIdeEditor(0, 0, "test".into())).await;
    s.flush().await;

    assert!(!s.errors.is_empty(), "should receive an error message");
    assert!(
        s.errors.iter().any(|e| e.contains("IDE not available")),
        "error should mention IDE not available, got: {:?}",
        s.errors
    );
    assert!(
        !s.widgets.values().any(|w| matches!(w.kind, WsWidgetKind::IdeEditor { .. })),
        "no IDE editor widgets should have been created"
    );

    Ok(())
}

/// Diagnostic: verify that ciborium serializes WsWidgetKind::IdeEditor field names
/// as camelCase (ideId, workspaceLabel) rather than snake_case (ide_id, workspace_label).
#[test]
fn test_ide_editor_cbor_field_names() {
    let kind = WsWidgetKind::IdeEditor {
        workspace_label: "test-project".into(),
        ide_id: 42,
    };
    // Serialize to CBOR
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&kind, &mut buf).unwrap();

    // Deserialize as ciborium::Value to inspect raw keys
    let value: ciborium::Value = ciborium::de::from_reader(&buf[..]).unwrap();

    // Value should be a Map with string keys
    let map = value.as_map().expect("should be a CBOR map");
    let keys: Vec<&str> = map
        .iter()
        .filter_map(|(k, _)| k.as_text())
        .collect();

    println!("CBOR map keys for WsWidgetKind::IdeEditor: {:?}", keys);

    assert!(keys.contains(&"type"), "missing 'type' tag");
    // These will fail if ciborium emits snake_case despite #[serde(rename)]
    assert!(
        keys.contains(&"ideId"),
        "field should be 'ideId', not 'ide_id' — actual keys: {:?}",
        keys
    );
    assert!(
        keys.contains(&"workspaceLabel"),
        "field should be 'workspaceLabel', not 'workspace_label' — actual keys: {:?}",
        keys
    );
}

/// Diagnostic: verify full WsWidget with IdeEditor kind serializes correctly via CBOR.
#[test]
fn test_widget_ide_editor_cbor_full() {
    let widget = WsWidget {
        x: 100,
        y: 200,
        w: 900,
        h: 600,
        kind: WsWidgetKind::IdeEditor {
            workspace_label: "my-project".into(),
            ide_id: 5,
        },
        collapsed: false,
        name: None,
    };
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&widget, &mut buf).unwrap();
    let value: ciborium::Value = ciborium::de::from_reader(&buf[..]).unwrap();

    println!("Full WsWidget CBOR Value: {:#?}", value);

    // Navigate into the "kind" field and check its keys
    let widget_map = value.as_map().expect("widget should be a CBOR map");
    let kind_entry = widget_map
        .iter()
        .find(|(k, _)| k.as_text() == Some("kind"))
        .expect("widget should have a 'kind' field");
    let kind_map = kind_entry.1.as_map().expect("kind should be a CBOR map");
    let kind_keys: Vec<&str> = kind_map
        .iter()
        .filter_map(|(k, _)| k.as_text())
        .collect();

    println!("kind CBOR keys: {:?}", kind_keys);

    assert!(
        kind_keys.contains(&"ideId"),
        "kind should contain 'ideId', not 'ide_id' — actual keys: {:?}",
        kind_keys
    );
    assert!(
        kind_keys.contains(&"workspaceLabel"),
        "kind should contain 'workspaceLabel' — actual keys: {:?}",
        kind_keys
    );
}
