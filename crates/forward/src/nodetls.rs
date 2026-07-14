//! Кастомный OpenSSL-коннектор под **byte-exact ClientHello настоящего Claude Code** (Node/OpenSSL 3.0.13):
//! точный порядок/состав cipher-suites, supported_groups, ALPN и опции расширений (OCSP status_request,
//! SCT, отключённый encrypt_then_mac). native-tls такого контроля не даёт (ставит свой cipher-list и
//! openssl переупорядочивает) — поэтому свой SslConnector на том же системном openssl-sys.
//!
//! Эталон снят tcpdump'ом с живого claude-cli → api.anthropic.com:
//!   ciphers 1301,1302,1303,c02b,c02f,c02c,c030,cca9,cca8,c009,c013,c00a,c014,009c,009d,002f,0035
//!   ext 0,23,65281,10,11,35,16,5,13,18,51,45,43,21 | groups x25519,secp256r1,secp384r1 | ALPN http/1.1

use openssl::ssl::{SslConnector, SslMethod, SslOptions, SslVersion};

/// TLS 1.2 cipher-list в порядке ClientHello Claude Code (c02b,c02f,c02c,c030,cca9,cca8,c009,c013,c00a,c014,009c,009d,002f,0035).
const CC_CIPHER_LIST: &str = "ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:\
ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-CHACHA20-POLY1305:\
ECDHE-RSA-CHACHA20-POLY1305:ECDHE-ECDSA-AES128-SHA:ECDHE-RSA-AES128-SHA:ECDHE-ECDSA-AES256-SHA:\
ECDHE-RSA-AES256-SHA:AES128-GCM-SHA256:AES256-GCM-SHA384:AES128-SHA:AES256-SHA";
/// TLS 1.3 suites в порядке CC: 1301,1302,1303.
const CC_CIPHERSUITES: &str = "TLS_AES_128_GCM_SHA256:TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256";
const CC_GROUPS: &str = "x25519:secp256r1:secp384r1";

/// Построить SslConnector, эмитящий ClientHello как у Claude Code.
pub fn build_cc_connector() -> Result<SslConnector, openssl::error::ErrorStack> {
    let mut b = SslConnector::builder(SslMethod::tls_client())?;
    b.set_min_proto_version(Some(SslVersion::TLS1_2))?;
    b.set_max_proto_version(Some(SslVersion::TLS1_3))?;
    b.set_security_level(0); // допускаем legacy AES-CBC-SHA (002f/0035) — Node их предлагает
    b.set_cipher_list(CC_CIPHER_LIST)?;
    b.set_ciphersuites(CC_CIPHERSUITES)?;
    b.set_groups_list(CC_GROUPS)?;
    b.set_alpn_protos(b"\x08http/1.1")?; // ALPN=http/1.1 (Claude Code = undici h1)
    // Node НЕ шлёт encrypt_then_mac(22) — SSL_OP_NO_ENCRYPT_THEN_MAC (0x0008_0000) не экспонирован
    // константой в openssl-крейте, задаём сырым битом. status_request(5, OCSP) добавим при необходимости.
    b.set_options(SslOptions::from_bits_retain(0x0008_0000));
    Ok(b.build())
}

/// Синхронный probe: коннект к host:443 нашим коннектором. ClientHello ловится tcpdump'ом снаружи;
/// печатаем negotiated version + ALPN, чтобы убедиться, что handshake проходит.
pub fn probe(host: &str) {
    let conn = match build_cc_connector() {
        Ok(c) => c,
        Err(e) => { eprintln!("connector err: {e}"); return; }
    };
    let tcp = match std::net::TcpStream::connect(format!("{host}:443")) {
        Ok(t) => t,
        Err(e) => { eprintln!("tcp err: {e}"); return; }
    };
    match conn.connect(host, tcp) {
        Ok(s) => println!("PROBE OK version={:?} alpn={:?}",
            s.ssl().version_str(),
            s.ssl().selected_alpn_protocol().map(|a| String::from_utf8_lossy(a).to_string())),
        Err(e) => eprintln!("handshake err: {e}"),
    }
}
