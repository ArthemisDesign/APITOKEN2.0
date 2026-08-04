//! KIMI (Kimi Code) roster loading: the step that turns a published profile into known capacity.
//!
//! This module is the engine-side counterpart of `authbot::kimi_roster`. It owns reading and
//! validating the sealed roster; selection, transport and billing live beside it.
//!
//! Contract: `docs/engine/PROVIDER_ONBOARDING.md` §6, provider facts in
//! `docs/engine/KIMI_PROVIDER.md`. Two rules here are load-bearing:
//!
//! * **A bad reload must never empty a working pool.** Reload returns the parsed set or an error;
//!   the caller keeps its last-good pool on error. A roster that momentarily fails to parse is an
//!   operational problem, not a reason to drop every subscription mid-traffic.
//! * **Subject is the quota identity.** KIMI quota is shared across all keys and devices of an
//!   account, so two profiles with the same `user_id` would double-count one subscription. A
//!   roster carrying a duplicate subject is refused as a whole rather than silently deduplicated.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use kimi_credential::{
    decode_envelope, encode_envelope, validate_profile_id, CredentialKeyring, KimiCredential,
};
use serde::Deserialize;

/// One live subscription profile as the engine sees it.
#[derive(Clone)]
pub struct KimiProfile {
    /// Opaque roster id. Safe for logs, metrics and admin projections.
    pub id: String,
    /// Stable provider subject. Quota and dedup authority; never published outward.
    pub subject_id: String,
    /// Authoritative paid plan, the calibration cohort key.
    pub plan_name: String,
    pub region: String,
    /// Key that sealed the current envelope. Refresh keeps using it until Auth Bot publishes a
    /// replacement under a newer active key; this lets old and candidate generations rotate the
    /// same credential without needing a second environment-owned active-key setting.
    pub credential_key_id: String,
    /// Sealed material. Held in memory only.
    pub credential: KimiCredential,
}

impl std::fmt::Debug for KimiProfile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Subject and credential are deliberately absent: this type ends up in operational logs.
        formatter
            .debug_struct("KimiProfile")
            .field("id", &self.id)
            .field("plan_name", &self.plan_name)
            .field("region", &self.region)
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
/// unreadable is a roster we do not understand, and serving from a half-parsed one would quietly
/// change which subscriptions carry traffic.
pub fn load_roster(root: &Path, keyring: &CredentialKeyring) -> Result<Vec<KimiProfile>> {
    load_roster_inner(root, keyring, true)
}

fn load_roster_inner(
    root: &Path,
    keyring: &CredentialKeyring,
    missing_is_empty: bool,
) -> Result<Vec<KimiProfile>> {
    let roster_path = root.join("profiles.json");
    let bytes = match read_private(&roster_path) {
        Ok(bytes) => bytes,
        Err(error)
            if missing_is_empty
                && error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|source| source.kind() == std::io::ErrorKind::NotFound) =>
        {
            // An absent roster is a legitimate cold state, not a failure: the plane simply has no
            // capacity yet. Decide from the protected read itself, not a racy exists-then-read.
            return Ok(Vec::new());
        }
        Err(error) => return Err(error),
    };
    let parsed: ProfilesFile = serde_json::from_slice(&bytes).context("decode KIMI roster")?;

    let credentials_dir = root.join("credentials");
    let mut ids = HashSet::new();
    let mut subjects = HashSet::new();
    let mut profiles = Vec::with_capacity(parsed.profiles.len());

    for entry in parsed.profiles {
        validate_profile_id(&entry.id).context("KIMI roster profile id")?;
        if !ids.insert(entry.id.clone()) {
            bail!("KIMI roster contains a duplicate profile id");
        }
        // The roster may only point at the canonical path for its id, so a roster edit cannot
        // redirect the engine at a file outside the sealed directory.
        let expected = credentials_dir.join(format!("{}.json", entry.id));
        let recorded = PathBuf::from(&entry.credential_file);
        if !recorded.is_absolute() || recorded != expected {
            bail!("KIMI roster profile points outside its credential directory");
        }
        let envelope = decode_envelope(&read_private(&recorded)?)
            .context("decode KIMI credential envelope")?;
        let credential_key_id = envelope.key_id.clone();
        let credential = keyring
            .open(&entry.id, &envelope)
            .context("open KIMI credential envelope")?;
        if !subjects.insert(credential.subject_id.clone()) {
            // Two profiles for one account would double its capacity and split its calibration
            // evidence across two rows.
            bail!("KIMI roster contains a duplicate provider subject");
        }
        profiles.push(KimiProfile {
            id: entry.id,
            subject_id: credential.subject_id.clone(),
            plan_name: credential.plan_name.clone(),
            region: credential.region.clone(),
            credential_key_id,
            credential,
        });
    }
    Ok(profiles)
}

/// Reload an already serving roster without treating disappearance as an intentional empty fleet.
///
/// Startup accepts an absent file as a legitimate cold plane. Once the gateway has last-good
/// capacity, however, an absent file is indistinguishable from a partial/failed publication and
/// must preserve that capacity. An operator who intentionally removes every profile publishes a
/// valid `profiles.json` with an empty array.
pub fn load_roster_for_reload(
    root: &Path,
    keyring: &CredentialKeyring,
    has_last_good_capacity: bool,
) -> Result<Vec<KimiProfile>> {
    load_roster_inner(root, keyring, !has_last_good_capacity).map_err(|error| {
        if has_last_good_capacity
            && error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|source| source.kind() == std::io::ErrorKind::NotFound)
        {
            anyhow::anyhow!("KIMI roster disappeared while last-good capacity exists")
        } else {
            error
        }
    })
}

/// Atomically replace one refreshed credential envelope before its in-memory refresh lock opens.
///
/// The profile id is revalidated and determines the path; callers cannot redirect a refresh to an
/// arbitrary file. A private staging file is synced before rename and the directory is synced
/// afterwards, so a successful return is the shared blue-green authority rather than a process-
/// local token that disappears on restart.
pub fn reseal_credential(
    root: &Path,
    keyring: &CredentialKeyring,
    key_id: &str,
    profile_id: &str,
    credential: &KimiCredential,
) -> Result<()> {
    validate_profile_id(profile_id)?;
    let path = root.join("credentials").join(format!("{profile_id}.json"));
    let parent = path.parent().context("KIMI credential has no parent")?;
    let metadata = fs::symlink_metadata(parent).context("stat KIMI credential directory")?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("KIMI credential directory must be a real directory");
    }
    let envelope = keyring
        .seal(key_id, profile_id, credential)
        .context("seal refreshed KIMI credential")?;
    let bytes = encode_envelope(&envelope).context("encode refreshed KIMI credential")?;
    atomic_private_replace(&path, &bytes)
}

fn atomic_private_replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("KIMI credential has no parent")?;
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|_| anyhow::anyhow!("CSPRNG unavailable"))?;
    let staging = parent.join(format!(
        ".kimi-runtime.{}.pending",
        URL_SAFE_NO_PAD.encode(random)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&staging)
        .context("create private KIMI refresh file")?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    if let Err(error) = fs::rename(&staging, path) {
        let _ = fs::remove_file(&staging);
        return Err(error).context("publish refreshed KIMI credential");
    }
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn read_private(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).context("stat private KIMI file")?;
    if metadata.file_type().is_symlink() {
        bail!("KIMI file must not be a symlink");
    }
    if !metadata.is_file() {
        bail!("KIMI path must be a regular file");
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        bail!("KIMI file must not be group or world accessible");
    }
    fs::read(path).context("read private KIMI file")
}

#[cfg(test)]
mod tests {
    use super::*;
    use kimi_credential::{KimiCredentialKind, KIMI_STATUS_NORMAL};
    use std::io::Write;

    fn keyring() -> CredentialKeyring {
        CredentialKeyring::parse(&format!("a1:{}", "11".repeat(32))).unwrap()
    }

    fn credential(subject: &str, plan: &str) -> KimiCredential {
        KimiCredential {
            version: 1,
            kind: KimiCredentialKind::Oauth,
            access_token: "access".into(),
            refresh_token: "refresh".into(),
            expires_at: 2_000_000_000,
            scope: "coding".into(),
            subject_id: subject.into(),
            plan_name: plan.into(),
            plan_level: 10,
            status: KIMI_STATUS_NORMAL.into(),
            region: "REGION_CN".into(),
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
            let root = std::env::temp_dir().join(format!("kimi-forward-roster-{suffix}"));
            fs::create_dir_all(root.join("credentials")).unwrap();
            Self { root }
        }

        fn seal(&self, id: &str, credential: &KimiCredential) {
            let sealed = keyring().seal("a1", id, credential).unwrap();
            let path = self.root.join("credentials").join(format!("{id}.json"));
            write_private(&path, &kimi_credential::encode_envelope(&sealed).unwrap());
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
        fixture.seal("kimi-01", &credential("u_1", "Moderato"));
        fixture.write_roster(&[("kimi-01", fixture.canonical("kimi-01"))]);

        let profiles = load_roster(&fixture.root, &keyring()).unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, "kimi-01");
        assert_eq!(profiles[0].plan_name, "Moderato");
        assert_eq!(profiles[0].credential.access_token, "access");
    }

    #[test]
    fn a_refreshed_credential_is_atomically_resealed_and_reopenable() {
        let fixture = Fixture::new();
        let original = credential("u_1", "Moderato");
        fixture.seal("kimi-01", &original);
        fixture.write_roster(&[("kimi-01", fixture.canonical("kimi-01"))]);

        let mut refreshed = original;
        refreshed.access_token = "rotated-access".into();
        refreshed.refresh_token = "rotated-refresh".into();
        refreshed.expires_at += 3_600;
        reseal_credential(&fixture.root, &keyring(), "a1", "kimi-01", &refreshed).unwrap();

        let profiles = load_roster(&fixture.root, &keyring()).unwrap();
        assert_eq!(profiles[0].credential.access_token, "rotated-access");
        assert_eq!(profiles[0].credential.refresh_token, "rotated-refresh");
        let mode = fs::metadata(fixture.root.join("credentials/kimi-01.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0);
        assert!(fs::read_dir(fixture.root.join("credentials"))
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".pending")));
    }

    #[test]
    fn profile_debug_never_leaks_subject_or_credential() {
        let fixture = Fixture::new();
        fixture.seal("kimi-01", &credential("u_secret", "Moderato"));
        fixture.write_roster(&[("kimi-01", fixture.canonical("kimi-01"))]);
        let profiles = load_roster(&fixture.root, &keyring()).unwrap();
        let rendered = format!("{:?}", profiles[0]);
        assert!(!rendered.contains("u_secret"));
        assert!(!rendered.contains("access"));
        assert!(!rendered.contains("refresh"));
    }

    #[test]
    fn a_duplicate_subject_is_refused_rather_than_deduplicated() {
        let fixture = Fixture::new();
        // One subscription published twice would double its measured capacity and split its
        // calibration evidence across two rows.
        fixture.seal("kimi-01", &credential("u_1", "Moderato"));
        fixture.seal("kimi-02", &credential("u_1", "Moderato"));
        fixture.write_roster(&[
            ("kimi-01", fixture.canonical("kimi-01")),
            ("kimi-02", fixture.canonical("kimi-02")),
        ]);
        let error = load_roster(&fixture.root, &keyring()).unwrap_err();
        assert!(error.to_string().contains("duplicate provider subject"));
    }

    #[test]
    fn a_roster_pointing_outside_the_credential_directory_is_refused() {
        let fixture = Fixture::new();
        fixture.seal("kimi-01", &credential("u_1", "Moderato"));
        fixture.write_roster(&[("kimi-01", "/etc/passwd".into())]);
        assert!(load_roster(&fixture.root, &keyring()).is_err());

        // A relative path is refused for the same reason.
        fixture.write_roster(&[("kimi-01", "credentials/kimi-01.json".into())]);
        assert!(load_roster(&fixture.root, &keyring()).is_err());
    }

    #[test]
    fn a_world_readable_credential_is_refused() {
        let fixture = Fixture::new();
        fixture.seal("kimi-01", &credential("u_1", "Moderato"));
        fixture.write_roster(&[("kimi-01", fixture.canonical("kimi-01"))]);
        fs::set_permissions(
            fixture.root.join("credentials/kimi-01.json"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(load_roster(&fixture.root, &keyring()).is_err());
    }

    #[test]
    fn a_profile_id_that_escapes_its_directory_is_refused() {
        let fixture = Fixture::new();
        fixture.write_roster(&[("../escape", "/tmp/escape.json".into())]);
        assert!(load_roster(&fixture.root, &keyring()).is_err());
    }

    #[test]
    fn an_envelope_sealed_for_another_profile_is_refused() {
        let fixture = Fixture::new();
        // AAD binds an envelope to its profile id, so a moved envelope must not open.
        let sealed = keyring()
            .seal("a1", "kimi-99", &credential("u_1", "Moderato"))
            .unwrap();
        write_private(
            &fixture.root.join("credentials/kimi-01.json"),
            &kimi_credential::encode_envelope(&sealed).unwrap(),
        );
        fixture.write_roster(&[("kimi-01", fixture.canonical("kimi-01"))]);
        assert!(load_roster(&fixture.root, &keyring()).is_err());
    }

    #[test]
    fn a_partially_broken_roster_fails_whole_so_the_caller_keeps_last_good() {
        let fixture = Fixture::new();
        fixture.seal("kimi-01", &credential("u_1", "Moderato"));
        fixture.write_roster(&[
            ("kimi-01", fixture.canonical("kimi-01")),
            ("kimi-02", fixture.canonical("kimi-02")),
        ]);
        // Serving from a half-parsed roster would quietly change which subscriptions carry
        // traffic, so there is no partial success.
        assert!(load_roster(&fixture.root, &keyring()).is_err());
    }
}
