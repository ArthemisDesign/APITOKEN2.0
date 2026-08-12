//! Customer binary intake for the Suno (suno.com) plane.
//!
//! The real Suno API takes audio input for covers/extend-type operations (uploads up to 30 min
//! on paid tiers), but the OSS blueprint documents **no upstream upload endpoint**
//! (`docs/engine/SUNO_PROVIDER.md` §4/§6), and this plane never invents one. What exists here
//! is the customer-facing half, end to end on our side: a bounded multipart intake that
//! persists the bytes durably (tmp → fsync → mode-0600 → atomic rename → directory fsync) and
//! returns an opaque upload id. Admission of a generation carrying attachment ids then fails
//! closed with a clear 400 naming the gap, so a customer binary is never silently dropped and
//! an undocumented upstream path is never guessed.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Intake bound: 30 minutes of 320 kbps audio is ~69 MiB; 96 MiB is the documented
/// conservative ceiling (ours — the provider publishes no upload contract we can target).
pub const UPLOAD_MAX_BYTES: usize = 96 * 1024 * 1024;

/// Reviewed audio containers recognized by magic bytes. Anything else is refused: the intake
/// exists for audio attachments, and an opaque blob has no sanctioned use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioFormat {
    Mp3,
    Wav,
    Ogg,
    Flac,
    Mp4,
}

impl AudioFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
            Self::Ogg => "ogg",
            Self::Flac => "flac",
            Self::Mp4 => "m4a",
        }
    }
}

/// Sniff the container from magic bytes only — the customer filename is never trusted.
pub fn sniff_audio_format(bytes: &[u8]) -> Option<AudioFormat> {
    if bytes.len() >= 3 && &bytes[..3] == b"ID3" {
        return Some(AudioFormat::Mp3);
    }
    if bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0 {
        return Some(AudioFormat::Mp3);
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        return Some(AudioFormat::Wav);
    }
    if bytes.len() >= 4 && &bytes[..4] == b"OggS" {
        return Some(AudioFormat::Ogg);
    }
    if bytes.len() >= 4 && &bytes[..4] == b"fLaC" {
        return Some(AudioFormat::Flac);
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Some(AudioFormat::Mp4);
    }
    None
}

/// An accepted upload: the id the customer references and its sniffed format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredUpload {
    pub upload_id: String,
    pub format: AudioFormat,
    pub bytes: usize,
}

/// Persist one customer binary under `<artifact_dir>/uploads/<id>.<ext>` with the artifact
/// store's durability discipline. The id is a fresh CSPRNG identity — client input (including
/// the filename) never becomes a path component.
pub async fn store_upload(
    artifact_dir: &Path,
    upload_id: &str,
    bytes: &[u8],
) -> Result<StoredUpload> {
    if bytes.is_empty() || bytes.len() > UPLOAD_MAX_BYTES {
        anyhow::bail!("Suno upload is empty or exceeds the bound");
    }
    let format = sniff_audio_format(bytes).context("Suno upload is not a reviewed audio container")?;
    let file_name = format!("{}.{}", upload_id, format.extension());
    super::artifacts::store_into_dir(&artifact_dir.join("uploads"), &file_name, bytes).await?;
    Ok(StoredUpload {
        upload_id: upload_id.to_string(),
        format,
        bytes: bytes.len(),
    })
}

/// Resolve an upload id to its stored path. The id must parse as our own issuance shape
/// (hex suffix); anything else names nothing.
pub fn upload_path(artifact_dir: &Path, upload_id: &str) -> Option<PathBuf> {
    let suffix = upload_id.strip_prefix("suo-")?;
    if suffix.is_empty()
        || suffix.len() > 128
        || !suffix.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return None;
    }
    let dir = artifact_dir.join("uploads");
    // The extension is not part of the id; resolve by directory scan against the exact stem.
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&format!("{upload_id}.")) && !name.contains(".tmp") {
            return Some(entry.path());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_bytes_select_the_reviewed_containers() {
        assert_eq!(sniff_audio_format(b"ID3\x04\x00"), Some(AudioFormat::Mp3));
        assert_eq!(sniff_audio_format(&[0xFF, 0xFB, 0x00]), Some(AudioFormat::Mp3));
        assert_eq!(
            sniff_audio_format(b"RIFF\x24\x00\x00\x00WAVEfmt "),
            Some(AudioFormat::Wav)
        );
        assert_eq!(sniff_audio_format(b"OggS\x00"), Some(AudioFormat::Ogg));
        assert_eq!(sniff_audio_format(b"fLaC\x00"), Some(AudioFormat::Flac));
        assert_eq!(
            sniff_audio_format(b"\x00\x00\x00\x18ftypM4A "),
            Some(AudioFormat::Mp4)
        );
        // Opaque or empty content is refused.
        assert_eq!(sniff_audio_format(b""), None);
        assert_eq!(sniff_audio_format(b"MZ\x90\x00"), None);
        assert_eq!(sniff_audio_format(b"%PDF-1.7"), None);
    }

    #[test]
    fn an_upload_persists_durably_and_resolves_by_id() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let mut random = [0u8; 8];
            getrandom::fill(&mut random).unwrap();
            let suffix = random.iter().map(|b| format!("{b:02x}")).collect::<String>();
            let root = std::env::temp_dir().join(format!("suno-uploads-{suffix}"));
            let payload = b"ID3 fake-but-sniffable audio payload";
            let stored = store_upload(&root, "suo-abc123", payload).await.unwrap();
            assert_eq!(stored.format, AudioFormat::Mp3);
            assert_eq!(stored.bytes, payload.len());
            let path = upload_path(&root, "suo-abc123").unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), payload);
            // Foreign-shaped ids name nothing and never become a path.
            assert!(upload_path(&root, "../escape").is_none());
            assert!(upload_path(&root, "suo-").is_none());
            assert!(upload_path(&root, "suo-nonexistent").is_none());
            // Empty and unreviewed content fail closed.
            assert!(store_upload(&root, "suo-empty", b"").await.is_err());
            assert!(store_upload(&root, "suo-pdf", b"%PDF-1.7").await.is_err());
            std::fs::remove_dir_all(&root).unwrap();
        });
    }
}
