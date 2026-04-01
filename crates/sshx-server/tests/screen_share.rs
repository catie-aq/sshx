use anyhow::Result;
use sshx::encrypt::Encrypt;
use sshx::{controller::Controller, runner::Runner};
use sshx_core::{
    proto::{
        browser_service_client::BrowserServiceClient, browser_update::BrowserMessage,
        sshx_service_client::SshxServiceClient, BrowserJoinRequest, BrowserUpdate, OpenRequest,
        VideoFrame,
    },
    Vid,
};
use sshx_server::web::protocol::{WsClient, WsIceCandidate};
use tokio::time::{self, Duration};
use tonic::transport::Channel;

use crate::common::*;

pub mod common;

/// Helper: open a session via gRPC Open() and return (name, token).
/// Uses a fixed encryption key "testkey1234567".
async fn open_session_raw(
    client: &mut SshxServiceClient<Channel>,
) -> Result<(String, String)> {
    let encrypt = Encrypt::new("testkey1234567");
    let encrypted_zeros = encrypt.zeros();

    let resp = client
        .open(OpenRequest {
            origin: "http://localhost".into(),
            encrypted_zeros: encrypted_zeros.into(),
            name: "test-session".into(),
            write_password_hash: None,
            restore_snapshot: None,
        })
        .await?
        .into_inner();

    Ok((resp.name, resp.token))
}

#[tokio::test]
async fn test_screen_share_lifecycle() -> Result<()> {
    let server = TestServer::new().await;

    let mut controller = Controller::new(&server.endpoint(), "", Runner::Echo, false, None, None, None).await?;
    let name = controller.name().to_owned();
    let key = controller.encryption_key().to_owned();
    tokio::spawn(async move { controller.run().await });

    let endpoint = server.ws_endpoint(&name);
    let mut s = ClientSocket::connect(&endpoint, &key, None).await?;
    s.flush().await;

    // Initially no video streams
    assert!(s.video_streams.is_empty());

    // Start screen share
    s.send(WsClient::StartScreenShare).await;
    s.flush().await;

    // Should have one video stream
    assert_eq!(s.video_streams.len(), 1);
    let (_, stream) = s.video_streams.iter().next().unwrap();
    assert_eq!(stream.owner_uid, Some(s.user_id));
    assert!(!stream.is_browser);

    // Stop screen share
    s.send(WsClient::StopScreenShare).await;
    s.flush().await;

    // Video stream should be removed
    assert!(
        s.video_streams.is_empty(),
        "video_streams should be empty after StopScreenShare, got: {:?}",
        s.video_streams
    );

    // Start again to verify lifecycle reset
    s.send(WsClient::StartScreenShare).await;
    s.flush().await;
    assert_eq!(s.video_streams.len(), 1);

    Ok(())
}

#[tokio::test]
async fn test_p2p_signaling_relay() -> Result<()> {
    let server = TestServer::new().await;

    let mut controller = Controller::new(&server.endpoint(), "", Runner::Echo, false, None, None, None).await?;
    let name = controller.name().to_owned();
    let key = controller.encryption_key().to_owned();
    tokio::spawn(async move { controller.run().await });

    let endpoint = server.ws_endpoint(&name);

    // Connect two clients
    let mut alice = ClientSocket::connect(&endpoint, &key, None).await?;
    alice.flush().await;
    let alice_uid = alice.user_id;

    let mut bob = ClientSocket::connect(&endpoint, &key, None).await?;
    bob.flush().await;
    let bob_uid = bob.user_id;

    // Alice starts screen share
    alice.send(WsClient::StartScreenShare).await;
    alice.flush().await;
    assert_eq!(alice.video_streams.len(), 1);

    let vid = *alice.video_streams.keys().next().unwrap();

    // Bob should also see the stream via broadcast
    bob.flush().await;
    assert_eq!(bob.video_streams.len(), 1);

    // Bob watches the stream — server sends an RtcOffer broadcast with "watch:{bob_uid}"
    bob.send(WsClient::WatchStream(vid)).await;
    alice.flush().await;

    // Alice should receive an RtcOffer with the "watch:{bob_uid}" SDP
    assert!(
        !alice.rtc_offers.is_empty(),
        "Alice should receive an RtcOffer when Bob watches"
    );
    let (offer_vid, _offer_target, offer_sdp) = &alice.rtc_offers[0];
    assert_eq!(*offer_vid, vid);
    assert!(offer_sdp.contains(&format!("watch:{}", bob_uid.0)));

    // Alice sends an RTC offer to Bob
    alice
        .send(WsClient::SendRtcOffer(
            vid,
            bob_uid,
            "offer-sdp-from-alice".into(),
        ))
        .await;
    bob.flush().await;

    // Bob should receive the offer. Since RTC messages are broadcast, Bob may
    // also see the earlier "watch:" notification. Find Alice's actual offer.
    let alice_offer = bob
        .rtc_offers
        .iter()
        .find(|(_, target, sdp)| *target == bob_uid && sdp == "offer-sdp-from-alice")
        .expect("Bob should receive RtcOffer from Alice");
    assert_eq!(alice_offer.0, vid);

    // Bob sends an RTC answer back to Alice
    bob.send(WsClient::SendRtcAnswer(
        vid,
        alice_uid,
        "answer-sdp-from-bob".into(),
    ))
    .await;
    alice.flush().await;

    // Find Alice's answer (broadcast means Alice may see multiple messages).
    let bob_answer = alice
        .rtc_answers
        .iter()
        .find(|(_, target, _sender, sdp)| *target == alice_uid && sdp == "answer-sdp-from-bob")
        .expect("Alice should receive RtcAnswer from Bob");
    assert_eq!(bob_answer.0, vid);

    // Alice sends an ICE candidate to Bob
    let ice = WsIceCandidate {
        candidate: "candidate:abc".into(),
        sdp_mid: Some("0".into()),
        sdp_mline_index: Some(0),
    };
    alice
        .send(WsClient::SendRtcIce(vid, bob_uid, ice.clone()))
        .await;
    bob.flush().await;

    assert!(
        !bob.rtc_ice_candidates.is_empty(),
        "Bob should receive ICE candidate from Alice"
    );
    let (i_vid, i_target, _i_sender, i_cand) = &bob.rtc_ice_candidates[0];
    assert_eq!(*i_vid, vid);
    assert_eq!(*i_target, bob_uid);
    assert_eq!(i_cand.candidate, "candidate:abc");

    // Bob sends an ICE candidate to Alice
    let ice2 = WsIceCandidate {
        candidate: "candidate:xyz".into(),
        sdp_mid: Some("0".into()),
        sdp_mline_index: Some(0),
    };
    bob.send(WsClient::SendRtcIce(vid, alice_uid, ice2))
        .await;
    alice.flush().await;

    // Alice should have received Bob's ICE candidate
    let alice_ice = alice
        .rtc_ice_candidates
        .iter()
        .find(|(_, _target, _sender, c)| c.candidate == "candidate:xyz")
        .expect("Alice should have received Bob's ICE candidate");
    assert_eq!(alice_ice.0, vid);

    Ok(())
}

#[tokio::test]
async fn test_browser_stream_join_and_frame_relay() -> Result<()> {
    let server = TestServer::new().await;

    // Create session via gRPC Open
    let mut grpc = server.grpc_client().await;
    let (name, token) = open_session_raw(&mut grpc).await?;

    // Connect a BrowserService client
    let mut browser_client = BrowserServiceClient::connect(server.endpoint()).await?;

    // Join browser stream
    let join_resp = browser_client
        .join(BrowserJoinRequest {
            session_name: name.clone(),
            token: token.clone(),
            width: 320,
            height: 240,
        })
        .await?
        .into_inner();
    let vid = Vid(join_resp.vid);

    // Connect a WebSocket client
    let key = "testkey1234567";
    let mut ws = ClientSocket::connect(&server.ws_endpoint(&name), key, None).await?;
    ws.flush().await;

    // WebSocket client should see the browser video stream
    assert_eq!(ws.video_streams.len(), 1);
    let stream = ws.video_streams.get(&vid).expect("should have vid");
    assert!(stream.is_browser);
    assert_eq!(stream.owner_uid, None);

    // Subscribe to the stream
    ws.send(WsClient::WatchStream(vid)).await;

    // Build distinct frame payloads with recognizable byte patterns.
    // We'll send three frames and verify the WS client receives each one
    // with identical data, timestamp, and keyframe flag.
    let frames_sent: Vec<(Vec<u8>, u64, bool)> = vec![
        (vec![0x9D, 0x01, 0x2A, 0x40, 0x01, 0xAA, 0xBB, 0xCC], 1000, true),   // keyframe
        (vec![0x10, 0x20, 0x30, 0x40, 0x50],                      2000, false), // delta
        (vec![0x9D, 0x01, 0x2A, 0x40, 0x01, 0xDD, 0xEE, 0xFF],   3000, true),  // second keyframe
    ];

    let (tx, rx) = tokio::sync::mpsc::channel::<BrowserUpdate>(16);
    let rx_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    // Enqueue all frames before calling Stream() so they're available.
    for (data, ts, kf) in &frames_sent {
        tx.send(BrowserUpdate {
            browser_message: Some(BrowserMessage::Frame(VideoFrame {
                vid: vid.0,
                data: data.clone().into(),
                timestamp: *ts,
                keyframe: *kf,
            })),
        })
        .await?;
    }

    // Start the stream RPC (returns a stream of BrowserCommands)
    let _cmd_stream = browser_client.stream(rx_stream).await?;

    // Wait for frames to propagate through the server to the WS subscriber.
    time::sleep(Duration::from_millis(300)).await;
    ws.flush().await;

    // ---- Byte-for-byte data validation ----

    // We should have received at least as many frames as we sent.
    assert!(
        ws.browser_frames.len() >= frames_sent.len(),
        "Expected at least {} frames, got {}",
        frames_sent.len(),
        ws.browser_frames.len()
    );

    // Verify each sent frame is present in the received frames with matching
    // data, timestamp, and keyframe flag.
    for (i, (expected_data, expected_ts, expected_kf)) in frames_sent.iter().enumerate() {
        let (recv_vid, recv_ts, recv_data, recv_kf) = &ws.browser_frames[i];

        // Correct video stream ID
        assert_eq!(
            *recv_vid, vid,
            "Frame {i}: vid mismatch — expected {vid}, got {recv_vid}"
        );

        // Byte-for-byte data comparison
        assert_eq!(
            recv_data.as_ref(),
            expected_data.as_slice(),
            "Frame {i}: data bytes differ!\n  sent:     {:02X?}\n  received: {:02X?}",
            expected_data,
            recv_data.as_ref()
        );

        // Timestamp must match exactly
        assert_eq!(
            *recv_ts, *expected_ts,
            "Frame {i}: timestamp mismatch — expected {expected_ts}, got {recv_ts}"
        );

        // Keyframe flag must match
        assert_eq!(
            *recv_kf, *expected_kf,
            "Frame {i}: keyframe flag mismatch — expected {expected_kf}, got {recv_kf}"
        );
    }

    // Specifically verify we got two keyframes and one delta
    let keyframe_count = ws.browser_frames.iter().filter(|(_, _, _, kf)| *kf).count();
    let delta_count = ws.browser_frames.iter().filter(|(_, _, _, kf)| !*kf).count();
    assert!(keyframe_count >= 2, "Expected at least 2 keyframes, got {keyframe_count}");
    assert!(delta_count >= 1, "Expected at least 1 delta frame, got {delta_count}");

    Ok(())
}

#[tokio::test]
async fn test_browser_control() -> Result<()> {
    let server = TestServer::new().await;

    // Create session via gRPC Open
    let mut grpc = server.grpc_client().await;
    let (name, token) = open_session_raw(&mut grpc).await?;

    // Join browser stream
    let mut browser_client = BrowserServiceClient::connect(server.endpoint()).await?;
    let join_resp = browser_client
        .join(BrowserJoinRequest {
            session_name: name.clone(),
            token: token.clone(),
            width: 320,
            height: 240,
        })
        .await?
        .into_inner();
    let vid = Vid(join_resp.vid);

    let key = "testkey1234567";
    let endpoint = server.ws_endpoint(&name);

    // Connect two WS clients
    let mut alice = ClientSocket::connect(&endpoint, key, None).await?;
    alice.flush().await;
    let alice_uid = alice.user_id;

    let mut bob = ClientSocket::connect(&endpoint, key, None).await?;
    bob.flush().await;
    let bob_uid = bob.user_id;

    // Initially, browser control should be None
    assert_eq!(
        alice.browser_control.get(&vid),
        Some(&None),
        "Browser stream should start with no controller"
    );

    // Alice requests control
    alice.send(WsClient::RequestBrowserControl(vid)).await;
    alice.flush().await;

    assert_eq!(
        alice.browser_control.get(&vid),
        Some(&Some(alice_uid)),
        "Alice should be the controller"
    );

    // Bob tries to request control — should be denied (already controlled)
    bob.send(WsClient::RequestBrowserControl(vid)).await;
    bob.flush().await;

    // Bob should still see Alice as controller (no change broadcast)
    assert_eq!(
        bob.browser_control.get(&vid),
        Some(&Some(alice_uid)),
        "Alice should still be the controller after Bob's failed request"
    );

    // Alice releases control
    alice.send(WsClient::ReleaseBrowserControl(vid)).await;
    alice.flush().await;

    assert_eq!(
        alice.browser_control.get(&vid),
        Some(&None),
        "No one should be controlling after release"
    );

    // Bob requests control — should succeed now
    bob.send(WsClient::RequestBrowserControl(vid)).await;
    bob.flush().await;

    assert_eq!(
        bob.browser_control.get(&vid),
        Some(&Some(bob_uid)),
        "Bob should be the controller now"
    );

    Ok(())
}

#[tokio::test]
async fn test_video_stream_move_resize() -> Result<()> {
    let server = TestServer::new().await;

    let mut controller = Controller::new(&server.endpoint(), "", Runner::Echo, false, None, None, None).await?;
    let name = controller.name().to_owned();
    let key = controller.encryption_key().to_owned();
    tokio::spawn(async move { controller.run().await });

    let mut s = ClientSocket::connect(&server.ws_endpoint(&name), &key, None).await?;
    s.flush().await;

    // Start screen share
    s.send(WsClient::StartScreenShare).await;
    s.flush().await;
    assert_eq!(s.video_streams.len(), 1);

    let vid = *s.video_streams.keys().next().unwrap();

    // Move the video stream
    s.send(WsClient::MoveVideoStream(vid, 200, 300)).await;
    s.flush().await;

    let stream = s.video_streams.get(&vid).unwrap();
    assert_eq!(stream.x, 200, "x should be updated to 200");
    assert_eq!(stream.y, 300, "y should be updated to 300");

    // Resize the video stream
    s.send(WsClient::ResizeVideoStream(vid, 800, 600)).await;
    s.flush().await;

    let stream = s.video_streams.get(&vid).unwrap();
    assert_eq!(stream.w, 800, "width should be updated to 800");
    assert_eq!(stream.h, 600, "height should be updated to 600");
    // Position should be unchanged after resize
    assert_eq!(stream.x, 200, "x should remain 200 after resize");
    assert_eq!(stream.y, 300, "y should remain 300 after resize");

    Ok(())
}
