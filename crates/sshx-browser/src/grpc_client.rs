//! gRPC client for the BrowserService.

use anyhow::{Context, Result};
use sshx_core::proto::{
    browser_service_client::BrowserServiceClient, BrowserJoinRequest,
};
use tonic::transport::Channel;
use tracing::info;

use crate::Options;

/// Connect to the sshx server and announce the browser stream.
///
/// Returns the assigned `vid` (video stream ID).
pub async fn join_session(opts: &Options) -> Result<u32> {
    let channel = Channel::from_shared(opts.server.clone())
        .context("invalid server URL")?
        .connect()
        .await
        .context("failed to connect to sshx server")?;

    let mut client = BrowserServiceClient::new(channel);

    let response = client
        .join(BrowserJoinRequest {
            session_name: opts.session.clone(),
            token: opts.token.clone(),
            width: opts.width,
            height: opts.height,
        })
        .await
        .context("BrowserService.Join RPC failed")?;

    let vid = response.into_inner().vid;
    info!("server assigned vid={vid}");
    Ok(vid)
}

/// Create a gRPC channel to the sshx server.
pub async fn connect(server: &str) -> Result<BrowserServiceClient<Channel>> {
    let channel = Channel::from_shared(server.to_string())
        .context("invalid server URL")?
        .connect()
        .await
        .context("failed to connect to sshx server")?;
    Ok(BrowserServiceClient::new(channel))
}
