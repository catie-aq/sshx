use anyhow::Result;
use sshx::encrypt::Encrypt;
use sshx_core::proto::*;
use sshx_server::web::protocol::WsClient;

use crate::common::*;

pub mod common;

#[tokio::test]
async fn test_rpc() -> Result<()> {
    let server = TestServer::new().await;
    let mut client = server.grpc_client().await;

    let req = OpenRequest {
        origin: "sshx.io".into(),
        encrypted_zeros: Encrypt::new("").zeros().into(),
        name: String::new(),
        write_password_hash: None,
        restore_snapshot: None,
    };
    let resp = client.open(req).await?;
    assert!(!resp.into_inner().name.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_web_get() -> Result<()> {
    let server = TestServer::new().await;

    let resp = reqwest::get(server.endpoint()).await?;
    assert!(!resp.status().is_server_error());

    Ok(())
}

/// Verify that `{ requestContextSnapshot: true }` (the CBOR cbor-x sends) can
/// be decoded as `WsClient::RequestContextSnapshot` by ciborium.
///
/// This is a regression test: if deserialization fails, socket.rs propagates
/// the error via `?` and closes the WebSocket, causing the "disconnect on
/// Refresh" bug.
#[test]
fn test_cbor_unit_variant_from_map_true() {
    // Simulate what cbor-x sends for `{ requestContextSnapshot: true }`:
    // a CBOR map with one key-value pair.
    fn encode_as_map(key: &str) -> Vec<u8> {
        let value = ciborium::Value::Map(vec![(
            ciborium::Value::Text(key.to_string()),
            ciborium::Value::Bool(true),
        )]);
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&value, &mut buf).unwrap();
        buf
    }

    let bytes = encode_as_map("requestContextSnapshot");
    let result: Result<WsClient, _> = ciborium::de::from_reader(&*bytes);
    assert!(
        result.is_ok(),
        "CBOR deserialization of {{requestContextSnapshot: true}} failed: {:?}",
        result.err()
    );
    assert!(matches!(result.unwrap(), WsClient::RequestContextSnapshot));

    // Also test startScreenShare (same pattern, same potential bug).
    let bytes2 = encode_as_map("startScreenShare");
    let result2: Result<WsClient, _> = ciborium::de::from_reader(&*bytes2);
    assert!(
        result2.is_ok(),
        "CBOR deserialization of {{startScreenShare: true}} failed: {:?}",
        result2.err()
    );
    assert!(matches!(result2.unwrap(), WsClient::StartScreenShare));
}
