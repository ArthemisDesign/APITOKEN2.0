//! Покупка ChatGPT-подписки: `codex login --device-auth` в PTY, отдаём продавцу ссылку и
//! одноразовый код, ждём завершения флоу, проверяем результат.
//!
//! **Чем это принципиально отличается от Claude.** `claude setup-token` выдаёт СТРОКУ-токен, её
//! можно положить в реестр — движок сам предъявит её как Bearer. У Codex токена, который нам
//! позволено хранить, НЕТ: `codex login` пишет auth store внутрь `CODEX_HOME`, а шлюз намеренно
//! никогда не читает и не реплеит его содержимое. Поэтому «купленная подписка» здесь — это
//! КАТАЛОГ, а не строка: бот создаёт его, проводит device-флоу, и движок подхватывает каталог
//! сканом (`CLAUDE_API_CODEX_HOMES_DIR`) без рестарта и без root.
//!
//! Отсюда инварианты этого модуля:
//! * ни один секрет из auth store не читается, не логируется и не пересылается в Telegram;
//! * незавершённая покупка не оставляет каталог в пуле — он либо аутентифицирован, либо удалён;
//! * прокси продавца — секрет: файл `proxy.url` пишется 0600 и никогда не печатается.

use anyhow::{anyhow, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Маркер завершённого логина внутри `CODEX_HOME`.
const AUTH_STORE: &str = "auth.json";
/// Персональный egress аккаунта, который читает движок.
const PROXY_FILE: &str = "proxy.url";
/// Одноразовый код живёт 15 минут — дальше ждать нечего.
const DEVICE_FLOW_TIMEOUT: Duration = Duration::from_secs(15 * 60);

struct Session {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    buf: Arc<Mutex<Vec<u8>>>,
    /// Hidden staging directory. The engine scanner ignores dot-prefixed entries.
    home: PathBuf,
    published_home: PathBuf,
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
    /// Аккаунт в пуле: auth store на месте, тип аккаунта — ChatGPT.
    Authorized { label: String, has_proxy: bool },
    /// Продавец не завершил флоу за отведённое время.
    Expired,
    /// Флоу завершился, но это не ChatGPT-подписка (например, вход по API-ключу).
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

pub fn has(chat: i64) -> bool {
    sessions().lock().unwrap().contains_key(&chat)
}

/// Прибить незавершённый флоу и НЕ оставлять полупустой каталог: движок его и так не увидит
/// (нет auth store), но мусор в пуле покупок никому не нужен.
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
fn write_secret(path: &Path, value: &str) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, value)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}
#[cfg(not(unix))]
fn write_secret(path: &Path, value: &str) -> std::io::Result<()> {
    std::fs::write(path, value)
}

fn prepare_published_home(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            // The previous authbot authenticated directly in the published directory. A restart
            // can strand a pre-auth directory there; its child died in the old service cgroup, so
            // that one migration shape is safe to remove. Never remove a symlink or auth store.
            if metadata.file_type().is_dir() && !path.join(AUTH_STORE).is_file() {
                std::fs::remove_dir_all(path).map_err(|e| {
                    anyhow!("не смог убрать незавершённый старый вход: {}", e.kind())
                })
            } else {
                Err(anyhow!("каталог такого аккаунта уже существует"))
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(anyhow!(
            "не смог проверить каталог аккаунта: {}",
            error.kind()
        )),
    }
}

/// Начать device-флоу для нового аккаунта. Возвращает ссылку и код для продавца.
pub fn start(
    chat: i64,
    label: &str,
    proxy: &str,
    codex_bin: &str,
    homes_dir: &str,
) -> Result<DeviceAuth> {
    cancel(chat);
    let slug = slug(label);
    if slug.is_empty() {
        return Err(anyhow!("не смог построить имя каталога из этого адреса"));
    }
    if !Path::new(codex_bin).is_file() {
        return Err(anyhow!("codex CLI недоступен на этом хосте"));
    }
    let published_home = Path::new(homes_dir).join(&slug);
    prepare_published_home(&published_home)?;
    // Authenticate under a hidden sibling and publish with one rename only after account type and
    // egress are final. Otherwise discovery can race auth.json appearing before proxy.url.
    let home = Path::new(homes_dir).join(format!(".{slug}.pending-{chat}"));
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
            buf: buf.clone(),
            home: home.clone(),
            published_home,
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

/// Спросить у CLI, чем именно закончился логин. Единственное, что мы читаем из профиля, —
/// эта строка статуса; сам auth store не открываем.
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
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
            text.contains("chatgpt")
        }
        Err(_) => false,
    }
}

/// Дождаться, пока продавец подтвердит вход. CLI сам опрашивает OpenAI и завершается —
/// докармливать код, как в Claude-флоу, здесь не нужно.
pub fn wait(chat: i64, codex_bin: &str) -> Outcome {
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
        // Вход по API-ключу движок всё равно отвергнет на account/read — лучше сказать об этом
        // сразу и не оставлять каталог, который никогда не заработает.
        let _ = std::fs::remove_dir_all(&s.home);
        return Outcome::NotChatgpt;
    }
    let has_proxy = !s.proxy.is_empty();
    if has_proxy {
        if let Err(e) = write_secret(&s.home.join(PROXY_FILE), &s.proxy) {
            let _ = std::fs::remove_dir_all(&s.home);
            return Outcome::Failed(format!("не смог сохранить прокси аккаунта: {}", e.kind()));
        }
    }
    if std::fs::symlink_metadata(&s.published_home).is_ok() {
        let _ = std::fs::remove_dir_all(&s.home);
        return Outcome::Failed("каталог аккаунта появился во время авторизации".into());
    }
    if let Err(e) = std::fs::rename(&s.home, &s.published_home) {
        let _ = std::fs::remove_dir_all(&s.home);
        return Outcome::Failed(format!("не смог опубликовать аккаунт в пул: {}", e.kind()));
    }
    Outcome::Authorized {
        label: s.label,
        has_proxy,
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
    fn interrupted_legacy_home_is_removed_but_an_auth_store_is_preserved() {
        let root = temp_dir();
        let home = root.join("account");
        std::fs::create_dir(&home).unwrap();
        prepare_published_home(&home).unwrap();
        assert!(!home.exists());

        std::fs::create_dir(&home).unwrap();
        std::fs::write(home.join(AUTH_STORE), "{}").unwrap();
        assert!(prepare_published_home(&home).is_err());
        assert!(home.join(AUTH_STORE).is_file());
        std::fs::remove_dir_all(root).unwrap();
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
}
