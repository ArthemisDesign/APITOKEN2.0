//! Roster discovery for sealed Codex profiles.
//!
//! The roster is one JSON document (`profiles: [{id, credential_file}]`) republished atomically
//! by the authbot. A profile counts only once its envelope opens under the configured keyring: a
//! half-finished purchase is invisible to the pool. Layout is enforced the Gemini way — every
//! credential lives at `<roster>/credentials/<id>.json`, so a roster entry can never point the
//! runtime at an arbitrary path.

use super::{CodexConfig, CodexProfileSpec, CodexProfilesFile};

/// Read and validate the roster. Returns an empty list when the roster is missing or malformed:
/// the caller keeps the previous pool rather than emptying it on a transient read failure.
pub(crate) fn discover(cfg: &CodexConfig) -> Vec<CodexProfileSpec> {
    let Ok(bytes) = std::fs::read(&cfg.profiles_file) else {
        return Vec::new();
    };
    let Ok(roster) = serde_json::from_str::<CodexProfilesFile>(
        std::str::from_utf8(&bytes).unwrap_or(""),
    ) else {
        eprintln!("Codex roster is not valid JSON; keeping the current pool");
        return Vec::new();
    };
    let credentials_dir = std::path::Path::new(&cfg.profiles_file)
        .parent()
        .map(|parent| parent.join("credentials"));
    let mut seen = std::collections::HashSet::new();
    let mut specs = Vec::with_capacity(roster.profiles.len());
    for spec in roster.profiles {
        if codex_credential::validate_profile_id(&spec.id).is_err() || !seen.insert(spec.id.clone())
        {
            eprintln!("Codex roster entry has an invalid or duplicate profile id; skipped");
            continue;
        }
        let path = std::path::Path::new(&spec.credential_file);
        let layout_ok = path.is_absolute()
            && path.file_name().and_then(|name| name.to_str())
                == Some(format!("{}.json", spec.id).as_str())
            && credentials_dir
                .as_ref()
                .is_some_and(|expected| path.parent() == Some(expected.as_path()));
        if !layout_ok {
            eprintln!(
                "Codex roster entry {} points outside <roster>/credentials/<id>.json; skipped",
                spec.id
            );
            continue;
        }
        specs.push(spec);
    }
    specs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn config(profiles_file: &std::path::Path) -> CodexConfig {
        CodexConfig {
            enabled: true,
            base_url: codex_credential::CODEX_DEFAULT_BASE_URL.to_string(),
            profiles_file: profiles_file.to_str().unwrap().to_string(),
            credential_keys: codex_credential::CredentialKeyring::parse(&format!(
                "current:{}",
                "11".repeat(32)
            ))
            .unwrap(),
            cli_version: codex_credential::CODEX_CLI_VERSION.to_string(),
            request_timeout_ms: 1_000,
            turn_timeout_ms: 1_000,
            turn_silence_timeout_ms: 1_000,
            health_probe_interval_secs: 300,
            reserve_5h: 0.10,
            reserve_7d: 0.03,
            reserve_jitter: 0.0,
            reserve_overhead_tokens: 0,
            history_ttl_secs: 600,
            history_local_cap: 32,
            history_redis_url: None,
            history_secret: None,
            history_redis_timeout_ms: 10,
            default_proxy_env: BTreeMap::new(),
            models: Vec::new(),
        }
    }

    #[test]
    fn roster_layout_is_enforced() {
        let root = std::env::temp_dir().join(format!(
            "claude-api-codex-discovery-{}",
            std::process::id()
        ));
        let credentials = root.join("credentials");
        std::fs::create_dir_all(&credentials).unwrap();
        let roster_path = root.join("profiles.json");
        let good = credentials.join("alpha.json");
        std::fs::write(&good, b"{}").unwrap();
        let roster = serde_json::json!({
            "profiles": [
                {"id": "alpha", "credential_file": good.to_str().unwrap()},
                {"id": "beta", "credential_file": "/etc/passwd"},
                {"id": "alpha", "credential_file": good.to_str().unwrap()},
                {"id": "bad id", "credential_file": good.to_str().unwrap()},
            ]
        });
        std::fs::write(&roster_path, serde_json::to_vec(&roster).unwrap()).unwrap();
        let specs = discover(&config(&roster_path));
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].id, "alpha");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn missing_or_malformed_roster_yields_no_specs() {
        let root = std::env::temp_dir().join(format!(
            "claude-api-codex-discovery-empty-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let roster_path = root.join("profiles.json");
        assert!(discover(&config(&roster_path)).is_empty());
        std::fs::write(&roster_path, b"not json").unwrap();
        assert!(discover(&config(&roster_path)).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
