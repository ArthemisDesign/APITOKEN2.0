//! Atomic publication of acquired GLM Coding Plan (Zhipu AI / Z.ai) profiles into the engine
//! roster.
//!
//! Separate from `glm_key` on purpose: that module owns provider validation, this one owns
//! the filesystem contract. Contract: `docs/engine/PROVIDER_ONBOARDING.md` §6; provider
//! facts: `docs/engine/GLM_PROVIDER.md` §2.1.
//!
//! The envelope is written in full first, then the roster is replaced atomically, so the
//! engine can never read a roster row whose credential file does not exist yet.
//!
//! The one GLM-specific rule here is key uniqueness. GLM has no machine-readable `/me`, so
//! the static API key itself is the quota identity (`docs/engine/GLM_PROVIDER.md` §2.1). Two
//! profiles carrying the same key would double-count one subscription's capacity and split
//! its calibration evidence, so re-publishing an already-known key replaces its existing
//! profile in place rather than creating a second one. The comparison runs on opened
//! envelopes inside the safe zone; the raw key never leaves the sealed directory.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use glm_credential::{
    decode_envelope, encode_envelope, validate_profile_id, CredentialKeyring, GlmCredential,
};
use serde::{Deserialize, Serialize};

/// Engine roster location plus the AEAD keyring used to seal published profiles.
///
/// Mirrors `kimi_roster::RosterConfig`: intake is gated on the keyring alone, so without
/// keys the module simply never publishes rather than half-configuring itself.
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
    /// A different key already occupies the requested profile id.
    Duplicate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishedProfile {
    pub id: String,
    pub credential_file: PathBuf,
    /// True when an existing profile of the same key was replaced in place.
    pub replaced_existing: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfilesFile {
    #[serde(default)]
    pub profiles: Vec<RosterProfile>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterProfile {
    /// Opaque id. The key is never published here.
    pub id: String,
    pub credential_file: String,
}

/// Publish an acquired credential. Returns the roster entry that now serves this
/// subscription.
///
/// `profile_id` is used only when a new profile is created; re-publishing an already-known
/// key keeps its existing id, so the engine's affinity, health and calibration history
/// survive the replacement.
pub fn publish(
    root: &Path,
    keyring: &CredentialKeyring,
    active_key_id: &str,
    profile_id: &str,
    credential: &GlmCredential,
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
    let mut keys = HashSet::new();
    let mut replacement: Option<(String, PathBuf)> = None;

    for profile in &roster.profiles {
        validate_profile_id(&profile.id).map_err(|_| PublishError::Storage)?;
        if !ids.insert(profile.id.clone()) {
            return Err(PublishError::Storage);
        }
        // The roster may only ever point at the exact canonical path for its id. An absolute
        // path elsewhere, or a relative one, would let a roster edit redirect the engine at
        // a file outside the sealed directory.
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
        if !keys.insert(existing.api_key.clone()) {
            // A roster that already double-counts a key is corrupt; refuse rather than add.
            return Err(PublishError::Storage);
        }
        if existing.api_key == credential.api_key {
            if replacement.is_some() {
                return Err(PublishError::Storage);
            }
            replacement = Some((profile.id.clone(), expected_path));
        }
    }

    let (target_id, credential_path, replaced_existing) = match replacement {
        // Re-publication of a known key. Rotation at GLM means reissuing the key in the
        // console, and the seller flow for a reissued key arrives as a *different* key; the
        // same key re-validated refreshes plan/base/proxy material in place, so refusing
        // here would leave the roster holding stale evidence for no benefit.
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

    // Envelope first, roster second: a roster row whose credential file is missing would
    // make the engine fail an otherwise healthy profile on every reload.
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
    fs::create_dir_all(path).context("create GLM credential directory")?;
    let metadata = fs::symlink_metadata(path).context("stat GLM credential directory")?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("GLM credential directory must be a real directory");
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn read_private(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).context("stat private GLM file")?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.permissions().mode() & 0o077 != 0
    {
        bail!("GLM file must be a private regular non-symlink file");
    }
    fs::read(path).context("read private GLM file")
}

fn write_new_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .context("create private GLM file")?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn atomic_private_replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("GLM roster has no parent")?;
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|_| anyhow::anyhow!("CSPRNG unavailable"))?;
    let staging = parent.join(format!(".glm.{}.pending", URL_SAFE_NO_PAD.encode(random)));
    write_new_private(&staging, bytes)?;
    if let Err(error) = fs::rename(&staging, path) {
        let _ = fs::remove_file(&staging);
        return Err(error).context("publish GLM roster");
    }
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use glm_credential::{GlmCredentialKind, GlmPlan, GLM_BASE_URL_INTERNATIONAL};

    fn keyring() -> CredentialKeyring {
        CredentialKeyring::parse(&format!("a1:{}", "11".repeat(32))).unwrap()
    }

    fn credential(key: &str) -> GlmCredential {
        credential_full(key, GlmPlan::Pro, "")
    }

    fn credential_full(key: &str, plan: GlmPlan, proxy: &str) -> GlmCredential {
        GlmCredential {
            version: 1,
            kind: GlmCredentialKind::PlanKey,
            api_key: key.into(),
            plan,
            base_url: GLM_BASE_URL_INTERNATIONAL.into(),
            proxy_url: proxy.into(),
        }
    }

    fn root() -> PathBuf {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        let path =
            std::env::temp_dir().join(format!("glm-roster-{}", URL_SAFE_NO_PAD.encode(random)));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn publication_writes_a_private_envelope_and_an_atomic_roster() {
        let root = root();
        let ring = keyring();
        let published = publish(&root, &ring, "a1", "glm-01", &credential("zai-key-1")).unwrap();
        assert!(!published.replaced_existing);
        assert_eq!(published.id, "glm-01");

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
        assert_eq!(roster.profiles[0].id, "glm-01");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_roster_never_carries_the_key_the_plan_or_the_proxy() {
        let root = root();
        let ring = keyring();
        publish(
            &root,
            &ring,
            "a1",
            "glm-01",
            &credential_full(
                "zai-secret-key-9f8c7b",
                GlmPlan::Max,
                "http://user:egress-pass@egress.example:8080",
            ),
        )
        .unwrap();
        let roster = fs::read_to_string(root.join("profiles.json")).unwrap();
        assert!(!roster.contains("zai-secret-key-9f8c7b"));
        // Quoted: the random temp-dir suffix is base64url and could contain a bare `max`.
        assert!(!roster.contains("\"max\""));
        assert!(!roster.contains("egress.example"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_same_key_replaces_its_profile_instead_of_adding_a_second_one() {
        let root = root();
        let ring = keyring();
        let first = publish(&root, &ring, "a1", "glm-01", &credential("zai-key-1")).unwrap();
        // The same key re-validated: refreshed material replaces the profile on the spot.
        let second = publish(
            &root,
            &ring,
            "a1",
            "glm-99",
            &credential_full("zai-key-1", GlmPlan::Max, ""),
        )
        .unwrap();

        assert!(second.replaced_existing);
        assert_eq!(
            second.id, first.id,
            "profile identity must survive re-publication"
        );

        let roster: ProfilesFile =
            serde_json::from_slice(&fs::read(root.join("profiles.json")).unwrap()).unwrap();
        assert_eq!(
            roster.profiles.len(),
            1,
            "one subscription must never occupy two profiles"
        );

        let envelope = decode_envelope(&fs::read(&second.credential_file).unwrap()).unwrap();
        let opened = ring.open(&second.id, &envelope).unwrap();
        assert_eq!(opened.api_key, "zai-key-1");
        assert_eq!(opened.plan, GlmPlan::Max);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn two_different_keys_get_two_profiles() {
        let root = root();
        let ring = keyring();
        publish(&root, &ring, "a1", "glm-01", &credential("zai-key-1")).unwrap();
        publish(&root, &ring, "a1", "glm-02", &credential("zai-key-2")).unwrap();
        let roster: ProfilesFile =
            serde_json::from_slice(&fs::read(root.join("profiles.json")).unwrap()).unwrap();
        assert_eq!(roster.profiles.len(), 2);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_reused_profile_id_for_a_new_key_is_refused() {
        let root = root();
        let ring = keyring();
        publish(&root, &ring, "a1", "glm-01", &credential("zai-key-1")).unwrap();
        assert_eq!(
            publish(&root, &ring, "a1", "glm-01", &credential("zai-key-2")),
            Err(PublishError::Duplicate)
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_roster_pointing_outside_the_credential_directory_fails_closed() {
        let root = root();
        let ring = keyring();
        publish(&root, &ring, "a1", "glm-01", &credential("zai-key-1")).unwrap();

        let roster_path = root.join("profiles.json");
        let tampered = ProfilesFile {
            profiles: vec![RosterProfile {
                id: "glm-01".into(),
                credential_file: "/etc/passwd".into(),
            }],
        };
        fs::write(&roster_path, serde_json::to_vec(&tampered).unwrap()).unwrap();
        fs::set_permissions(&roster_path, fs::Permissions::from_mode(0o600)).unwrap();

        assert_eq!(
            publish(&root, &ring, "a1", "glm-02", &credential("zai-key-2")),
            Err(PublishError::Storage)
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_unreadable_envelope_stops_publication_rather_than_dropping_a_profile() {
        let root = root();
        let ring = keyring();
        let first = publish(&root, &ring, "a1", "glm-01", &credential("zai-key-1")).unwrap();
        fs::write(&first.credential_file, b"not-an-envelope").unwrap();
        assert_eq!(
            publish(&root, &ring, "a1", "glm-02", &credential("zai-key-2")),
            Err(PublishError::Storage)
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_symlinked_envelope_stops_publication() {
        let root = root();
        let ring = keyring();
        let first = publish(&root, &ring, "a1", "glm-01", &credential("zai-key-1")).unwrap();
        // Replace the envelope with a symlink to a perfectly valid envelope elsewhere: the
        // safe zone never follows links out of the sealed directory.
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        let elsewhere =
            std::env::temp_dir().join(format!("glm-elsewhere-{}", URL_SAFE_NO_PAD.encode(random)));
        fs::write(&elsewhere, fs::read(&first.credential_file).unwrap()).unwrap();
        fs::remove_file(&first.credential_file).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &first.credential_file).unwrap();
        assert_eq!(
            publish(&root, &ring, "a1", "glm-02", &credential("zai-key-2")),
            Err(PublishError::Storage)
        );
        let _ = fs::remove_file(&elsewhere);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_invalid_credential_never_touches_the_roster() {
        let root = root();
        let ring = keyring();
        let mut broken = credential("zai-key-1");
        broken.api_key = String::new();
        assert_eq!(
            publish(&root, &ring, "a1", "glm-01", &broken),
            Err(PublishError::Storage)
        );
        assert!(!root.join("profiles.json").exists());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_world_readable_roster_is_refused() {
        let root = root();
        let ring = keyring();
        publish(&root, &ring, "a1", "glm-01", &credential("zai-key-1")).unwrap();
        let roster_path = root.join("profiles.json");
        fs::set_permissions(&roster_path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            publish(&root, &ring, "a1", "glm-02", &credential("zai-key-2")),
            Err(PublishError::Storage)
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn publication_leaves_no_staging_files_behind() {
        let root = root();
        let ring = keyring();
        publish(&root, &ring, "a1", "glm-01", &credential("zai-key-1")).unwrap();
        for dir in [root.clone(), root.join("credentials")] {
            let leftovers: Vec<_> = fs::read_dir(&dir)
                .unwrap()
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_name().to_string_lossy().starts_with(".glm."))
                .collect();
            assert!(leftovers.is_empty(), "staging files left in {dir:?}");
        }
        fs::remove_dir_all(&root).ok();
    }
}
