//! Registration of sshx sessions with an sshx platform server.
//!
//! The platform is a multi-user web app listing every terminal of a user's
//! machines (see the `sshx-dev-platform` repository). A machine authenticates
//! with a **machine token** created in the platform UI:
//!
//! ```text
//! smt_<auth secret>.<machine key>
//! ```
//!
//! Only the part before the dot is ever sent to the platform, as a bearer
//! token. The machine key (32 bytes, base64url) stays on the machine and in the
//! owner's browser, like the URL fragment of a plain sshx link. It encrypts:
//!
//! - the sshx session key (`keyEnc`), so the owner's browser can open the
//!   session without the platform learning the key;
//! - per-terminal metadata (`metaEnc`): directory, git, process, agent detail.
//!
//! The platform only sees session names, terminal IDs and agent states.
//!
//! Ciphertexts use AES-256-GCM: `"v1." + base64url(nonce ‖ ciphertext ‖ tag)`,
//! with an associated-data string binding each blob to its slot, so the
//! server cannot swap blobs between sessions or terminals.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::oneshot;
use tracing::{debug, info, warn};

use crate::agent::{TerminalInfo, TerminalRegistry};

/// Maximum delay between two state reports, also the platform heartbeat.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

/// Delay to batch bursts of changes into a single report.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// Version prefix of ciphertexts.
const CIPHER_PREFIX: &str = "v1.";

/// Associated data for the wrapped sshx session key.
pub fn aad_session_key(session: &str) -> String {
    format!("sshx-platform/v1/session-key/{session}")
}

/// Associated data for the encrypted metadata of a terminal.
pub fn aad_terminal_meta(session: &str, sid: u32) -> String {
    format!("sshx-platform/v1/terminal-meta/{session}/{sid}")
}

/// Encrypt `plaintext` with AES-256-GCM under `key`, bound to `aad`.
pub fn seal(key: &[u8; 32], aad: &str, plaintext: &[u8]) -> String {
    let cipher = Aes256Gcm::new(key.into());
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: plaintext, aad: aad.as_bytes() })
        .expect("AES-GCM encryption cannot fail for in-memory buffers");
    let mut out = nonce.to_vec();
    out.extend_from_slice(&ct);
    format!("{CIPHER_PREFIX}{}", URL_SAFE_NO_PAD.encode(out))
}

/// Decrypt a value produced by [`seal`].
pub fn open(key: &[u8; 32], aad: &str, sealed: &str) -> Result<Vec<u8>> {
    let data = sealed
        .strip_prefix(CIPHER_PREFIX)
        .context("unknown ciphertext version")?;
    let data = URL_SAFE_NO_PAD.decode(data)?;
    if data.len() < 12 + 16 {
        bail!("ciphertext too short");
    }
    let (nonce, ct) = data.split_at(12);
    let cipher = Aes256Gcm::new(key.into());
    cipher
        .decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad: aad.as_bytes() })
        .map_err(|_| anyhow::anyhow!("decryption failed"))
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Platform URL and machine token.
#[derive(Serialize, Deserialize, Clone)]
pub struct PlatformConfig {
    /// Base URL of the platform, e.g. `https://platform.example.ts.net`.
    pub url: String,
    /// Full machine token, `smt_…​.<key>`.
    pub token: String,
    /// sshx server announced by the platform at enrollment, used when
    /// `--server` is not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

impl std::fmt::Debug for PlatformConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlatformConfig")
            .field("url", &self.url)
            .field("token", &"<redacted>")
            .field("server", &self.server)
            .finish()
    }
}

impl PlatformConfig {
    /// Path of the saved configuration, `~/.sshx/platform.json`.
    pub fn path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("cannot find the home directory")?;
        Ok(home.join(".sshx").join("platform.json"))
    }

    /// Load the saved configuration, if any.
    pub fn load() -> Result<Option<Self>> {
        let path = Self::path()?;
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Some(
                serde_json::from_str(&text)
                    .with_context(|| format!("invalid {}", path.display()))?,
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Save the configuration with owner-only permissions.
    pub fn save(&self) -> Result<PathBuf> {
        let path = Self::path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self)?;
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)?;
            file.write_all(text.as_bytes())?;
        }
        #[cfg(not(unix))]
        std::fs::write(&path, text)?;
        Ok(path)
    }

    /// Delete the saved configuration.
    pub fn forget() -> Result<bool> {
        let path = Self::path()?;
        match std::fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// Resolve the configuration from flags/env, falling back to the saved file.
    pub fn resolve(url: Option<String>, token: Option<String>) -> Result<Option<Self>> {
        match (url, token) {
            (Some(url), Some(token)) => Ok(Some(Self { url, token, server: None })),
            (None, None) => Self::load(),
            (url, token) => {
                // Complete a partial override with the saved configuration.
                let saved = Self::load()?;
                let server = saved.as_ref().and_then(|s| s.server.clone());
                match (url.or(saved.as_ref().map(|s| s.url.clone())), token.or(saved.map(|s| s.token))) {
                    (Some(url), Some(token)) => Ok(Some(Self { url, token, server })),
                    _ => bail!("both --platform and --platform-token are required"),
                }
            }
        }
    }

    /// Split the token into its bearer secret and the machine key.
    pub fn credentials(&self) -> Result<(String, [u8; 32])> {
        let (auth, key) = self
            .token
            .trim()
            .rsplit_once('.')
            .context("malformed machine token (expected smt_….<key>)")?;
        if !auth.starts_with("smt_") {
            bail!("malformed machine token (expected the smt_ prefix)");
        }
        let key = URL_SAFE_NO_PAD
            .decode(key)
            .context("malformed machine key in token")?;
        let key: [u8; 32] = key
            .try_into()
            .map_err(|_| anyhow::anyhow!("machine key must be 32 bytes"))?;
        Ok((auth.to_string(), key))
    }
}

// ---------------------------------------------------------------------------
// HTTP client
// ---------------------------------------------------------------------------

/// Identity of the machine as known by the platform.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MachineIdentity {
    /// Machine ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// `"personal"` or `"project"`.
    pub kind: String,
    /// Project label for project machines.
    #[serde(default)]
    pub project: Option<String>,
    /// Owner's username.
    pub owner: String,
    /// sshx server machines should use, as configured on the platform.
    #[serde(default)]
    pub sshx_server: Option<String>,
    /// Sessions of this machine currently online.
    #[serde(default)]
    pub sessions_online: Option<u32>,
}

/// Last sealed metadata per terminal: plaintext and its ciphertext.
type SealedMetaCache = Arc<std::sync::Mutex<HashMap<u32, (Vec<u8>, String)>>>;

/// Client for the platform's agent API.
#[derive(Clone)]
pub struct PlatformClient {
    http: reqwest::Client,
    base: String,
    auth: String,
    key: [u8; 32],
    /// Last sealed metadata per terminal, reused while the plaintext is the
    /// same so that heartbeats do not look like changes to the platform.
    sealed_meta: SealedMetaCache,
}

impl PlatformClient {
    /// Build a client from a configuration.
    pub fn new(config: &PlatformConfig) -> Result<Self> {
        let (auth, key) = config.credentials()?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent(format!("sshx/{}", crate::update::VERSION))
            .build()?;
        Ok(Self {
            http,
            base: config.url.trim_end_matches('/').to_string(),
            auth,
            key,
            sealed_meta: Default::default(),
        })
    }

    /// Base URL of the platform.
    pub fn base_url(&self) -> &str {
        &self.base
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/agent/v1{path}", self.base)
    }

    /// Check the token and return the machine identity.
    pub async fn whoami(&self) -> Result<MachineIdentity> {
        let resp = self
            .http
            .get(self.url("/whoami"))
            .bearer_auth(&self.auth)
            .send()
            .await
            .context("cannot reach the platform")?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            bail!("the platform rejected this machine token (revoked or invalid)");
        }
        Ok(resp.error_for_status()?.json().await?)
    }

    async fn register(&self, reg: &Registration) -> Result<()> {
        let mut secret = json!({ "key": reg.encryption_key });
        if let Some(pw) = &reg.write_password {
            secret["writePassword"] = json!(pw);
        }
        let key_enc = seal(&self.key, &aad_session_key(&reg.name), secret.to_string().as_bytes());
        let body = json!({
            "name": reg.name,
            "url": reg.url,
            "sshxServer": reg.sshx_server,
            "displayName": reg.display_name,
            "keyEnc": key_enc,
            "hostname": whoami::fallible::hostname().unwrap_or_default(),
            "username": whoami::username(),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "version": crate::update::VERSION,
        });
        self.http
            .post(self.url("/sessions"))
            .bearer_auth(&self.auth)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    fn sealed_meta(&self, session: &str, sid: u32, info: &TerminalInfo) -> String {
        let meta = serde_json::to_vec(info).unwrap_or_default();
        let mut cache = self.sealed_meta.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((plain, sealed)) = cache.get(&sid) {
            if *plain == meta {
                return sealed.clone();
            }
        }
        let sealed = seal(&self.key, &aad_terminal_meta(session, sid), &meta);
        cache.insert(sid, (meta, sealed.clone()));
        sealed
    }

    /// Send terminal states. Returns false if the platform forgot the session.
    async fn report(&self, session: &str, terminals: &[(u32, TerminalInfo)]) -> Result<bool> {
        {
            let live: std::collections::HashSet<u32> = terminals.iter().map(|(sid, _)| *sid).collect();
            let mut cache = self.sealed_meta.lock().unwrap_or_else(|e| e.into_inner());
            cache.retain(|sid, _| live.contains(sid));
        }
        let terminals: Vec<_> = terminals
            .iter()
            .map(|(sid, info)| {
                json!({
                    "sid": sid,
                    "state": info.agent_state().as_str(),
                    "since": info.agent.as_ref().map(|a| a.since),
                    "agent": info.agent.as_ref().map(|a| a.kind.clone()),
                    "metaEnc": self.sealed_meta(session, *sid, info),
                })
            })
            .collect();
        let resp = self
            .http
            .put(self.url(&format!("/sessions/{session}/state")))
            .bearer_auth(&self.auth)
            .json(&json!({ "terminals": terminals }))
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(false);
        }
        resp.error_for_status()?;
        Ok(true)
    }

    async fn close(&self, session: &str) -> Result<()> {
        self.http
            .post(self.url(&format!("/sessions/{session}/close")))
            .bearer_auth(&self.auth)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Reporter task
// ---------------------------------------------------------------------------

/// Information about the sshx session to register.
#[derive(Clone)]
pub struct Registration {
    /// sshx session name.
    pub name: String,
    /// Public session URL, without the key fragment.
    pub url: String,
    /// sshx server origin used by the CLI.
    pub sshx_server: String,
    /// Display name (`user@host` by default).
    pub display_name: String,
    /// End-to-end encryption key (URL fragment), sent only encrypted.
    pub encryption_key: String,
    /// Write password for read-only sessions, sent only encrypted.
    pub write_password: Option<String>,
}

/// Handle to the background reporter.
pub struct PlatformHandle {
    client: PlatformClient,
    session: String,
    stop: Option<oneshot::Sender<()>>,
}

impl PlatformHandle {
    /// Stop reporting and mark the session closed on the platform.
    pub async fn close(mut self) {
        if let Some(stop) = self.stop.take() {
            stop.send(()).ok();
        }
        match tokio::time::timeout(Duration::from_secs(3), self.client.close(&self.session)).await {
            Ok(Ok(())) => debug!("platform session closed"),
            Ok(Err(err)) => warn!("failed to close the platform session: {err:#}"),
            Err(_) => warn!("timed out closing the platform session"),
        }
    }
}

/// Register the session and keep the platform updated in the background.
pub fn spawn_reporter(
    client: PlatformClient,
    reg: Registration,
    registry: Arc<TerminalRegistry>,
) -> PlatformHandle {
    let (stop_tx, mut stop_rx) = oneshot::channel();
    let handle = PlatformHandle {
        client: client.clone(),
        session: reg.name.clone(),
        stop: Some(stop_tx),
    };
    tokio::spawn(async move {
        tokio::select! {
            _ = run_reporter(client, reg, registry) => {}
            _ = &mut stop_rx => {}
        }
    });
    handle
}

async fn run_reporter(client: PlatformClient, reg: Registration, registry: Arc<TerminalRegistry>) {
    let mut backoff = Duration::from_secs(2);
    loop {
        if let Err(err) = client.register(&reg).await {
            warn!("platform registration failed, retrying in {backoff:?}: {err:#}");
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(60));
            continue;
        }
        backoff = Duration::from_secs(2);
        info!(session = %reg.name, "registered with the platform");

        let mut changes = registry.subscribe();
        let mut last_sent: Option<(Vec<(u32, TerminalInfo)>, Instant)> = None;
        loop {
            let snapshot = registry.snapshot();
            let due = match &last_sent {
                None => true,
                Some((prev, at)) => *prev != snapshot || at.elapsed() >= HEARTBEAT_INTERVAL,
            };
            if due {
                match client.report(&reg.name, &snapshot).await {
                    Ok(true) => last_sent = Some((snapshot, Instant::now())),
                    Ok(false) => {
                        info!("the platform forgot this session, registering again");
                        break;
                    }
                    Err(err) => {
                        warn!("platform report failed: {err:#}");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                }
            }
            let wait = last_sent
                .as_ref()
                .map(|(_, at)| HEARTBEAT_INTERVAL.saturating_sub(at.elapsed()))
                .unwrap_or(Duration::ZERO);
            tokio::select! {
                res = changes.changed() => {
                    if res.is_err() { return; }
                    tokio::time::sleep(DEBOUNCE).await;
                }
                _ = tokio::time::sleep(wait) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_roundtrip_and_aad_binding() {
        let key = [7u8; 32];
        let sealed = seal(&key, "a", b"hello");
        assert!(sealed.starts_with("v1."));
        assert_eq!(open(&key, "a", &sealed).unwrap(), b"hello");
        assert!(open(&key, "b", &sealed).is_err(), "wrong slot must fail");
        assert!(open(&[8u8; 32], "a", &sealed).is_err(), "wrong key must fail");
    }

    #[test]
    fn token_parsing() {
        let key = URL_SAFE_NO_PAD.encode([1u8; 32]);
        let cfg = PlatformConfig {
            url: "https://p".into(),
            token: format!("smt_abc.{key}"),
            server: None,
        };
        let (auth, k) = cfg.credentials().unwrap();
        assert_eq!(auth, "smt_abc");
        assert_eq!(k, [1u8; 32]);
        let bad = PlatformConfig { url: "x".into(), token: "nope".into(), server: None };
        assert!(bad.credentials().is_err());
    }

    #[test]
    fn config_without_server_still_loads() {
        let cfg: PlatformConfig = serde_json::from_str(r#"{"url":"https://p","token":"smt_a.b"}"#).unwrap();
        assert!(cfg.server.is_none());
        let text = serde_json::to_string(&cfg).unwrap();
        assert!(!text.contains("server"), "absent server is not written: {text}");
    }

    /// Known-answer vector shared with the platform's TypeScript tests.
    #[test]
    fn interop_vector() {
        let key = [0x42u8; 32];
        let sealed = "v1.AAECAwQFBgcICQoLZ7H_t95ZgGeYK7KKa7Z8Dnk8P63mEZU";
        let plain = open(&key, "sshx-platform/v1/test", sealed).unwrap();
        assert_eq!(plain, b"bonjour");
    }
}
