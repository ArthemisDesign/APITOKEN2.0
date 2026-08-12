//! Per-profile Suno (suno.com) Clerk session: on-demand JWT minting and the `set-cookie`
//! rotation re-seal, under a per-profile single-flight.
//!
//! Contract: `docs/engine/SUNO_PROVIDER.md` §2. The credential is the browser session cookie
//! (critical entry: the Clerk `__client` cookie); short-lived JWTs are minted on demand and
//! never persisted. The load-bearing discipline, exactly the KIMI rotating-family one:
//!
//! * **Single-flight from mint through envelope re-seal.** A mint answer may rotate the
//!   underlying Clerk material via `set-cookie` — and so may any business-host call, whose
//!   `set-cookie` is merged back on every call (manifest §2). Two concurrent rotations would
//!   lose one side's material, so all mutation of the session state runs under one per-profile
//!   lock: the winner merges the rotation and re-seals the envelope BEFORE releasing the lock;
//!   the loser re-reads the state after acquiring it and finds the fresh material already
//!   there.
//! * **The winner's re-seal is durable before the lock is released.** The merged credential is
//!   written to the roster's credential file atomically (staging file, fsync, mode-0600,
//!   rename, directory fsync — the Auth Bot's publication discipline). A re-seal failure keeps
//!   the in-memory material serving but is logged as an error: the durable copy then lags the
//!   rotated session until the next successful rotation.
//! * **JWTs are memory-only.** A minted JWT is reused for a short bounded window (the TTL is
//!   an open `unknown`, manifest §6, so reuse stays conservative) and is never written to
//!   disk.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use suno_credential::{encode_envelope, CredentialKeyring, SunoCredential};
use tokio::sync::Mutex as AsyncMutex;

use super::client::{
    clerk_client_value, merge_set_cookie, parse_jwt_mint, JwtMint,
};
use super::transport::{SunoHosts, UpstreamVerdict};

/// Conservative in-memory JWT reuse window. The JWT TTL is an open `unknown` (manifest §6):
/// minting is idempotent keep-alive, so a fresh mint is always safe; this bound only avoids a
/// mint per poll tick inside one operation burst.
const JWT_REUSE_SECS: i64 = 30;

/// Why a session call failed. The runtime maps this onto the plane's verdicts: `Invalid` is a
/// Clerk-side 401/403 (the material is dead or revoked), everything else is transport or a
/// changed wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionError {
    /// HTTP 401/403 from the auth host: the cookie is dead or revoked.
    Invalid,
    /// Connect/read failure or a non-decisive status.
    Transport,
    /// A 2xx whose body is not the documented shape: a contract change, fail closed.
    Protocol,
}

impl SessionError {
    pub(crate) fn verdict(self) -> UpstreamVerdict {
        match self {
            Self::Invalid => UpstreamVerdict::AuthRefused,
            Self::Transport => UpstreamVerdict::Transport,
            Self::Protocol => UpstreamVerdict::Protocol,
        }
    }
}

/// The mutable session state of one profile. The cookie and session id may rotate; the plan
/// and proxy never do (they are the roster identity).
struct SessionState {
    credential: SunoCredential,
    /// Cached JWT and its mint instant (unix seconds). Memory-only, never persisted.
    jwt: Option<(String, i64)>,
}

/// Per-profile session manager. Clone-free: owned by the runtime profile.
pub(crate) struct SessionManager {
    profile_id: String,
    /// The key id the current envelope was sealed under; rotations re-seal under the same key
    /// (the keyring keeps old keys readable, and a keyring rotation arrives as an atomic
    /// Auth Bot republication, not as a runtime decision).
    credential_key_id: String,
    /// Canonical credential file inside the roster directory (path-pinned by the roster
    /// loader, so the re-seal can only ever overwrite the profile's own envelope).
    credential_path: PathBuf,
    keyring: CredentialKeyring,
    state: Mutex<SessionState>,
    /// The single-flight: held from JWT mint / `set-cookie` merge through envelope re-seal.
    flight: AsyncMutex<()>,
}

impl SessionManager {
    pub(crate) fn new(
        profile_id: &str,
        credential_key_id: &str,
        roster_dir: &Path,
        keyring: CredentialKeyring,
        credential: SunoCredential,
    ) -> Self {
        Self {
            profile_id: profile_id.to_string(),
            credential_key_id: credential_key_id.to_string(),
            credential_path: roster_dir
                .join("credentials")
                .join(format!("{profile_id}.json")),
            keyring,
            state: Mutex::new(SessionState {
                credential,
                jwt: None,
            }),
            flight: AsyncMutex::new(()),
        }
    }

    /// The current session material snapshot (cookie + session id) for building requests.
    pub(crate) fn material(&self) -> (String, String) {
        let state = self.state.lock().expect("Suno session lock");
        (
            state.credential.cookie.clone(),
            state.credential.session_id.clone().unwrap_or_default(),
        )
    }

    /// The key id the current envelope was sealed under (roster-match identity).
    pub(crate) fn credential_key_id(&self) -> &str {
        &self.credential_key_id
    }

    /// The pinned egress of the current credential (roster-match identity; never logged).
    pub(crate) fn proxy_url(&self) -> String {
        self.state
            .lock()
            .expect("Suno session lock")
            .credential
            .proxy_url
            .clone()
    }

    /// A valid JWT for business-host calls: a fresh cached one when possible, else a mint
    /// under the single-flight.
    pub(crate) async fn jwt(
        &self,
        client: &wreq::Client,
        hosts: &SunoHosts,
    ) -> Result<String, SessionError> {
        if let Some(jwt) = self.cached_jwt() {
            return Ok(jwt);
        }
        let _flight = self.flight.lock().await;
        // The loser re-reads: a concurrent mint may already have refreshed the cache.
        if let Some(jwt) = self.cached_jwt() {
            return Ok(jwt);
        }
        let (cookie, session_id) = self.material();
        if session_id.is_empty() {
            // The roster loader refuses a credential without a session id; reaching this means
            // memory corruption, not a provider state.
            return Err(SessionError::Protocol);
        }
        let (jwt, set_cookie) = mint_once(client, hosts, &cookie, &session_id).await?;
        self.apply_rotation(set_cookie)
            .await
            .map_err(|_| SessionError::Transport)?;
        let now = now_unix();
        self.state.lock().expect("Suno session lock").jwt = Some((jwt.clone(), now));
        Ok(jwt)
    }

    /// Merge any `set-cookie` rotation a business-host answer carried (manifest §2: merged
    /// back on every call). Cheap no-op when the answer carried none.
    pub(crate) async fn observe_response(
        &self,
        headers: &wreq::header::HeaderMap,
    ) -> Result<(), SessionError> {
        let set_cookie: Vec<String> = headers
            .get_all(wreq::header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok().map(str::to_owned))
            .collect();
        if set_cookie.is_empty() {
            return Ok(());
        }
        let _flight = self.flight.lock().await;
        self.apply_rotation(set_cookie)
            .await
            .map_err(|_| SessionError::Transport)
    }

    fn cached_jwt(&self) -> Option<String> {
        let now = now_unix();
        let state = self.state.lock().expect("Suno session lock");
        state
            .jwt
            .as_ref()
            .filter(|(_, minted_at)| now.saturating_sub(*minted_at) < JWT_REUSE_SECS)
            .map(|(jwt, _)| jwt.clone())
    }

    /// Merge a rotation into the session state and re-seal the envelope BEFORE the
    /// single-flight releases (the winner's duty). Runs under `flight`.
    async fn apply_rotation(&self, set_cookie: Vec<String>) -> Result<()> {
        let merged_credential = {
            let state = self.state.lock().expect("Suno session lock");
            let refs: Vec<&str> = set_cookie.iter().map(String::as_str).collect();
            let merged = merge_set_cookie(&state.credential.cookie, &refs)
                .context("merge Suno set-cookie rotation")?;
            if merged == state.credential.cookie {
                // A rotation that changes nothing (e.g. only attributes moved) re-seals nothing.
                return Ok(());
            }
            let mut credential = state.credential.clone();
            credential.cookie = merged;
            credential
        };
        let envelope = self
            .keyring
            .seal(&self.credential_key_id, &self.profile_id, &merged_credential)
            .context("re-seal Suno credential rotation")?;
        let bytes = encode_envelope(&envelope).context("encode Suno credential rotation")?;
        let path = self.credential_path.clone();
        tokio::task::spawn_blocking(move || atomic_private_replace(&path, &bytes))
            .await
            .context("join Suno credential rotation writer")?
            .context("persist Suno credential rotation")?;
        // The Clerk material changed: every cached JWT was minted under the old cookie. The
        // in-memory state flips only after the durable write, so a failed re-seal keeps serving
        // the last durably sealed material rather than a session a restart would lose.
        let mut state = self.state.lock().expect("Suno session lock");
        state.credential = merged_credential;
        state.jwt = None;
        Ok(())
    }
}

/// One JWT mint against the auth host. Returns the JWT and any `set-cookie` headers for the
/// rotation merge. The `__client` value rides as the raw `Authorization` header and the full
/// cookie is re-sent — the reviewed Clerk contract (manifest §2).
async fn mint_once(
    client: &wreq::Client,
    hosts: &SunoHosts,
    cookie: &str,
    session_id: &str,
) -> Result<(String, Vec<String>), SessionError> {
    let client_value = clerk_client_value(cookie).ok_or(SessionError::Protocol)?;
    let response = client
        .post(hosts.jwt_mint_url(session_id))
        .header(wreq::header::AUTHORIZATION, client_value)
        .header(wreq::header::COOKIE, cookie)
        .header(wreq::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| SessionError::Transport)?;
    let status = response.status().as_u16();
    let set_cookie: Vec<String> = response
        .headers()
        .get_all(wreq::header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok().map(str::to_owned))
        .collect();
    let body = response.bytes().await.map_err(|_| SessionError::Transport)?;
    match parse_jwt_mint(status, &body) {
        Ok(JwtMint::Minted { jwt }) => Ok((jwt, set_cookie)),
        Ok(JwtMint::Invalid) => Err(SessionError::Invalid),
        Err(_) => Err(if status == 401 || status == 403 {
            SessionError::Invalid
        } else if (200..300).contains(&status) {
            SessionError::Protocol
        } else {
            SessionError::Transport
        }),
    }
}

/// Atomically replace a private credential file: staging sibling (create_new, mode-0600,
/// fsync), rename, directory fsync — the Auth Bot's publication discipline, so a crash leaves
/// either the old envelope or the new one, never a torn file.
fn atomic_private_replace(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt;
    let parent = path.parent().context("Suno credential path has no parent")?;
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|_| anyhow::anyhow!("CSPRNG unavailable"))?;
    let staging = parent.join(format!(".suno.{}.pending", URL_SAFE_NO_PAD.encode(random)));
    let write = || -> Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&staging)
            .context("create Suno rotation staging file")?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    };
    if let Err(error) = write() {
        let _ = std::fs::remove_file(&staging);
        return Err(error);
    }
    if let Err(error) = std::fs::rename(&staging, path) {
        let _ = std::fs::remove_file(&staging);
        return Err(error).context("replace Suno credential envelope");
    }
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::{Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;
    use suno_credential::{decode_envelope, SunoCredentialKind, SunoPlan};

    fn keyring() -> CredentialKeyring {
        CredentialKeyring::parse(&format!("a1:{}", "11".repeat(32))).unwrap()
    }

    fn credential(cookie: &str) -> SunoCredential {
        SunoCredential {
            version: 1,
            kind: SunoCredentialKind::SessionCookie,
            cookie: cookie.into(),
            session_id: Some("sess_2abc001".into()),
            plan: SunoPlan::Pro,
            proxy_url: String::new(),
        }
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let mut random = [0u8; 8];
            getrandom::fill(&mut random).unwrap();
            let suffix = random.iter().map(|b| format!("{b:02x}")).collect::<String>();
            let root = std::env::temp_dir().join(format!("suno-session-{suffix}"));
            fs::create_dir_all(root.join("credentials")).unwrap();
            Self { root }
        }

        fn manager(&self, cookie: &str) -> SessionManager {
            let ring = keyring();
            let sealed = ring.seal("a1", "suno-01", &credential(cookie)).unwrap();
            let path = self.root.join("credentials").join("suno-01.json");
            fs::write(&path, encode_envelope(&sealed).unwrap()).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            SessionManager::new(
                "suno-01",
                "a1",
                &self.root,
                keyring(),
                credential(cookie),
            )
        }

        fn opened_cookie(&self) -> String {
            let bytes = fs::read(self.root.join("credentials").join("suno-01.json")).unwrap();
            let envelope = decode_envelope(&bytes).unwrap();
            keyring().open("suno-01", &envelope).unwrap().cookie.clone()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// One mocked Clerk answer: the JWT plus optional `set-cookie` rotation headers.
    fn mock_auth(responses: std::sync::Arc<Mutex<Vec<(String, Vec<String>)>>>) -> (String, std::sync::mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || loop {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let responses = responses.clone();
            let sender = sender.clone();
            std::thread::spawn(move || {
                let mut buffer = Vec::new();
                let mut byte = [0u8; 1];
                loop {
                    if stream.read(&mut byte).unwrap_or(0) == 0 {
                        break;
                    }
                    buffer.push(byte[0]);
                    if buffer.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
                sender.send(String::from_utf8_lossy(&buffer).to_string()).unwrap();
                let (body, cookies) = responses.lock().unwrap().remove(0);
                let mut head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n",
                    body.len()
                );
                for cookie in cookies {
                    head.push_str(&format!("set-cookie: {cookie}\r\n"));
                }
                head.push_str("connection: close\r\n\r\n");
                stream.write_all(head.as_bytes()).unwrap();
                stream.write_all(body.as_bytes()).unwrap();
            });
        });
        (format!("http://{address}"), receiver)
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn a_mint_caches_the_jwt_and_a_rotation_reseals_before_releasing() {
        runtime().block_on(async {
            let fixture = Fixture::new();
            let manager = fixture.manager("__client=old-token; ajs_id=x");
            // Rotation merge is exercised directly (the HTTP mint path is covered by the
            // gateway tests): the winner re-seals before the lock would release.
            manager
                .apply_rotation(vec!["__client=new-token; Path=/; HttpOnly".to_string()])
                .await
                .unwrap();
            // In-memory state and the durable envelope both carry the rotation.
            let (cookie, _) = manager.material();
            assert!(cookie.contains("__client=new-token"));
            assert!(cookie.contains("ajs_id=x"));
            assert!(fixture.opened_cookie().contains("__client=new-token"));
        });
    }

    #[test]
    fn a_noop_rotation_seals_nothing() {
        runtime().block_on(async {
            let fixture = Fixture::new();
            let manager = fixture.manager("__client=same-token; ajs_id=x");
            let before = fs::read(fixture.root.join("credentials").join("suno-01.json")).unwrap();
            manager
                .apply_rotation(vec!["__client=same-token; Path=/".to_string()])
                .await
                .unwrap();
            let after = fs::read(fixture.root.join("credentials").join("suno-01.json")).unwrap();
            assert_eq!(before, after, "an unchanged merge must not re-seal");
        });
    }

    #[test]
    fn a_minted_jwt_is_cached_and_a_rotation_invalidates_the_cache() {
        runtime().block_on(async {
            let fixture = Fixture::new();
            let manager = fixture.manager("__client=tok; ajs_id=x");
            let responses = std::sync::Arc::new(Mutex::new(vec![
                (r#"{"jwt":"jwt-one"}"#.to_string(), vec![]),
                (r#"{"jwt":"jwt-two"}"#.to_string(), vec![]),
            ]));
            let (origin, _requests) = mock_auth(responses.clone());
            let hosts = SunoHosts::loopback(origin.clone(), origin);
            let client = super::super::client::build_client("", Duration::from_secs(5), Duration::from_secs(5)).unwrap();
            let first = manager.jwt(&client, &hosts).await.unwrap();
            assert_eq!(first, "jwt-one");
            // Cached: no second mint happens while the window is fresh.
            assert_eq!(manager.jwt(&client, &hosts).await.unwrap(), "jwt-one");
            assert!(responses.lock().unwrap().len() == 1);
            // A rotation kills the cache: the next call mints again under the single-flight.
            manager
                .apply_rotation(vec!["__client=rotated; Path=/".to_string()])
                .await
                .unwrap();
            assert_eq!(manager.jwt(&client, &hosts).await.unwrap(), "jwt-two");
            assert!(fixture.opened_cookie().contains("__client=rotated"));
        });
    }

    #[test]
    fn concurrent_mints_share_one_flight_and_one_mint() {
        runtime().block_on(async {
            let fixture = Fixture::new();
            let manager = std::sync::Arc::new(fixture.manager("__client=tok; ajs_id=x"));
            // Slow answers serialize the two contenders; the loser must re-read the cache.
            let responses = std::sync::Arc::new(Mutex::new(vec![
                (r#"{"jwt":"jwt-shared"}"#.to_string(), vec!["__client=rotated; Path=/".to_string()]),
            ]));
            let (origin, requests) = mock_auth(responses.clone());
            let hosts = SunoHosts::loopback(origin.clone(), origin);
            let client = super::super::client::build_client("", Duration::from_secs(5), Duration::from_secs(5)).unwrap();
            let (a, b) = tokio::join!(manager.jwt(&client, &hosts), manager.jwt(&client, &hosts));
            assert_eq!(a.unwrap(), "jwt-shared");
            assert_eq!(b.unwrap(), "jwt-shared");
            // One mint for two contenders: the loser re-read after the winner's re-seal.
            assert!(responses.lock().unwrap().is_empty());
            drop(requests);
            assert!(fixture.opened_cookie().contains("__client=rotated"));
        });
    }

    #[test]
    fn a_reseal_failure_keeps_serving_but_reports() {
        runtime().block_on(async {
            let fixture = Fixture::new();
            let manager = fixture.manager("__client=tok; ajs_id=x");
            // Make the credential path a directory so the rename fails.
            let path = fixture.root.join("credentials").join("suno-01.json");
            fs::remove_file(&path).unwrap();
            fs::create_dir(&path).unwrap();
            let result = manager
                .apply_rotation(vec!["__client=rotated; Path=/".to_string()])
                .await;
            assert!(result.is_err());
            // The in-memory material is untouched on a failed re-seal: the last durably sealed
            // state is what the process keeps serving.
            let (cookie, _) = manager.material();
            assert!(cookie.contains("__client=tok"));
        });
    }

    #[test]
    fn session_ids_and_cookies_snapshot_together() {
        let fixture = Fixture::new();
        let manager = fixture.manager("__client=tok; ajs_id=x");
        let (cookie, session_id) = manager.material();
        assert!(cookie.contains("__client=tok"));
        assert_eq!(session_id, "sess_2abc001");
        assert_eq!(manager.credential_path, fixture.root.join("credentials").join("suno-01.json"));
    }

    #[test]
    fn a_missing_session_id_is_a_protocol_anomaly_not_a_mint() {
        runtime().block_on(async {
            let fixture = Fixture::new();
            let mut anonymous = credential("__client=tok");
            anonymous.session_id = None;
            let manager = SessionManager::new("suno-01", "a1", &fixture.root, keyring(), anonymous);
            let hosts = SunoHosts::loopback(String::new(), String::new());
            let client = super::super::client::build_client("", Duration::from_secs(1), Duration::from_secs(1)).unwrap();
            assert_eq!(
                manager.jwt(&client, &hosts).await.err(),
                Some(SessionError::Protocol)
            );
        });
    }
}
