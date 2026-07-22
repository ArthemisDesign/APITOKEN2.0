//! TLS-отпечаток **байт-в-байт как у Claude Code** (Bun 1.4.0 = BoringSSL). Подтверждено на tls.peet.ws:
//! JA3 = `d871d02cecbde59abbf8f4806134addf` (совпал), JA4 тоже. Ключ — тот же движок (BoringSSL) + точный
//! конфиг: без GREASE, Bun cipher-list в порядке ClientHello, OCSP status_request(5) + SCT(18), curves
//! x25519/P256/P384, ALPN http/1.1. Эталон снят tcpdump'ом с живого claude-cli → api.anthropic.com.

use std::borrow::Cow;
use wreq::header::OrigHeaderMap;
use wreq::tls::{AlpnProtocol, TlsOptions};
use wreq::{Emulation, Group};

/// Порядок заголовков ТОЧНО как реальный claude 2.1.195 (снято mitm 2026-07-14): SDK сортирует
/// case-sensitive → все `X-*` (uppercase) идут перед `anthropic-*`/`x-app` (lowercase). `OrigHeaderMap`
/// задаёт одновременно исходный регистр и порядок на проводе. Последние четыре записи —
/// транспортный хвост Claude Code.
const HDR_ORDER: [&str; 22] = [
    "Accept",
    "Authorization",
    "Content-Type",
    "User-Agent",
    "X-Claude-Code-Session-Id",
    "X-Stainless-Arch",
    "X-Stainless-Lang",
    "X-Stainless-OS",
    "X-Stainless-Package-Version",
    "X-Stainless-Retry-Count",
    "X-Stainless-Runtime",
    "X-Stainless-Runtime-Version",
    "X-Stainless-Timeout",
    "anthropic-beta",
    "anthropic-dangerous-direct-browser-access",
    "anthropic-version",
    "x-app",
    "x-client-request-id",
    "Connection",
    "Host",
    "Accept-Encoding",
    "Content-Length",
];

/// Bun/Claude-Code cipher-list в IANA-именах (как требует BoringSSL). Порядок ClientHello:
/// TLS1.3 (1301,1302,1303), затем TLS1.2 (c02b,c02f,c02c,c030,cca9,cca8,c009,c013,c00a,c014,009c,009d,002f,0035).
const BUN_CIPHERS: &str =
    "TLS_AES_128_GCM_SHA256:TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:\
TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256:TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256:\
TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384:TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384:\
TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256:TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256:\
TLS_ECDHE_ECDSA_WITH_AES_128_CBC_SHA:TLS_ECDHE_RSA_WITH_AES_128_CBC_SHA:\
TLS_ECDHE_ECDSA_WITH_AES_256_CBC_SHA:TLS_ECDHE_RSA_WITH_AES_256_CBC_SHA:\
TLS_RSA_WITH_AES_128_GCM_SHA256:TLS_RSA_WITH_AES_256_GCM_SHA384:\
TLS_RSA_WITH_AES_128_CBC_SHA:TLS_RSA_WITH_AES_256_CBC_SHA";

/// Эмуляция с TLS-конфигом Claude Code (Bun/BoringSSL). Отдаём в `Client::builder().emulation(..)`.
pub fn bun_emulation() -> Emulation {
    let tls = TlsOptions::builder()
        .grease_enabled(Some(false)) // Bun не шлёт GREASE
        .enable_ocsp_stapling(true) // → status_request(5)
        .enable_signed_cert_timestamps(true) // → SCT(18)
        .cipher_list(Cow::Borrowed(BUN_CIPHERS))
        .curves_list(Cow::Borrowed("X25519:P-256:P-384"))
        .alpn_protocols([AlpnProtocol::HTTP1]) // ALPN=http/1.1 (Bun/undici)
        .build();
    let mut orig_headers = OrigHeaderMap::with_capacity(HDR_ORDER.len());
    for header in HDR_ORDER {
        orig_headers.insert(header);
    }
    Emulation::builder()
        .tls_options(tls)
        .orig_headers(orig_headers)
        .build(Group::new("claude-code-bun-1.4"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// Проверяет БАЙТ-В-БАЙТ регистр имён заголовков на проводе.
    /// Поднимаем сырой TCP-листенер, шлём запрос клиентом с bun_emulation, читаем сырые байты
    /// запроса и проверяем: X-Stainless-*/Accept — Title-Case; anthropic-*/x-app — lowercase.
    #[test]
    fn header_name_case_is_byte_exact_cc() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let srv = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            sock.set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .ok();
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).unwrap_or(0);
            let _ = sock
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok");
            String::from_utf8_lossy(&buf[..n]).into_owned()
        });

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let client = wreq::Client::builder()
                .emulation(bun_emulation())
                .build()
                .unwrap();
            let _ = client
                .post(format!("http://127.0.0.1:{port}/v1/messages"))
                .header("x-stainless-arch", "x64")
                .header("x-stainless-os", "Linux")
                .header("x-stainless-package-version", "0.94.0")
                .header("anthropic-beta", "oauth-2025-04-20")
                .header("anthropic-version", "2023-06-01")
                .header("x-app", "cli")
                .header("x-client-request-id", "abc")
                .header("accept-encoding", "gzip, deflate, br, zstd")
                .header("connection", "keep-alive")
                .body("{}")
                .send()
                .await;
        });

        let req = srv.join().unwrap();
        // Title-Case для стандартных / X-Stainless-* / X-Claude-*
        assert!(
            req.contains("X-Stainless-Arch: x64"),
            "ожидался Title-Case X-Stainless-Arch:\n{req}"
        );
        assert!(
            req.contains("X-Stainless-Package-Version: 0.94.0"),
            "\n{req}"
        );
        // аббревиатура OS — В ВЕРХНЕМ регистре (как реальный CC), НЕ "Os"
        assert!(
            req.contains("X-Stainless-OS: Linux"),
            "OS должен быть UPPER (X-Stainless-OS):\n{req}"
        );
        assert!(
            !req.contains("X-Stainless-Os"),
            "OS не должен быть 'Os':\n{req}"
        );
        // lowercase для anthropic-* / x-app / x-client-request-id
        assert!(
            req.contains("anthropic-beta: oauth-2025-04-20"),
            "anthropic-beta должен быть lowercase:\n{req}"
        );
        assert!(req.contains("anthropic-version: 2023-06-01"), "\n{req}");
        assert!(
            req.contains("x-app: cli"),
            "x-app должен быть lowercase:\n{req}"
        );
        assert!(req.contains("x-client-request-id: abc"), "\n{req}");
        // НЕ должно быть ошибочного Title-Case на anthropic-*/x-app
        assert!(
            !req.contains("Anthropic-Beta"),
            "anthropic-* НЕ должен быть Title-Case:\n{req}"
        );
        assert!(
            !req.contains("X-App:"),
            "x-app НЕ должен быть Title-Case:\n{req}"
        );
        // ТРАНСПОРТНЫЙ ХВОСТ в порядке CC: Connection, Host, Accept-Encoding, Content-Length
        let pos = |h: &str| req.find(h).unwrap_or_else(|| panic!("нет {h}:\n{req}"));
        let (c, h, ae, cl) = (
            pos("Connection:"),
            pos("Host:"),
            pos("Accept-Encoding:"),
            pos("Content-Length:"),
        );
        assert!(
            c < h && h < ae && ae < cl,
            "хвост не в порядке CC (Connection<Host<Accept-Encoding<Content-Length):\n{req}"
        );
        // весь хвост — ПОСЛЕ приложенческих заголовков
        assert!(
            pos("x-client-request-id:") < c,
            "транспорт должен быть после app-заголовков:\n{req}"
        );
    }
}
