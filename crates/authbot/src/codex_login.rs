//! Покупка ChatGPT-подписки: `codex login --device-auth` в PTY, отдаём продавцу ссылку и
//! одноразовый код, ждём завершения флоу, запечатываем результат в encrypted roster.
//!
//! **Модель доверия (та же, что у Gemini).** Логин выполняет ТОЛЬКО официальный клиент в PTY —
//! бот не знает пароля и не видит второй фактор. По завершении `codex login` пишет auth store в
//! скрытый staging-каталог; бот ОДИН РАЗ читает из него OAuth-материал, запечатывает его в AEAD
//! envelope (`codex-credential`) в roster движка и ПОЛНОСТЬЮ удаляет staging. После этого
//! открытого токена не существует ни на диске, ни в Telegram, ни в логах.
//!
//! Отсюда инварианты этого модуля:
//! * ни один секрет не логируется и не пересылается в Telegram;
//! * незавершённая покупка не оставляет профиль в пуле — он либо запечатан в roster, либо удалён;
//! * прокси продавца — секрет: он существует только внутри envelope;
//! * roster обновляется атомарно (tmp+rename): движок никогда не читает половину файла.

use anyhow::{anyhow, Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Маркер завершённого логина внутри staging-каталога.
const AUTH_STORE: &str = "auth.json";
/// Одноразовый код живёт 15 минут — дальше ждать нечего.
const DEVICE_FLOW_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Конфигурация encrypted roster, в который запечатываются купленные аккаунты.
#[derive(Clone)]
pub struct RosterConfig {
    /// Корень roster: `<dir>/profiles.json` + `<dir>/credentials/<id>.json`.
    pub dir: PathBuf,
    pub keyring: codex_credential::CredentialKeyring,
    pub active_key_id: String,
}

struct Session {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    /// Hidden staging directory. The roster never points at it.
    home: PathBuf,
    proxy: String,
    label: String,
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

struct StagingDir(Option<PathBuf>);

impl StagingDir {
    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for StagingDir {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

fn sessions() -> &'static Mutex<HashMap<i64, Session>> {
    static S: OnceLock<Mutex<HashMap<i64, Session>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Что показать продавцу: ссылка + одноразовый код.
pub struct DeviceAuth {
    pub url: String,
    pub code: String,
}

pub enum Outcome {
    /// Аккаунт в пуле: envelope в roster, staging удалён.
    Authorized { label: String, has_proxy: bool },
    /// Продавец не завершил флоу за отведённое время.
    Expired,
    /// Флоу завершился, но это не ChatGPT-подписка (например, вход по API-ключу или free-план).
    NotChatgpt,
    Failed(String),
}

fn slug(label: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in label.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn buf_string(buf: &Arc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8_lossy(&buf.lock().unwrap()).to_string()
}

/// Убрать ANSI-раскраску: codex печатает ссылку и код в цвете, внутри escape-последовательностей.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    out
}

/// Ссылка device-флоу. Codex печатает фиксированный адрес, но берём его из вывода, а не из
/// константы: если пиннованная версия сменит адрес, лучше отдать продавцу реальный, чем устаревший.
pub(crate) fn scan_url(s: &str) -> Option<String> {
    let plain = strip_ansi(s);
    let start = plain.find("https://auth.openai.com/")?;
    let tail = &plain[start..];
    let end = tail
        .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '<')
        .unwrap_or(tail.len());
    Some(tail[..end].to_string())
}

/// Одноразовый код вида `Q550-5VVAF`.
pub(crate) fn scan_code(s: &str) -> Option<String> {
    let plain = strip_ansi(s);
    for line in plain.lines() {
        let token = line.trim();
        let Some((left, right)) = token.split_once('-') else {
            continue;
        };
        let ok = (4..=6).contains(&left.len())
            && (4..=6).contains(&right.len())
            && left.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            && right.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            && token.len() == left.len() + right.len() + 1;
        if ok {
            return Some(token.to_string());
        }
    }
    None
}

/// Прибить незавершённый флоу и НЕ оставлять полупустой каталог.
pub fn cancel(chat: i64) {
    if let Some(mut s) = sessions().lock().unwrap().remove(&chat) {
        let _ = s.child.kill();
        let _ = s.child.wait();
        let _ = std::fs::remove_dir_all(&s.home);
    }
}

#[cfg(unix)]
fn secure_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}
#[cfg(not(unix))]
fn secure_dir(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn write_secret(path: &Path, value: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, value)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}
#[cfg(not(unix))]
fn write_secret(path: &Path, value: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, value)
}

/// Атомарная публикация файла (tmp + rename): читатель никогда не видит половину записи.
fn publish(path: &Path, bytes: &[u8], secret: bool) -> Result<()> {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    if secret {
        write_secret(&tmp, bytes)?;
    } else {
        std::fs::write(&tmp, bytes)?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Начать device-флоу для нового аккаунта. Возвращает ссылку и код для продавца.
pub fn start(chat: i64, label: &str, proxy: &str, codex_bin: &str, staging_dir: &str) -> Result<DeviceAuth> {
    cancel(chat);
    let slug = slug(label);
    if slug.is_empty() {
        return Err(anyhow!("не смог построить имя каталога из этого адреса"));
    }
    if !Path::new(codex_bin).is_file() {
        return Err(anyhow!("codex CLI недоступен на этом хосте"));
    }
    // Authenticate under a hidden directory; the roster only learns about the account once its
    // credential is sealed. Nothing here is ever scanned by the engine.
    let home = Path::new(staging_dir).join(format!(".{slug}.pending-{chat}"));
    let _ = std::fs::remove_dir_all(&home);
    let mut staging = StagingDir(Some(home.clone()));
    std::fs::create_dir_all(&home)
        .map_err(|e| anyhow!("не смог создать каталог аккаунта: {}", e.kind()))?;
    secure_dir(&home).map_err(|e| anyhow!("не смог закрыть права каталога: {}", e.kind()))?;

    let pty = native_pty_system();
    let pair = pty.openpty(PtySize {
        rows: 50,
        cols: 200,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut cmd = CommandBuilder::new(codex_bin);
    cmd.env_clear();
    cmd.arg("login");
    cmd.arg("--device-auth");
    // Ребёнок не наследует окружение бота: только то, что нужно этому логину.
    cmd.env("CODEX_HOME", &home);
    cmd.env("HOME", &home);
    cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLUMNS", "200");
    cmd.env("LINES", "50");
    if !proxy.trim().is_empty() {
        // Логин уходит через тот же egress, что и будущий трафик аккаунта: иначе покупка
        // и эксплуатация выглядят как два разных пользователя.
        for name in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
            cmd.env(name, proxy.trim());
        }
    }

    let mut child = pair.slave.spawn_command(cmd)?;
    drop(pair.slave);
    let mut reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.into());
        }
    };
    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let buf2 = buf.clone();
    std::thread::spawn(move || {
        let mut tmp = [0u8; 4096];
        loop {
            match reader.read(&mut tmp) {
                Ok(0) | Err(_) => break,
                Ok(n) => buf2.lock().unwrap().extend_from_slice(&tmp[..n]),
            }
        }
    });

    sessions().lock().unwrap().insert(
        chat,
        Session {
            child,
            home: home.clone(),
            proxy: proxy.trim().to_string(),
            label: label.to_string(),
            _master: pair.master,
        },
    );
    staging.disarm();

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let text = buf_string(&buf);
        if let (Some(url), Some(code)) = (scan_url(&text), scan_code(&text)) {
            return Ok(DeviceAuth { url, code });
        }
        if Instant::now() > deadline {
            cancel(chat);
            return Err(anyhow!("codex не выдал ссылку авторизации вовремя"));
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

/// Спросить у CLI, чем именно закончился логин. Строка статуса — официальная проверка типа
/// аккаунта до того, как мы вообще прикоснёмся к auth store.
fn status_reports_chatgpt(success: bool, stdout: &[u8], stderr: &[u8]) -> bool {
    success
        && [stdout, stderr].iter().any(|stream| {
            String::from_utf8_lossy(stream)
                .to_lowercase()
                .contains("logged in using chatgpt")
        })
}

fn is_chatgpt_login(codex_bin: &str, home: &Path) -> bool {
    let mut command = std::process::Command::new(codex_bin);
    command
        .env_clear()
        .arg("login")
        .arg("status")
        .env("CODEX_HOME", home)
        .env("HOME", home)
        .env("PATH", "/usr/local/bin:/usr/bin:/bin");
    let out = command.output();
    match out {
        // Pinned Codex writes login status to stderr; older releases used stdout. Accept either
        // stream, but only when the command itself succeeded.
        Ok(out) => status_reports_chatgpt(out.status.success(), &out.stdout, &out.stderr),
        Err(_) => false,
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// Распарсить JWT payload без проверки подписи: токен только что записан официальным клиентом в
/// локальный файл, нам нужны его claims (account id, план, email, expiry).
fn jwt_claims(token: &str) -> Option<serde_json::Value> {
    let payload = token.split('.').nth(1)?;
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Извлечь из auth store официального клиента ровно тот материал, который войдёт в envelope.
/// Это единственная точка модуля, где открытый токен существует в памяти; по выходу из неё
/// staging удаляется.
fn credential_from_auth_store(home: &Path, proxy: &str) -> Result<(String, codex_credential::CodexCredential)> {
    let raw = std::fs::read_to_string(home.join(AUTH_STORE))
        .context("прочитать auth store")?;
    let raw = zeroize::Zeroizing::new(raw);
    let store: serde_json::Value = serde_json::from_str(&raw).context("разобрать auth store")?;
    let tokens = store
        .get("tokens")
        .cloned()
        .ok_or_else(|| anyhow!("в auth store нет tokens"))?;
    let get = |name: &str| -> Result<String> {
        tokens
            .get(name)
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("в auth store нет {name}"))
    };
    let access_token = get("access_token")?;
    let refresh_token = get("refresh_token")?;
    let id_token = get("id_token")?;
    let id_claims = jwt_claims(&id_token).unwrap_or(serde_json::Value::Null);
    let auth_claims = id_claims
        .get("https://api.openai.com/auth")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let account_id = tokens
        .get("account_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            auth_claims
                .get("chatgpt_account_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .ok_or_else(|| anyhow!("не смог определить account id аккаунта"))?;
    let plan_claim = auth_claims
        .get("chatgpt_plan_type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let plan = codex_credential::supported_plan_for_claim(plan_claim)
        .ok_or_else(|| anyhow!("план {plan_claim:?} не является платной подпиской ChatGPT"))?
        .to_string();
    let email = id_claims
        .get("email")
        .and_then(serde_json::Value::as_str)
        .filter(|value| value.len() >= 3)
        .map(str::to_string)
        .unwrap_or_else(|| "unknown@example.com".to_string());
    let expires_at = jwt_claims(&access_token)
        .and_then(|claims| claims.get("exp").and_then(serde_json::Value::as_i64))
        .unwrap_or_else(|| now() + 3_600);
    let profile_id = slug(&account_id);
    let credential = codex_credential::CodexCredential {
        version: 1,
        access_token,
        refresh_token,
        expires_at,
        oauth_client_id: codex_credential::CODEX_OFFICIAL_OAUTH_CLIENT_ID.to_string(),
        token_uri: codex_credential::CODEX_OFFICIAL_TOKEN_URI.to_string(),
        account_id,
        email,
        plan,
        proxy: proxy.to_string(),
        proxy_order_id: 0,
        issued_at: now(),
    };
    Ok((profile_id, credential))
}

/// Запечатать credential в roster и атомарно обновить `profiles.json`.
fn publish_credential(
    roster: &RosterConfig,
    profile_id: &str,
    credential: &codex_credential::CodexCredential,
) -> Result<()> {
    codex_credential::validate_profile_id(profile_id)?;
    let credentials_dir = roster.dir.join("credentials");
    std::fs::create_dir_all(&credentials_dir)?;
    secure_dir(&credentials_dir)?;
    let envelope = roster
        .keyring
        .seal(&roster.active_key_id, profile_id, credential)
        .context("запечатать credential")?;
    let encoded = codex_credential::encode_envelope(&envelope)?;
    let credential_file = credentials_dir.join(format!("{profile_id}.json"));
    publish(&credential_file, &encoded, true)?;

    // Republish the roster with this profile present (replace on re-login of the same account).
    let profiles_path = roster.dir.join("profiles.json");
    let mut profiles: Vec<serde_json::Value> = std::fs::read(&profiles_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| value.get("profiles")?.as_array().cloned())
        .unwrap_or_default();
    let credential_file = credential_file.to_string_lossy().to_string();
    profiles.retain(|entry| entry.get("id").and_then(serde_json::Value::as_str) != Some(profile_id));
    profiles.push(serde_json::json!({
        "id": profile_id,
        "credential_file": credential_file,
    }));
    let document = serde_json::to_vec(&serde_json::json!({ "profiles": profiles }))?;
    publish(&profiles_path, &document, false)?;
    Ok(())
}

/// Дождаться, пока продавец подтвердит вход. CLI сам опрашивает OpenAI и завершается —
/// докармливать код, как в Claude-флоу, здесь не нужно.
pub fn wait(chat: i64, codex_bin: &str, roster: &RosterConfig) -> Outcome {
    let deadline = Instant::now() + DEVICE_FLOW_TIMEOUT;
    loop {
        let finished = {
            let mut guard = sessions().lock().unwrap();
            let Some(s) = guard.get_mut(&chat) else {
                return Outcome::Failed("сессия потеряна".into());
            };
            match s.child.try_wait() {
                Ok(Some(_)) => Ok(true),
                Ok(None) => Ok(false),
                Err(e) => Err(e.kind()),
            }
        };
        let finished = match finished {
            Ok(finished) => finished,
            Err(kind) => {
                cancel(chat);
                return Outcome::Failed(format!("{kind}"));
            }
        };
        if finished {
            break;
        }
        if Instant::now() > deadline {
            cancel(chat);
            return Outcome::Expired;
        }
        std::thread::sleep(Duration::from_millis(700));
    }

    let Some(s) = sessions().lock().unwrap().remove(&chat) else {
        return Outcome::Failed("сессия потеряна".into());
    };
    if !s.home.join(AUTH_STORE).is_file() {
        let _ = std::fs::remove_dir_all(&s.home);
        return Outcome::Expired;
    }
    if !is_chatgpt_login(codex_bin, &s.home) {
        // Вход по API-ключу движок всё равно отвергнет — лучше сказать об этом сразу и не
        // оставлять материал, который никогда не заработает.
        let _ = std::fs::remove_dir_all(&s.home);
        return Outcome::NotChatgpt;
    }
    let has_proxy = !s.proxy.is_empty();
    let outcome = (|| {
        let (profile_id, credential) = credential_from_auth_store(&s.home, &s.proxy)?;
        publish_credential(roster, &profile_id, &credential)
    })();
    // С этого момента открытый auth store не нужен ни при каком исходе.
    let _ = std::fs::remove_dir_all(&s.home);
    match outcome {
        Ok(()) => Outcome::Authorized {
            label: s.label,
            has_proxy,
        },
        Err(error) => {
            let message = error.to_string();
            if message.contains("платной подпиской") {
                Outcome::NotChatgpt
            } else {
                Outcome::Failed(message)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "authbot-codex-login-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }

    /// Реальный вывод пиннованного codex 0.145.0 (с раскраской).
    const SAMPLE: &str = "Welcome to Codex [v\u{1b}[90m0.145.0\u{1b}[0m]\n\n\
Follow these steps to sign in with ChatGPT using device code authorization:\n\n\
1. Open this link in your browser and sign in to your account\n   \
\u{1b}[94mhttps://auth.openai.com/codex/device\u{1b}[0m\n\n\
2. Enter this one-time code \u{1b}[90m(expires in 15 minutes)\u{1b}[0m\n   \
\u{1b}[94mQ550-5VVAF\u{1b}[0m\n";

    #[test]
    fn reads_the_link_and_code_out_of_coloured_output() {
        assert_eq!(
            scan_url(SAMPLE).as_deref(),
            Some("https://auth.openai.com/codex/device")
        );
        assert_eq!(scan_code(SAMPLE).as_deref(), Some("Q550-5VVAF"));
    }

    /// Второй живой прогон пиннованного CLI: код той же формы, но другой длины слева/справа.
    /// Формат кода — контракт с провайдером, поэтому фиксируем оба наблюдения.
    #[test]
    fn reads_a_second_real_code_of_a_different_shape() {
        let live = "2. Enter this one-time code \u{1b}[90m(expires in 15 minutes)\u{1b}[0m\n   \u{1b}[94mQ5U5-TR80K\u{1b}[0m\n";
        assert_eq!(scan_code(live).as_deref(), Some("Q5U5-TR80K"));
    }

    #[test]
    fn waits_instead_of_reporting_a_half_written_screen() {
        let partial = "Follow these steps to sign in with ChatGPT using device code authorization:\n";
        assert!(scan_url(partial).is_none());
        assert!(scan_code(partial).is_none());
    }

    #[test]
    fn version_and_prose_are_not_mistaken_for_a_code() {
        assert!(scan_code("Welcome to Codex [v0.145.0]").is_none());
        assert!(scan_code("expires in 15 minutes").is_none());
        assert!(scan_code("2026-07-25").is_none());
    }

    #[test]
    fn account_slug_is_filesystem_safe() {
        assert_eq!(slug("Seller.One+tag@Example.COM"), "seller-one-tag-example-com");
        assert_eq!(slug("../../etc/passwd"), "etc-passwd");
        assert!(slug("!!!").is_empty());
    }

    #[test]
    fn accepts_chatgpt_status_from_pinned_codex_stderr() {
        assert!(status_reports_chatgpt(
            true,
            b"",
            b"Logged in using ChatGPT\n"
        ));
    }

    #[test]
    fn accepts_legacy_chatgpt_status_from_stdout() {
        assert!(status_reports_chatgpt(
            true,
            b"Logged in using ChatGPT\n",
            b""
        ));
    }

    #[test]
    fn rejects_api_key_and_failed_status_checks() {
        assert!(!status_reports_chatgpt(
            true,
            b"",
            b"Logged in using an API key - sk-proj-***\n"
        ));
        assert!(!status_reports_chatgpt(
            false,
            b"",
            b"Logged in using ChatGPT\n"
        ));
    }

    #[test]
    fn staging_guard_removes_failures_and_preserves_published_state() {
        let root = temp_dir();
        let failed = root.join("failed");
        std::fs::create_dir(&failed).unwrap();
        drop(StagingDir(Some(failed.clone())));
        assert!(!failed.exists());

        let published = root.join("published");
        std::fs::create_dir(&published).unwrap();
        let mut guard = StagingDir(Some(published.clone()));
        guard.disarm();
        drop(guard);
        assert!(published.is_dir());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn fake_jwt(claims: serde_json::Value) -> String {
        use base64::Engine as _;
        let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        format!(
            "{}.{}.sig",
            engine.encode(b"{}"),
            engine.encode(serde_json::to_vec(&claims).unwrap())
        )
    }

    #[test]
    fn auth_store_becomes_a_sealed_roster_profile() {
        let root = temp_dir();
        let home = root.join("staging");
        std::fs::create_dir(&home).unwrap();
        let id_token = fake_jwt(serde_json::json!({
            "email": "owner@example.com",
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "acct_test_0001",
                "chatgpt_plan_type": "plus"
            }
        }));
        let access_token = fake_jwt(serde_json::json!({"exp": 4_102_444_800i64}));
        std::fs::write(
            home.join(AUTH_STORE),
            serde_json::to_vec(&serde_json::json!({
                "tokens": {
                    "id_token": id_token,
                    "access_token": access_token,
                    "refresh_token": "refresh-material",
                    "account_id": "acct_test_0001"
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let (profile_id, credential) =
            credential_from_auth_store(&home, "http://user:pass@127.0.0.1:8080").unwrap();
        assert_eq!(profile_id, "acct-test-0001");
        assert_eq!(credential.plan, "chatgpt_plus");
        assert_eq!(credential.account_id, "acct_test_0001");
        assert_eq!(credential.email, "owner@example.com");
        assert_eq!(credential.expires_at, 4_102_444_800);
        credential.validate().unwrap();

        let roster = RosterConfig {
            dir: root.join("roster"),
            keyring: codex_credential::CredentialKeyring::parse(&format!(
                "current:{}",
                "ef".repeat(32)
            ))
            .unwrap(),
            active_key_id: "current".to_string(),
        };
        publish_credential(&roster, &profile_id, &credential).unwrap();
        let sealed = std::fs::read(roster.dir.join("credentials/acct-test-0001.json")).unwrap();
        assert!(!String::from_utf8_lossy(&sealed).contains("refresh-material"));
        let envelope = codex_credential::decode_envelope(&sealed).unwrap();
        let opened = roster.keyring.open(&profile_id, &envelope).unwrap();
        assert_eq!(opened.refresh_token, "refresh-material");
        let profiles: serde_json::Value = serde_json::from_slice(
            &std::fs::read(roster.dir.join("profiles.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(profiles["profiles"][0]["id"], "acct-test-0001");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn free_plan_is_not_a_paid_subscription() {
        let root = temp_dir();
        let home = root.join("staging");
        std::fs::create_dir(&home).unwrap();
        let id_token = fake_jwt(serde_json::json!({
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "acct_free_0001",
                "chatgpt_plan_type": "free"
            }
        }));
        std::fs::write(
            home.join(AUTH_STORE),
            serde_json::to_vec(&serde_json::json!({
                "tokens": {
                    "id_token": id_token,
                    "access_token": fake_jwt(serde_json::json!({"exp": 4_102_444_800i64})),
                    "refresh_token": "refresh-material",
                    "account_id": "acct_free_0001"
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let error = credential_from_auth_store(&home, "").unwrap_err();
        assert!(error.to_string().contains("платной подпиской"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
