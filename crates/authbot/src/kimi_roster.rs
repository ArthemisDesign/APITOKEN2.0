//! Atomic publication of acquired KIMI (Kimi Code) profiles into the engine roster.
//!
//! Separate from `kimi_oauth` on purpose: that module owns the provider exchange, this one owns
//! the filesystem contract. Contract: `docs/engine/PROVIDER_ONBOARDING.md` §6.
//!
//! The envelope is written in full first, then the roster is replaced atomically, so the engine
//! can never read a roster row whose credential file does not exist yet.
//!
//! The one KIMI-specific rule here is subject uniqueness. KIMI quota is shared across every device
//! and API key of an account, so `user_id` — not the key and not the profile id — is the quota
//! identity. Two profiles carrying the same subject would double-count one subscription's capacity
//! and split its calibration evidence, so the same subject re-authorizing replaces its existing
//! profile in place rather than creating a second one.

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
use serde::{Deserialize, Serialize};

/// Engine roster location plus the AEAD keyring used to seal published profiles.
///
/// Mirrors `codex_login::RosterConfig`: intake is gated on the keyring alone, so without keys the
/// module simply never publishes rather than half-configuring itself.
#[derive(Clone)]
pub struct RosterConfig {
    /// Roster root: `<dir>/profiles.json` + `<dir>/credentials/<id>.json`.
    pub dir: PathBuf,
    pub keyring: CredentialKeyring,
    pub active_key_id: String,
}

impl std::fmt::Debug for RosterConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RosterConfig")
            .field("dir", &self.dir)
            .field("active_key_id", &self.active_key_id)
            .field("keyring", &"REDACTED")
            .finish()
    }
}

/// Why a publication was refused. Every variant leaves the roster untouched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublishError {
    /// Filesystem, permissions or a malformed existing roster. Fail closed.
    Storage,
    /// The subject already occupies a profile and the candidate may not take it over.
    Duplicate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishedProfile {
    pub id: String,
    pub credential_file: PathBuf,
    /// True when an existing profile of the same subject was replaced in place.
    pub replaced_existing: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfilesFile {
    #[serde(default)]
    pub profiles: Vec<RosterProfile>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterProfile {
    /// Opaque id. The subject is never published here.
    pub id: String,
    pub credential_file: String,
}

/// Publish an acquired credential. Returns the roster entry that now serves this subscription.
///
/// `profile_id` is used only when a new profile is created; a re-authorization of a known subject
/// keeps its existing id so the engine's affinity, health and calibration history survive.
pub fn publish(
    root: &Path,
    keyring: &CredentialKeyring,
    active_key_id: &str,
    profile_id: &str,
    credential: &KimiCredential,
) -> Result<PublishedProfile, PublishError> {
    credential.validate().map_err(|_| PublishError::Storage)?;
    validate_profile_id(profile_id).map_err(|_| PublishError::Storage)?;

    let credentials_dir = root.join("credentials");
    private_dir(root).map_err(|_| PublishError::Storage)?;
    private_dir(&credentials_dir).map_err(|_| PublishError::Storage)?;
    let roster_path = root.join("profiles.json");

    let mut roster = if roster_path.exists() {
        let bytes = read_private(&roster_path).map_err(|_| PublishError::Storage)?;
        serde_json::from_slice::<ProfilesFile>(&bytes).map_err(|_| PublishError::Storage)?
    } else {
        ProfilesFile::default()
    };

    let mut ids = HashSet::new();
    let mut subjects = HashSet::new();
    let mut replacement: Option<(String, PathBuf)> = None;

    for profile in &roster.profiles {
        validate_profile_id(&profile.id).map_err(|_| PublishError::Storage)?;
        if !ids.insert(profile.id.clone()) {
            return Err(PublishError::Storage);
        }
        // The roster may only ever point at the exact canonical path for its id. An absolute
        // path elsewhere, or a relative one, would let a roster edit redirect the engine at a
        // file outside the sealed directory.
        let expected_path = credentials_dir.join(format!("{}.json", profile.id));
        let recorded = Path::new(&profile.credential_file);
        if !recorded.is_absolute() || recorded != expected_path {
            return Err(PublishError::Storage);
        }
        let envelope = decode_envelope(&read_private(recorded).map_err(|_| PublishError::Storage)?)
            .map_err(|_| PublishError::Storage)?;
        let existing = keyring
            .open(&profile.id, &envelope)
            .map_err(|_| PublishError::Storage)?;
        if !subjects.insert(existing.subject_id.clone()) {
            // A roster that already double-counts a subject is corrupt; refuse rather than add.
            return Err(PublishError::Storage);
        }
        if existing.subject_id == credential.subject_id {
            if replacement.is_some() {
                return Err(PublishError::Storage);
            }
            replacement = Some((profile.id.clone(), expected_path));
        }
    }

    let (target_id, credential_path, replaced_existing) = match replacement {
        // Re-authorization of a known subject. The provider rotates the refresh family on every
        // consent, so refusing here would leave the roster holding a token the provider has
        // already invalidated.
        Some((existing_id, path)) => (existing_id, path, true),
        None => {
            if ids.contains(profile_id) {
                return Err(PublishError::Duplicate);
            }
            let path = credentials_dir.join(format!("{profile_id}.json"));
            (profile_id.to_string(), path, false)
        }
    };

    let sealed = keyring
        .seal(active_key_id, &target_id, credential)
        .map_err(|_| PublishError::Storage)?;
    let bytes = encode_envelope(&sealed).map_err(|_| PublishError::Storage)?;

    // Envelope first, roster second: a roster row whose credential file is missing would make the
    // engine fail an otherwise healthy profile on every reload.
    atomic_private_replace(&credential_path, &bytes).map_err(|_| PublishError::Storage)?;

    if !replaced_existing {
        roster.profiles.push(RosterProfile {
            id: target_id.clone(),
            credential_file: credential_path.to_string_lossy().into_owned(),
        });
        let encoded = serde_json::to_vec_pretty(&roster).map_err(|_| PublishError::Storage)?;
        atomic_private_replace(&roster_path, &encoded).map_err(|_| PublishError::Storage)?;
    }

    Ok(PublishedProfile {
        id: target_id,
        credential_file: credential_path,
        replaced_existing,
    })
}

fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).context("create KIMI credential directory")?;
    let metadata = fs::symlink_metadata(path).context("stat KIMI credential directory")?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("KIMI credential directory must be a real directory");
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn read_private(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).context("stat private KIMI file")?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.permissions().mode() & 0o077 != 0
    {
        bail!("KIMI file must be a private regular non-symlink file");
    }
    fs::read(path).context("read private KIMI file")
}

fn write_new_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .context("create private KIMI file")?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn atomic_private_replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("KIMI roster has no parent")?;
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|_| anyhow::anyhow!("CSPRNG unavailable"))?;
    let staging = parent.join(format!(
        ".kimi.{}.pending",
        URL_SAFE_NO_PAD.encode(random)
    ));
    write_new_private(&staging, bytes)?;
    if let Err(error) = fs::rename(&staging, path) {
        let _ = fs::remove_file(&staging);
        return Err(error).context("publish KIMI roster");
    }
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kimi_credential::{KimiCredentialKind, KIMI_STATUS_NORMAL};

    fn keyring() -> CredentialKeyring {
        CredentialKeyring::parse(&format!("a1:{}", "11".repeat(32))).unwrap()
    }

    fn credential(subject: &str, access: &str) -> KimiCredential {
        KimiCredential {
            version: 1,
            kind: KimiCredentialKind::Oauth,
            access_token: access.into(),
            refresh_token: format!("refresh-{access}"),
            expires_at: 2_000_000_000,
            scope: "coding".into(),
            subject_id: subject.into(),
            plan_name: "Moderato".into(),
            plan_level: 10,
            status: KIMI_STATUS_NORMAL.into(),
            region: "REGION_CN".into(),
            proxy_url: String::new(),
        }
    }

    fn root() -> PathBuf {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        let path =
            std::env::temp_dir().join(format!("kimi-roster-{}", URL_SAFE_NO_PAD.encode(random)));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn publication_writes_a_private_envelope_and_an_atomic_roster() {
        let root = root();
        let ring = keyring();
        let published =
            publish(&root, &ring, "a1", "kimi-01", &credential("u_1", "access-1")).unwrap();
        assert!(!published.replaced_existing);
        assert_eq!(published.id, "kimi-01");

        let envelope_mode = fs::metadata(&published.credential_file)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(envelope_mode & 0o777, 0o600);
        assert_eq!(
            fs::metadata(root.join("credentials"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );

        let roster: ProfilesFile =
            serde_json::from_slice(&fs::read(root.join("profiles.json")).unwrap()).unwrap();
        assert_eq!(roster.profiles.len(), 1);
        assert_eq!(roster.profiles[0].id, "kimi-01");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_roster_never_carries_the_subject_or_any_secret() {
        let root = root();
        let ring = keyring();
        publish(&root, &ring, "a1", "kimi-01", &credential("u_secret", "access-1")).unwrap();
        let roster = fs::read_to_string(root.join("profiles.json")).unwrap();
        assert!(!roster.contains("u_secret"));
        assert!(!roster.contains("access-1"));
        assert!(!roster.contains("refresh-"));
        assert!(!roster.contains("Moderato"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_same_subject_replaces_its_profile_instead_of_adding_a_second_one() {
        let root = root();
        let ring = keyring();
        let first =
            publish(&root, &ring, "a1", "kimi-01", &credential("u_1", "access-1")).unwrap();
        // Re-authorization: the provider rotated the refresh family, so the old token is dead.
        let second =
            publish(&root, &ring, "a1", "kimi-99", &credential("u_1", "access-2")).unwrap();

        assert!(second.replaced_existing);
        assert_eq!(second.id, first.id, "profile identity must survive re-auth");

        let roster: ProfilesFile =
            serde_json::from_slice(&fs::read(root.join("profiles.json")).unwrap()).unwrap();
        assert_eq!(
            roster.profiles.len(),
            1,
            "one subscription must never occupy two profiles"
        );

        let envelope =
            decode_envelope(&fs::read(&second.credential_file).unwrap()).unwrap();
        let opened = ring.open(&second.id, &envelope).unwrap();
        assert_eq!(opened.access_token, "access-2");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn two_different_subjects_get_two_profiles() {
        let root = root();
        let ring = keyring();
        publish(&root, &ring, "a1", "kimi-01", &credential("u_1", "a")).unwrap();
        publish(&root, &ring, "a1", "kimi-02", &credential("u_2", "b")).unwrap();
        let roster: ProfilesFile =
            serde_json::from_slice(&fs::read(root.join("profiles.json")).unwrap()).unwrap();
        assert_eq!(roster.profiles.len(), 2);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_reused_profile_id_for_a_new_subject_is_refused() {
        let root = root();
        let ring = keyring();
        publish(&root, &ring, "a1", "kimi-01", &credential("u_1", "a")).unwrap();
        assert_eq!(
            publish(&root, &ring, "a1", "kimi-01", &credential("u_2", "b")),
            Err(PublishError::Duplicate)
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_roster_pointing_outside_the_credential_directory_fails_closed() {
        let root = root();
        let ring = keyring();
        publish(&root, &ring, "a1", "kimi-01", &credential("u_1", "a")).unwrap();

        let roster_path = root.join("profiles.json");
        let tampered = ProfilesFile {
            profiles: vec![RosterProfile {
                id: "kimi-01".into(),
                credential_file: "/etc/passwd".into(),
            }],
        };
        fs::write(&roster_path, serde_json::to_vec(&tampered).unwrap()).unwrap();
        fs::set_permissions(&roster_path, fs::Permissions::from_mode(0o600)).unwrap();

        assert_eq!(
            publish(&root, &ring, "a1", "kimi-02", &credential("u_2", "b")),
            Err(PublishError::Storage)
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_unreadable_envelope_stops_publication_rather_than_dropping_a_profile() {
        let root = root();
        let ring = keyring();
        let first = publish(&root, &ring, "a1", "kimi-01", &credential("u_1", "a")).unwrap();
        fs::write(&first.credential_file, b"not-an-envelope").unwrap();
        assert_eq!(
            publish(&root, &ring, "a1", "kimi-02", &credential("u_2", "b")),
            Err(PublishError::Storage)
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_invalid_credential_never_touches_the_roster() {
        let root = root();
        let ring = keyring();
        let mut broken = credential("u_1", "a");
        broken.plan_name = String::new();
        assert_eq!(
            publish(&root, &ring, "a1", "kimi-01", &broken),
            Err(PublishError::Storage)
        );
        assert!(!root.join("profiles.json").exists());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_world_readable_roster_is_refused() {
        let root = root();
        let ring = keyring();
        publish(&root, &ring, "a1", "kimi-01", &credential("u_1", "a")).unwrap();
        let roster_path = root.join("profiles.json");
        fs::set_permissions(&roster_path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            publish(&root, &ring, "a1", "kimi-02", &credential("u_2", "b")),
            Err(PublishError::Storage)
        );
        fs::remove_dir_all(&root).ok();
    }
}
