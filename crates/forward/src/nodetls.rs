//! TLS-отпечаток **байт-в-байт как у Claude Code** (Bun 1.4.0 = BoringSSL). Подтверждено на tls.peet.ws:
//! JA3 = `d871d02cecbde59abbf8f4806134addf` (совпал), JA4 тоже. Ключ — тот же движок (BoringSSL) + точный
//! конфиг: без GREASE, Bun cipher-list в порядке ClientHello, OCSP status_request(5) + SCT(18), curves
//! x25519/P256/P384, ALPN http/1.1. Эталон снят tcpdump'ом с живого claude-cli → api.anthropic.com.

use std::borrow::Cow;
use wreq::tls::{AlpnProtos, TlsConfig};
use wreq::{EmulationProvider, SslCurve};

/// Bun/Claude-Code cipher-list в IANA-именах (как требует BoringSSL). Порядок ClientHello:
/// TLS1.3 (1301,1302,1303), затем TLS1.2 (c02b,c02f,c02c,c030,cca9,cca8,c009,c013,c00a,c014,009c,009d,002f,0035).
const BUN_CIPHERS: &str = "TLS_AES_128_GCM_SHA256:TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:\
TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256:TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256:\
TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384:TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384:\
TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256:TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256:\
TLS_ECDHE_ECDSA_WITH_AES_128_CBC_SHA:TLS_ECDHE_RSA_WITH_AES_128_CBC_SHA:\
TLS_ECDHE_ECDSA_WITH_AES_256_CBC_SHA:TLS_ECDHE_RSA_WITH_AES_256_CBC_SHA:\
TLS_RSA_WITH_AES_128_GCM_SHA256:TLS_RSA_WITH_AES_256_GCM_SHA384:\
TLS_RSA_WITH_AES_128_CBC_SHA:TLS_RSA_WITH_AES_256_CBC_SHA";

/// EmulationProvider с TLS-конфигом Claude Code (Bun/BoringSSL). Отдаём в `Client::builder().emulation(..)`.
pub fn bun_emulation() -> EmulationProvider {
    let tls = TlsConfig::builder()
        .grease_enabled(Some(false))          // Bun не шлёт GREASE
        .enable_ocsp_stapling(true)           // → status_request(5)
        .enable_signed_cert_timestamps(true)  // → SCT(18)
        .cipher_list(Cow::Borrowed(BUN_CIPHERS))
        .curves(Cow::Owned(vec![SslCurve::X25519, SslCurve::SECP256R1, SslCurve::SECP384R1]))
        .alpn_protos(AlpnProtos::HTTP1)        // ALPN=http/1.1 (Bun/undici)
        .build();
    EmulationProvider::builder().tls_config(tls).build()
}
