//! Upload transport for the Tripo3D (VAST / Holymolly) plane: the two real mechanisms the
//! platform documents (`docs/engine/TRIPO3D_PROVIDER.md` §4):
//!
//! * **Images** — `POST {base}/v2/openapi/upload/sts`, multipart, ≤20 MB, returns an
//!   `image_token`. The plane passes the customer's bytes through; nothing is persisted, so no
//!   fsync discipline applies here (that belongs to the artifact store).
//! * **Model files** — `POST {base}/v2/openapi/upload/sts/token` returns temporary S3
//!   credentials; the file is then PUT to the S3-compatible store with AWS Signature V4 and the
//!   task references `file: {"object": {bucket, key}}`.
//!
//! Both flows run on the uploading profile's pinned egress and per-profile platform origin.
//! Uploads are account-scoped: a token is usable only by tasks created on the same key, which is
//! why the gateway pins an uploaded token to its profile.

use anyhow::{Context, Result};
use hmac::Mac as _;
use sha2::Digest as _;

use super::client::StsSession;

/// The direct image upload accepts at most 20 MB (manifest §4).
pub const IMAGE_UPLOAD_MAX_BYTES: usize = 20 * 1024 * 1024;
/// Customer model files are bounded at 64 MiB: the documented image cap is 20 MB and the model
/// flow carries no documented bound, so this is OUR conservative bound, not the provider's.
pub const MODEL_UPLOAD_MAX_BYTES: usize = 64 * 1024 * 1024;

/// Image formats the upload endpoint admits (SDK `_EXT_TO_STS_FORMAT`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Webp,
}

impl ImageFormat {
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::Webp => "image/webp",
        }
    }

    /// The `format` value of the STS token request for a model file (SDK `_EXT_TO_STS_FORMAT`).
    pub fn as_sts_format(self) -> &'static str {
        match self {
            Self::Jpeg => "jpeg",
            Self::Png => "png",
            Self::Webp => "webp",
        }
    }
}

/// Model formats the STS flow admits (SDK `_EXT_TO_STS_FORMAT`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelFormat {
    Glb,
    Obj,
    Fbx,
    Stl,
}

impl ModelFormat {
    pub fn from_extension(extension: &str) -> Option<Self> {
        Some(match extension.to_ascii_lowercase().as_str() {
            "glb" => Self::Glb,
            "obj" => Self::Obj,
            "fbx" => Self::Fbx,
            "stl" => Self::Stl,
            _ => return None,
        })
    }

    pub fn as_sts_format(self) -> &'static str {
        match self {
            Self::Glb => "glb",
            Self::Obj => "obj",
            Self::Fbx => "fbx",
            Self::Stl => "stl",
        }
    }

    pub fn content_type(self) -> &'static str {
        // S3 stores whatever we declare; the platform consumes the object by key, not by MIME.
        "application/octet-stream"
    }
}

/// Sniff an image's format from its magic bytes. An unrecognized payload fails closed — the
/// declared content type must be true, or the platform would store a mislabeled object.
pub fn sniff_image_format(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(b"\xFF\xD8\xFF") {
        Some(ImageFormat::Jpeg)
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageFormat::Png)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::Webp)
    } else {
        None
    }
}

/// Build a one-field multipart/form-data body. Hand-rolled on purpose: the body is a bounded
/// buffer assembled once (wreq's multipart feature is not enabled, and a dependency for a
/// three-part envelope is not justified). The boundary is caller-supplied CSPRNG material.
pub fn build_multipart_body(
    field: &str,
    filename: &str,
    content_type: &str,
    bytes: &[u8],
    boundary: &str,
) -> Vec<u8> {
    let mut body = Vec::with_capacity(bytes.len() + 512);
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"{field}\"; filename=\"{filename}\"\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

/// A fresh multipart boundary from the operating-system CSPRNG.
pub fn fresh_boundary() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("operating-system CSPRNG unavailable");
    let mut hex = String::with_capacity(40);
    hex.push_str("tripo3d-");
    for byte in bytes {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// AWS Signature V4 for a single `PUT` of an in-memory payload. The region is not on the STS
/// wire (the SDK's boto3 call leaves it default), so the signature uses `us-east-1` — an
/// `unknown` recorded in manifest §6 until the live gate pins the region.
pub fn sigv4_authorization(
    session: &StsSession,
    payload_sha256: &[u8; 32],
    amz_date: &str,
    date: &str,
) -> (String, String) {
    let host = session.s3_host.trim_end_matches('/');
    // The Host header is the bare authority even when the test wire carries an explicit scheme.
    let host_header = host
        .strip_prefix("https://")
        .or_else(|| host.strip_prefix("http://"))
        .unwrap_or(host);
    let path = format!("/{}", session.object_key);
    let payload_hex = payload_sha256.iter().map(|b| format!("{b:02x}")).collect::<String>();

    let canonical_headers = format!(
        "host:{host_header}\nx-amz-content-sha256:{payload_hex}\nx-amz-date:{amz_date}\nx-amz-security-token:{}\n",
        session.session_token
    );
    let signed_headers = "host;x-amz-content-sha256;x-amz-date;x-amz-security-token";
    let canonical_request =
        format!("PUT\n{path}\n\n{canonical_headers}\n{signed_headers}\n{payload_hex}");
    let canonical_hash = sha2::Sha256::digest(canonical_request.as_bytes());
    let canonical_hex = canonical_hash.iter().map(|b| format!("{b:02x}")).collect::<String>();

    let scope = format!("{date}/us-east-1/s3/aws4_request");
    let string_to_sign = format!("AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{canonical_hex}");

    let key = signing_key(&session.secret_key, date, "us-east-1", "s3");
    let mut mac = HmacSha256::new_from_slice(&key).expect("HMAC accepts any key length");
    mac.update(string_to_sign.as_bytes());
    let signature = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();

    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        session.access_key, scope, signed_headers, signature
    );
    (authorization, payload_hex)
}

type HmacSha256 = hmac::Hmac<sha2::Sha256>;

/// The SigV4 key-derivation chain: `AWS4`-prefixed secret, then date, region, service and the
/// terminal `aws4_request` label. Split out so the documented AWS reference vector pins it.
fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(format!("AWS4{secret}").as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(date.as_bytes());
    let mut key = mac.finalize().into_bytes();
    for message in [region, service, "aws4_request"] {
        let mut mac = HmacSha256::new_from_slice(&key).expect("HMAC accepts any key length");
        mac.update(message.as_bytes());
        key = mac.finalize().into_bytes();
    }
    key.into()
}

/// PUT one bounded payload to the STS-authorized object. The endpoint is the session's own
/// `s3_host`; the egress client is the uploading profile's, and no redirect is ever followed.
pub async fn s3_put_object(
    client: &wreq::Client,
    session: &StsSession,
    payload: Vec<u8>,
    content_type: &str,
    now: std::time::SystemTime,
) -> Result<()> {
    let payload_sha256: [u8; 32] = sha2::Sha256::digest(&payload).into();
    let (amz_date, date) = sigv4_timestamps(now);
    let (authorization, payload_hex) = sigv4_authorization(session, &payload_sha256, &amz_date, &date);
    let url = format!(
        "{}/{}",
        session.s3_host.trim_end_matches('/'),
        session.object_key
    );
    // The wire carries a bare host (the SDK prefixes `https://`); an explicit scheme — loopback
    // mock upstreams in tests — is honored as given.
    let url = if url.starts_with("http://") || url.starts_with("https://") {
        url
    } else {
        format!("https://{url}")
    };
    let response = client
        .put(url)
        .header("authorization", authorization)
        .header("x-amz-content-sha256", payload_hex)
        .header("x-amz-date", amz_date)
        .header("x-amz-security-token", &session.session_token)
        .header("content-type", content_type)
        .body(payload)
        .send()
        .await
        .context("Tripo3D model upload transport failure")?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        // The body may carry an S3 XML error with the session's bucket; log only the status.
        anyhow::bail!("Tripo3D model upload refused with HTTP {status}");
    }
    Ok(())
}

/// The two timestamp forms SigV4 needs: `YYYYMMDD'T'HHMMSS'Z'` and `YYYYMMDD`.
fn sigv4_timestamps(now: std::time::SystemTime) -> (String, String) {
    let secs = now
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0) as i64;
    let days = secs.div_euclid(86_400);
    let seconds_of_day = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) = (
        seconds_of_day / 3_600,
        seconds_of_day % 3_600 / 60,
        seconds_of_day % 60,
    );
    (
        format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z"),
        format!("{year:04}{month:02}{day:02}"),
    )
}

/// Howard Hinnant's civil-from-days algorithm (public domain), days since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    // Shift the epoch to 0000-03-01 so March is the first month (leap day lands at era end).
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_param = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_param + 2) / 5 + 1;
    let month = if month_param < 10 {
        month_param + 3
    } else {
        month_param - 9
    };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_sniffing_recognizes_the_admitted_formats_only() {
        assert_eq!(sniff_image_format(b"\xFF\xD8\xFF\xE0...."), Some(ImageFormat::Jpeg));
        assert_eq!(
            sniff_image_format(b"\x89PNG\r\n\x1a\n...."),
            Some(ImageFormat::Png)
        );
        assert_eq!(
            sniff_image_format(b"RIFF....WEBP...."),
            Some(ImageFormat::Webp)
        );
        assert_eq!(sniff_image_format(b"GIF89a"), None);
        assert_eq!(sniff_image_format(b""), None);
    }

    #[test]
    fn model_format_is_extension_bounded() {
        assert_eq!(ModelFormat::from_extension("glb"), Some(ModelFormat::Glb));
        assert_eq!(ModelFormat::from_extension("STL"), Some(ModelFormat::Stl));
        assert_eq!(ModelFormat::from_extension("exe"), None);
        assert_eq!(ModelFormat::from_extension(""), None);
    }

    #[test]
    fn multipart_body_has_exact_envelope() {
        let body = build_multipart_body("file", "a.png", "image/png", b"PNG", "B");
        let text = String::from_utf8(body).unwrap();
        assert_eq!(
            text,
            "--B\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.png\"\r\nContent-Type: image/png\r\n\r\nPNG\r\n--B--\r\n"
        );
    }

    /// The key-derivation chain is pinned against two independent implementations of the same
    /// standard chain (Python stdlib `hmac` and OpenSSL `dgst -mac hmac`, agreeing): secret
    /// `AWS4wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY`, date 20150830, region us-east-1, service
    /// `iam` → the kSigning below. The full-request signature itself is verified structurally
    /// (shape, determinism): the plane's exact signed-header set has no published reference
    /// vector, and inventing a golden value would test nothing.
    #[test]
    fn sigv4_key_derivation_matches_the_independently_verified_chain() {
        let key = signing_key(
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "20150830",
            "us-east-1",
            "iam",
        );
        let hex = key.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(
            hex,
            "c4afb1cc5771d871763a393e44b703571b55cc28424d1a5e86da6ed3c154a4b9"
        );
    }

    #[test]
    fn sigv4_signature_is_deterministic_and_well_formed() {
        let session = StsSession {
            s3_host: "s3.example.com".into(),
            access_key: "AKIAEXAMPLE".into(),
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
            session_token: "token".into(),
            bucket: "bucket".into(),
            object_key: "dir/file.glb".into(),
        };
        let payload_sha256: [u8; 32] = sha2::Sha256::digest(b"payload").into();
        let (first, payload_hex) =
            sigv4_authorization(&session, &payload_sha256, "20150830T123600Z", "20150830");
        let (second, _) = sigv4_authorization(&session, &payload_sha256, "20150830T123600Z", "20150830");
        assert_eq!(first, second, "signing must be deterministic");
        assert!(first.starts_with(
            "AWS4-HMAC-SHA256 Credential=AKIAEXAMPLE/20150830/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token, Signature="
        ));
        let signature = first.rsplit('=').next().unwrap();
        assert_eq!(signature.len(), 64);
        assert!(signature.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(payload_hex.len(), 64);
        // The payload hash is content-addressed input, not derived state.
        let other: [u8; 32] = sha2::Sha256::digest(b"different").into();
        let (changed, _) = sigv4_authorization(&session, &other, "20150830T123600Z", "20150830");
        assert_ne!(first, changed);
    }

    #[test]
    fn timestamps_format_both_sigv4_forms() {
        // 2015-08-30T12:36:00Z = 1440938160.
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_440_938_160);
        let (amz, date) = sigv4_timestamps(now);
        assert_eq!(amz, "20150830T123600Z");
        assert_eq!(date, "20150830");
        // The civil algorithm across month and leap boundaries.
        let leap = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_582_934_400); // 2020-02-29
        assert_eq!(sigv4_timestamps(leap).1, "20200229");
    }
}
