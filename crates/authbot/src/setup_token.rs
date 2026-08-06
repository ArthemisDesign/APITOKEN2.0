//! Выпуск 1-летнего токена: гоним `claude setup-token` в PTY, ловим OAuth-URL, кормим
//! `code#state`, извлекаем токен `sk-ant-oat01-…`. Порт логики Python-бота.
//!
//! Сессия живёт МЕЖДУ сообщениями (email → URL, потом code#state → токен), поэтому держим
//! её в памяти (живой процесс — в SQLite не положишь). Ключ — chat_id.

use crate::db::SellerJobRef;
use anyhow::{anyhow, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

struct Session {
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    buf: Arc<Mutex<Vec<u8>>>,
    pub email: String,
    pub proxy: String,
    pub proxy_order_id: i64,
    pub job: Option<SellerJobRef>,
    // локальный http→socks5 мост (gost), если прокси SOCKS — claude CLI умеет только CONNECT
    bridge: Option<std::process::Child>,
    // держим master живым, пока сессия открыта
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

fn sessions() -> &'static Mutex<HashMap<i64, Session>> {
    static S: OnceLock<Mutex<HashMap<i64, Session>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Каталог PATH-шима с no-op open/xdg-open — чтобы setup-token НЕ открывал браузер на сервере.
fn shim_dir() -> String {
    let dir = "/tmp/authbot_shim".to_string();
    let _ = std::fs::create_dir_all(&dir);
    for n in ["open", "xdg-open"] {
        let p = format!("{dir}/{n}");
        if std::fs::write(&p, "#!/bin/sh\nexit 0\n").is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755));
            }
        }
    }
    dir
}

fn slug(email: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in email.to_lowercase().chars() {
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

fn account_config_dir(root: &Path, email: &str) -> PathBuf {
    root.join(format!(".claude-bot-{}", slug(email)))
}

fn prepare_config_dir(root: &str, email: &str) -> Result<PathBuf> {
    let path = account_config_dir(Path::new(root), email);
    std::fs::create_dir_all(&path)
        .map_err(|e| anyhow!("не смог создать каталог Claude-сессии: {}", e.kind()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| anyhow!("не смог закрыть права каталога Claude-сессии: {}", e.kind()))?;
    }
    Ok(path)
}

fn buf_string(buf: &Arc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8_lossy(&buf.lock().unwrap()).to_string()
}

/// Найти OAuth-URL в выводе (даже внутри OSC-гиперссылки). Терминатор — пробел/ESC/BEL/кавычка.
fn scan_url(s: &str) -> Option<String> {
    let start = s.find("https://claude.com/")?;
    let tail = &s[start..];
    if !tail.contains("oauth/authorize") {
        return None;
    }
    let end = tail
        .find(|c: char| {
            c.is_whitespace() || c == '\u{1b}' || c == '\u{7}' || c == '"' || c == '\'' || c == '<'
        })
        .unwrap_or(tail.len());
    let url = &tail[..end];
    if url.contains("oauth/authorize") {
        Some(url.to_string())
    } else {
        None
    }
}

/// Извлечь токен sk-ant-oat01-… (трейлинг из [A-Za-z0-9_-]).
fn scan_token(s: &str) -> Option<String> {
    let start = s.find("sk-ant-oat01-")?;
    let tail = &s[start..];
    let end = tail
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .unwrap_or(tail.len());
    let tok = &tail[..end];
    if tok.len() >= 30 {
        Some(tok.to_string())
    } else {
        None
    }
}

fn error_class(s: &str) -> Option<&'static str> {
    let lower = s.to_lowercase();
    if lower.contains("oauth error") {
        Some("oauth_error")
    } else if lower.contains("request failed with status code") {
        Some("http_error")
    } else if lower.contains("expired") {
        Some("expired")
    } else if lower.contains("invalid") {
        Some("invalid_response")
    } else {
        None
    }
}

fn bounded_output_size(s: &str) -> usize {
    s.len().min(64 * 1024)
}

pub fn kill(chat: i64) {
    if let Some(mut s) = sessions().lock().unwrap().remove(&chat) {
        let _ = s.child.kill();
        if let Some(mut b) = s.bridge.take() {
            let _ = b.kill();
            let _ = b.wait();
        }
    }
}

/// claude CLI (Node) понимает только HTTP CONNECT прокси. Для socks5(h) поднимаем
/// локальный мост gost: HTTPS_PROXY=http://127.0.0.1:PORT -F socks5-URL.
fn spawn_bridge(proxy: &str) -> Result<(String, std::process::Child)> {
    // gost не знает схему socks5h (curl-изм); хостнейм он и так отдаёт прокси
    let proxy = proxy.trim().replacen("socks5h://", "socks5://", 1);
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0")?;
        l.local_addr()?.port()
    };
    let child = std::process::Command::new("gost")
        .arg(format!("-L=http://127.0.0.1:{port}"))
        .arg(format!("-F={}", proxy))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow!("gost не запустился (нужен /usr/local/bin/gost): {e}"))?;
    std::thread::sleep(Duration::from_millis(300));
    Ok((format!("http://127.0.0.1:{port}"), child))
}

pub fn has(chat: i64) -> bool {
    sessions().lock().unwrap().contains_key(&chat)
}

/// Старт: запустить setup-token, вернуть OAuth-URL для входа продавца.
pub fn start(
    chat: i64,
    email: &str,
    proxy: &str,
    proxy_order_id: i64,
    claude_bin: &str,
    config_root: &str,
    job: Option<SellerJobRef>,
) -> Result<String> {
    if proxy_order_id < 0 {
        return Err(anyhow!("proxy order id не может быть отрицательным"));
    }
    kill(chat);
    let pty = native_pty_system();
    let pair = pty.openpty(PtySize {
        rows: 50,
        cols: 1000,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let mut cmd = CommandBuilder::new(claude_bin);
    cmd.arg("setup-token");
    let cfg_dir = prepare_config_dir(config_root, email)?;
    cmd.env("CLAUDE_CONFIG_DIR", &cfg_dir);
    cmd.env("HOME", &cfg_dir);
    cmd.env("BROWSER", format!("{}/open", shim_dir()));
    cmd.env(
        "PATH",
        format!(
            "{}:{}",
            shim_dir(),
            std::env::var("PATH").unwrap_or_default()
        ),
    );
    cmd.env("COLUMNS", "1000");
    cmd.env("LINES", "50");
    cmd.env("TERM", "xterm-256color");
    let mut bridge = None;
    if !proxy.is_empty() {
        let effective = if proxy.trim_start().to_lowercase().starts_with("socks") {
            let (url, child) = spawn_bridge(proxy)?;
            bridge = Some(child);
            url
        } else {
            proxy.to_string()
        };
        cmd.env("HTTPS_PROXY", &effective);
        cmd.env("HTTP_PROXY", &effective);
    }

    let child = pair.slave.spawn_command(cmd)?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;
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
            writer,
            child,
            buf: buf.clone(),
            email: email.to_string(),
            proxy: proxy.to_string(),
            proxy_order_id,
            job,
            bridge,
            _master: pair.master,
        },
    );

    // ждём URL до 25с
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        if let Some(url) = scan_url(&buf_string(&buf)) {
            return Ok(url);
        }
        if Instant::now() > deadline {
            kill(chat);
            return Err(anyhow!(
                "не нашёл OAuth-URL (claude setup-token не ответил вовремя)"
            ));
        }
        std::thread::sleep(Duration::from_millis(400));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_state_is_scoped_below_the_writable_root() {
        let root = Path::new("/srv/claude-api/data/authbot");
        assert_eq!(
            account_config_dir(root, "Seller+Test@example.com"),
            root.join(".claude-bot-seller-test-example-com")
        );
    }

    #[test]
    fn token_outcome_metadata_keeps_proxy_order() {
        let outcome = Outcome::Token {
            token: "secret".into(),
            email: "private@example.com".into(),
            proxy: "http://private-proxy".into(),
            proxy_order_id: 4242,
            job: None,
        };
        assert!(matches!(
            outcome,
            Outcome::Token {
                proxy_order_id: 4242,
                ..
            }
        ));
    }

    #[test]
    fn setup_token_diagnostics_are_bounded_classes_only() {
        assert_eq!(error_class("OAuth Error: denied"), Some("oauth_error"));
        assert_eq!(
            error_class("request failed with status code 403"),
            Some("http_error")
        );
        assert_eq!(error_class("the code expired"), Some("expired"));
        assert_eq!(error_class("invalid state"), Some("invalid_response"));
        assert_eq!(error_class("still waiting"), None);
        assert_eq!(bounded_output_size(&"x".repeat(70_000)), 64 * 1024);
    }
}

pub enum Outcome {
    Token {
        token: String,
        email: String,
        proxy: String,
        proxy_order_id: i64,
        job: Option<SellerJobRef>,
    },
    BadCode(Option<SellerJobRef>),
    NoToken(Option<SellerJobRef>),
}

/// Докормить code#state, извлечь токен.
pub fn feed(chat: i64, codestate: &str) -> Result<Outcome> {
    let (email, proxy, proxy_order_id, job, buf) = {
        let mut g = sessions().lock().unwrap();
        let s = g
            .get_mut(&chat)
            .ok_or_else(|| anyhow!("нет активной сессии — начни с email"))?;
        // Ink-TUI claude сабмитит ТОЛЬКО отдельным CR ПОСЛЕ того, как поле отрисовало
        // вставленный код. code#state+\r одной пачкой поле НЕ сабмитит — claude висит.
        // Поэтому: пишем код → пауза (даём TUI отрисоваться) → отдельный \r (см. Python-бот).
        s.writer.write_all(codestate.trim().as_bytes())?;
        s.writer.flush()?;
        (
            s.email.clone(),
            s.proxy.clone(),
            s.proxy_order_id,
            s.job.clone(),
            s.buf.clone(),
        )
    };
    std::thread::sleep(Duration::from_millis(1200));
    {
        let mut g = sessions().lock().unwrap();
        if let Some(s) = g.get_mut(&chat) {
            let _ = s.writer.write_all(b"\r");
            let _ = s.writer.flush();
        }
    }
    let deadline = Instant::now() + Duration::from_secs(120); // токен ~до 120с (как в Python)
    loop {
        let out = buf_string(&buf);
        if let Some(token) = scan_token(&out) {
            kill(chat);
            return Ok(Outcome::Token {
                token,
                email,
                proxy,
                proxy_order_id,
                job,
            });
        }
        if let Some(class) = error_class(&out) {
            elog::warn("authbot", format!("[setup-token] outcome=bad_code class={class} output_bytes={}", bounded_output_size(&out)));
            kill(chat);
            return Ok(Outcome::BadCode(job));
        }
        if Instant::now() > deadline {
            elog::warn("authbot", format!("[setup-token] outcome=no_token class=timeout output_bytes={}", bounded_output_size(&out)));
            kill(chat);
            return Ok(Outcome::NoToken(job));
        }
        std::thread::sleep(Duration::from_millis(400));
    }
}
