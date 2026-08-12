//! Tripo3D (VAST / Holymolly) roster loading: the step that turns a published profile into
//! known capacity.
//!
//! This module is the engine-side counterpart of `authbot::tripo3d_roster`. It owns reading
//! and validating the sealed roster; selection, transport and billing live beside it.
//!
//! Contract: `docs/engine/PROVIDER_ONBOARDING.md` §6, provider facts in
//! `docs/engine/TRIPO3D_PROVIDER.md`. Three rules here are load-bearing:
//!
//! * **A bad reload must never empty a working pool.** Reload returns the parsed set or an
//!   error; the caller keeps its last-good pool on error. A roster that momentarily fails to
//!   parse is an operational problem, not a reason to drop every subscription mid-traffic.
//! * **The key is the balance identity.** Tripo3D has no machine-readable `/me`, so the static
//!   API key itself is the subject (manifest §2). Two profiles carrying the same key would
//!   double-count one prepaid account, so a roster with a duplicate key digest is refused as a
//!   whole rather than silently deduplicated.
//! * **There is no reseal-on-refresh.** The credential is a static key: nothing here rewrites
//!   envelopes. Key replacement arrives exclusively as an atomic Auth Bot republication, which
//!   the next roster reload picks up.

use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use tripo3d_credential::{decode_envelope, validate_profile_id, CredentialKeyring, Tripo3dCredential};

/// Domain-separation context for the provider-subject digest (manifest §2: the subject is the
/// keyed-BLAKE3 digest of the key, the raw key never leaves the envelope). A KDF context rather
/// than a stored keyed-hash key: any persisted key would need rotation, and rotating it would
/// orphan every calibration row keyed by the old digests.
const SUBJECT_DIGEST_CONTEXT: &str = "apitoken/tripo3d-subject-identity/v1";

/// Stable provider subject for one API key. Deterministic across restarts and keyring
/// rotations, one-way, and safe to store as the calibration/spend identity.
pub fn subject_id_of(api_key: &str) -> String {
    let digest = blake3::derive_key(SUBJECT_DIGEST_CONTEXT, api_key.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// One live prepaid profile as the engine sees it.
#[derive(Clone)]
pub struct Tripo3dProfile {
    /// Opaque roster id. Safe for logs, metrics and admin projections.
    pub id: String,
    /// Stable provider subject: the keyed digest of the key (never the key itself). Balance and
    /// dedup authority; the calibration identity.
    pub subject_id: String,
    /// Declared top-up cohort of the offer product, lowercase-normalized by the credential
    /// (e.g. `tripo3d-api-50`). The calibration cohort key (migration 0049).
    pub cohort: String,
    /// Per-profile platform origin (`https://api.tripo3d.ai` or `https://api.tripo3d.com`) from
    /// the sealed credential. Keys are only valid against the platform that issued them.
    pub base_url: String,
    /// Key that sealed the current envelope. Old keys stay readable so a keyring rotation can
    /// be online: the Auth Bot seals new envelopes under the active key while the runtime still
    /// opens existing ones.
    pub credential_key_id: String,
    /// Sealed material. Held in memory only.
    pub credential: Tripo3dCredential,
}

impl std::fmt::Debug for Tripo3dProfile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Subject and credential are deliberately absent: this type ends up in operational logs.
        formatter
            .debug_struct("Tripo3dProfile")
            .field("id", &self.id)
            .field("cohort", &self.cohort)
            .field("base_url", &self.base_url)
            .finish()
    }
}

#[derive(Debug, Default, Deserialize)]
struct ProfilesFile {
    #[serde(default)]
    profiles: Vec<RosterEntry>,
}

#[derive(Debug, Deserialize)]
struct RosterEntry {
    id: String,
    credential_file: String,
}

/// Load and fully validate the roster at `<root>/profiles.json`.
///
/// Returns every profile or an error. There is no partial success: a roster where one entry is
/// unreadable is a roster we do not understand, and serving from a half-parsed one would
/// quietly change which subscriptions carry traffic.
pub fn load_roster(root: &Path, keyring: &CredentialKeyring) -> Result<Vec<Tripo3dProfile>> {
    load_roster_inner(root, keyring, true)
}

fn load_roster_inner(
    root: &Path,
    keyring: &CredentialKeyring,
    missing_is_empty: bool,
) -> Result<Vec<Tripo3dProfile>> {
    let roster_path = root.join("profiles.json");
    let bytes = match read_private(&roster_path) {
        Ok(bytes) => bytes,
        Err(error)
            if missing_is_empty
                && error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|source| source.kind() == std::io::ErrorKind::NotFound) =>
        {
            // An absent roster is a legitimate cold state, not a failure: the plane simply has
            // no capacity yet. Decide from the protected read itself, not a racy
            // exists-then-read.
            return Ok(Vec::new());
        }
        Err(error) => return Err(error),
    };
    let parsed: ProfilesFile = serde_json::from_slice(&bytes).context("decode Tripo3D roster")?;

    let credentials_dir = root.join("credentials");
    let mut ids = HashSet::new();
    let mut subjects = HashSet::new();
    let mut profiles = Vec::with_capacity(parsed.profiles.len());

    for entry in parsed.profiles {
        validate_profile_id(&entry.id).context("Tripo3D roster profile id")?;
        if !ids.insert(entry.id.clone()) {
            bail!("Tripo3D roster contains a duplicate profile id");
        }
        // The roster may only point at the canonical path for its id, so a roster edit cannot
        // redirect the engine at a file outside the sealed directory.
        let expected = credentials_dir.join(format!("{}.json", entry.id));
        let recorded = PathBuf::from(&entry.credential_file);
        if !recorded.is_absolute() || recorded != expected {
            bail!("Tripo3D roster profile points outside its credential directory");
        }
        let envelope = decode_envelope(&read_private(&recorded)?)
            .context("decode Tripo3D credential envelope")?;
        let credential_key_id = envelope.key_id.clone();
        let credential = keyring
            .open(&entry.id, &envelope)
            .context("open Tripo3D credential envelope")?;
        let subject_id = subject_id_of(&credential.api_key);
        if !subjects.insert(subject_id.clone()) {
            // Two profiles for one key would double its capacity and split its calibration
            // evidence across two rows. The Auth Bot replaces in place instead; a roster that
            // still carries a duplicate is corrupt.
            bail!("Tripo3D roster contains a duplicate provider subject");
        }
        profiles.push(Tripo3dProfile {
            id: entry.id,
            subject_id,
            cohort: credential.cohort.clone(),
            base_url: credential.base_url.clone(),
            credential_key_id,
            credential,
        });
    }
    Ok(profiles)
}

/// Reload an already serving roster without treating disappearance as an intentional empty
/// fleet.
///
/// Startup accepts an absent file as a legitimate cold plane. Once the gateway has last-good
/// capacity, however, an absent file is indistinguishable from a partial/failed publication
/// and must preserve that capacity. An operator who intentionally removes every profile
/// publishes a valid `profiles.json` with an empty array.
pub fn load_roster_for_reload(
    root: &Path,
    keyring: &CredentialKeyring,
    has_last_good_capacity: bool,
) -> Result<Vec<Tripo3dProfile>> {
    load_roster_inner(root, keyring, !has_last_good_capacity).map_err(|error| {
        if has_last_good_capacity
            && error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|source| source.kind() == std::io::ErrorKind::NotFound)
        {
            anyhow::anyhow!("Tripo3D roster disappeared while last-good capacity exists")
        } else {
            error
        }
    })
}

fn read_private(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).context("stat private Tripo3D file")?;
    if metadata.file_type().is_symlink() {
        bail!("Tripo3D file must not be a symlink");
    }
    if !metadata.is_file() {
        bail!("Tripo3D path must be a regular file");
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        bail!("Tripo3D file must not be group or world accessible");
    }
    fs::read(path).context("read private Tripo3D file")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tripo3d_credential::{
        encode_envelope, Tripo3dCredentialKind, TRIPO3D_BASE_URL_GLOBAL,
    };

    fn keyring() -> CredentialKeyring {
        CredentialKeyring::parse(&format!("a1:{}", "11".repeat(32))).unwrap()
    }

    fn credential(key: &str) -> Tripo3dCredential {
        Tripo3dCredential {
            version: 1,
            kind: Tripo3dCredentialKind::ApiKey,
            api_key: key.into(),
            cohort: "tripo3d-api-50".into(),
            base_url: TRIPO3D_BASE_URL_GLOBAL.into(),
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
            let suffix: String = random.iter().map(|b| format!("{b:02x}")).collect();
            let root = std::env::temp_dir().join(format!("tripo3d-forward-roster-{suffix}"));
            fs::create_dir_all(root.join("credentials")).unwrap();
            Self { root }
        }

        fn seal(&self, id: &str, credential: &Tripo3dCredential) {
            let sealed = keyring().seal("a1", id, credential).unwrap();
            let path = self.root.join("credentials").join(format!("{id}.json"));
            write_private(&path, &encode_envelope(&sealed).unwrap());
        }

        fn write_roster(&self, entries: &[(&str, String)]) {
            let json = serde_json::json!({
                "profiles": entries
                    .iter()
                    .map(|(id, file)| serde_json::json!({"id": id, "credential_file": file}))
                    .collect::<Vec<_>>()
            });
            write_private(
                &self.root.join("profiles.json"),
                &serde_json::to_vec(&json).unwrap(),
            );
        }

        fn canonical(&self, id: &str) -> String {
            self.root
                .join("credentials")
                .join(format!("{id}.json"))
                .to_string_lossy()
                .into_owned()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn write_private(path: &Path, bytes: &[u8]) {
        let mut file = fs::File::create(path).unwrap();
        file.write_all(bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn an_absent_roster_is_a_cold_plane_not_an_error() {
        let fixture = Fixture::new();
        assert!(load_roster(&fixture.root, &keyring()).unwrap().is_empty());
    }

    #[test]
    fn a_serving_reload_never_treats_a_disappeared_roster_as_an_empty_fleet() {
        let fixture = Fixture::new();
        assert!(load_roster_for_reload(&fixture.root, &keyring(), false)
            .unwrap()
            .is_empty());
        let error = load_roster_for_reload(&fixture.root, &keyring(), true).unwrap_err();
        assert!(error.to_string().contains("roster disappeared"));
    }

    #[test]
    fn an_explicit_empty_roster_is_the_only_way_to_remove_a_serving_fleet() {
        let fixture = Fixture::new();
        fixture.write_roster(&[]);
        assert!(load_roster_for_reload(&fixture.root, &keyring(), true)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_published_profile_becomes_known_capacity() {
        let fixture = Fixture::new();
        fixture.seal("tripo3d-01", &credential("tsk_key-1"));
        fixture.write_roster(&[("tripo3d-01", fixture.canonical("tripo3d-01"))]);

        let profiles = load_roster(&fixture.root, &keyring()).unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, "tripo3d-01");
        assert_eq!(profiles[0].cohort, "tripo3d-api-50");
        assert_eq!(profiles[0].base_url, TRIPO3D_BASE_URL_GLOBAL);
        assert_eq!(profiles[0].credential.api_key, "tsk_key-1");
        // The subject is the keyed digest of the key, stable and never the key itself.
        assert_eq!(profiles[0].subject_id, subject_id_of("tsk_key-1"));
        assert!(!profiles[0].subject_id.contains("tsk_key-1"));
    }

    #[test]
    fn the_subject_digest_is_deterministic_and_domain_separated() {
        let first = subject_id_of("tsk_key-1");
        assert_eq!(first, subject_id_of("tsk_key-1"));
        assert_ne!(first, subject_id_of("tsk_key-2"));
        assert_eq!(first.len(), 64);
        // Not the plain hash of the key: the KDF context separates this identity from every
        // other BLAKE3 usage in the system, including the GLM subject digest of the same key.
        assert_ne!(first, blake3::hash(b"tsk_key-1").to_hex().as_str());
        let glm_digest = blake3::derive_key("apitoken/glm-subject-identity/v1", b"tsk_key-1");
        let glm_hex = glm_digest.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_ne!(first, glm_hex);
    }

    #[test]
    fn a_duplicate_key_is_refused_rather_than_deduplicated() {
        let fixture = Fixture::new();
        // One prepaid account published twice would double its measured capacity and split its
        // calibration evidence across two rows.
        fixture.seal("tripo3d-01", &credential("tsk_key-1"));
        fixture.seal("tripo3d-02", &credential("tsk_key-1"));
        fixture.write_roster(&[
            ("tripo3d-01", fixture.canonical("tripo3d-01")),
            ("tripo3d-02", fixture.canonical("tripo3d-02")),
        ]);
        let error = load_roster(&fixture.root, &keyring()).unwrap_err();
        assert!(error.to_string().contains("duplicate provider subject"));
    }

    #[test]
    fn a_roster_pointing_outside_the_credential_directory_is_refused() {
        let fixture = Fixture::new();
        fixture.seal("tripo3d-01", &credential("tsk_key-1"));
        fixture.write_roster(&[("tripo3d-01", "/etc/passwd".into())]);
        assert!(load_roster(&fixture.root, &keyring()).is_err());

        // A relative path is refused for the same reason.
        fixture.write_roster(&[("tripo3d-01", "credentials/tripo3d-01.json".into())]);
        assert!(load_roster(&fixture.root, &keyring()).is_err());
    }

    #[test]
    fn a_world_readable_credential_is_refused() {
        let fixture = Fixture::new();
        fixture.seal("tripo3d-01", &credential("tsk_key-1"));
        fixture.write_roster(&[("tripo3d-01", fixture.canonical("tripo3d-01"))]);
        fs::set_permissions(
            fixture.root.join("credentials/tripo3d-01.json"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(load_roster(&fixture.root, &keyring()).is_err());
    }

    #[test]
    fn an_envelope_sealed_for_another_profile_is_refused() {
        let fixture = Fixture::new();
        // AAD binds an envelope to its profile id, so a moved envelope must not open.
        let sealed = keyring()
            .seal("a1", "tripo3d-99", &credential("tsk_key-1"))
            .unwrap();
        write_private(
            &fixture.root.join("credentials/tripo3d-01.json"),
            &encode_envelope(&sealed).unwrap(),
        );
        fixture.write_roster(&[("tripo3d-01", fixture.canonical("tripo3d-01"))]);
        assert!(load_roster(&fixture.root, &keyring()).is_err());
    }

    #[test]
    fn a_partially_broken_roster_fails_whole_so_the_caller_keeps_last_good() {
        let fixture = Fixture::new();
        fixture.seal("tripo3d-01", &credential("tsk_key-1"));
        fixture.write_roster(&[
            ("tripo3d-01", fixture.canonical("tripo3d-01")),
            ("tripo3d-02", fixture.canonical("tripo3d-02")),
        ]);
        // Serving from a half-parsed roster would quietly change which subscriptions carry
        // traffic, so there is no partial success.
        assert!(load_roster(&fixture.root, &keyring()).is_err());
    }

    #[test]
    fn profile_debug_never_leaks_the_key_the_subject_or_the_proxy() {
        let fixture = Fixture::new();
        let mut credential = credential("tsk_secret-key-9f8c");
        credential.proxy_url = "http://user:pr0xy-pass@egress.example:8080".into();
        fixture.seal("tripo3d-01", &credential);
        fixture.write_roster(&[("tripo3d-01", fixture.canonical("tripo3d-01"))]);
        let profiles = load_roster(&fixture.root, &keyring()).unwrap();
        let rendered = format!("{:?}", profiles[0]);
        assert!(!rendered.contains("tsk_secret-key-9f8c"));
        assert!(!rendered.contains("pr0xy-pass"));
        assert!(!rendered.contains(&profiles[0].subject_id));
        // Non-secret identity stays visible so operators can diagnose.
        assert!(rendered.contains("tripo3d-01"));
        assert!(rendered.contains("tripo3d-api-50"));
    }
}
