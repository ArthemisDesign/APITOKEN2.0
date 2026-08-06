use anyhow::{bail, Context, Result};
use std::io::Read;

use crate::config::openai_image_settlement_diagnostic_database_url;

pub(crate) fn run() -> Result<()> {
    let mut request_id = String::new();
    std::io::stdin()
        .take(64)
        .read_to_string(&mut request_id)
        .context("read fenced image request identity")?;
    let request_id = request_id.trim();
    if !valid_request_id(request_id) {
        bail!("fenced image request identity is not a lowercase UUIDv4");
    }
    let database_url = openai_image_settlement_diagnostic_database_url()
        .context("image settlement diagnostic requires CLAUDE_API_DATABASE_URL")?;
    let mut registry = registry::pg::PgStore::connect_with_application_name(
        &database_url,
        "gpt-image-2-settlement-diagnostic",
    )?;
    registry.verify_schema()?;
    let diagnostic = registry.openai_image_settlement_diagnostic(request_id)?;
    println!("{}", serde_json::to_string(&diagnostic)?);
    Ok(())
}

fn valid_request_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes[8] == b'-'
        && bytes[13] == b'-'
        && bytes[18] == b'-'
        && bytes[23] == b'-'
        && bytes[14] == b'4'
        && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 8 | 13 | 18 | 23)
                || byte.is_ascii_digit()
                || matches!(*byte, b'a'..=b'f')
        })
}

#[cfg(test)]
mod tests {
    use super::valid_request_id;

    #[test]
    fn diagnostic_accepts_only_lowercase_uuid_v4() {
        assert!(valid_request_id("01234567-89ab-4cde-8f01-23456789abcd"));
        assert!(!valid_request_id("01234567-89AB-4CDE-8F01-23456789ABCD"));
        assert!(!valid_request_id("01234567-89ab-3cde-8f01-23456789abcd"));
        assert!(!valid_request_id("request-id"));
    }
}
