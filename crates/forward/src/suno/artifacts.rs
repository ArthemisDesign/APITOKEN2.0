//! Artifact store for the Suno (suno.com) plane.
//!
//! Upstream media URLs are short-lived and never cross the plane boundary
//! (`docs/engine/SUNO_PROVIDER.md` §4), so a finalized generation's audio is downloaded
//! IMMEDIATELY into the plane's own storage and customers are served from there.
//!
//! Persistence discipline: every artifact is streamed to a temporary sibling file bounded by
//! [`ARTIFACT_MAX_BYTES`], fsynced, mode-0600, then atomically renamed into
//! `<artifact_dir>/<request_id>/<name>` and the directory is fsynced. A reader therefore only
//! ever sees a complete file or nothing, and a crash mid-write leaves a tmp orphan, never a
//! half artifact under a served name. Lyrics text (no upstream URL) is persisted through the
//! same tmp→rename path from memory.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use futures_util::StreamExt;

/// Per-artifact bound. Paid-tier audio runs to ~8 min per clip (WAV on Pro ≈ 80 MiB); 256 MiB
/// is the documented conservative ceiling (ours, not the provider's — the provider publishes
/// none).
pub const ARTIFACT_MAX_BYTES: usize = 256 * 1024 * 1024;

/// The file name an artifact is stored and served under: `<field><ext>`, with the extension
/// derived from the URL's path (`.mp3` default for audio, `.mp4` for video, `.jpg` for the
/// cover image). Bounded and sanitized: the served path is always our generated name, never
/// client input.
pub fn artifact_file_name(field: &str, url: &str) -> String {
    let extension = url
        .split('?')
        .next()
        .and_then(|path| path.rsplit('/').next())
        .and_then(|file| file.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()))
        .filter(|ext| !ext.is_empty() && ext.len() <= 8 && ext.bytes().all(|b| b.is_ascii_alphanumeric()));
    let extension = extension.unwrap_or_else(|| {
        match field {
            "video_url" => "mp4".to_string(),
            "image_url" => "jpg".to_string(),
            _ => "mp3".to_string(),
        }
    });
    format!("{field}.{extension}")
}

/// The on-disk location of one stored artifact.
pub fn artifact_path(artifact_dir: &Path, request_id: &str, file_name: &str) -> PathBuf {
    artifact_dir.join(request_id).join(file_name)
}

/// Download one artifact URL into the store with the discipline above. Returns the served file
/// name. A failure loses this artifact only — the caller records the class and continues with
/// the remaining fields.
pub async fn store_artifact(
    client: &wreq::Client,
    url: &str,
    artifact_dir: &Path,
    request_id: &str,
    field: &str,
) -> Result<String> {
    let file_name = artifact_file_name(field, url);
    let response = client
        .get(url)
        .send()
        .await
        .context("Suno artifact fetch transport failure")?;
    if !response.status().is_success() {
        anyhow::bail!("Suno artifact fetch returned HTTP {}", response.status().as_u16());
    }

    let task_dir = artifact_dir.join(request_id);
    tokio::fs::create_dir_all(&task_dir)
        .await
        .context("create Suno artifact directory")?;
    let tmp_path = task_dir.join(format!(".{file_name}.tmp"));
    let final_path = task_dir.join(&file_name);

    let write = async {
        let mut file = tokio::fs::File::create(&tmp_path).await?;
        let mut stream = response.bytes_stream();
        let mut total = 0usize;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Suno artifact stream failure")?;
            total = total.saturating_add(chunk.len());
            if total > ARTIFACT_MAX_BYTES {
                anyhow::bail!("Suno artifact exceeded the bound");
            }
            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
        }
        tokio::io::AsyncWriteExt::flush(&mut file).await?;
        file.sync_all().await?;
        anyhow::Ok(())
    };
    if let Err(error) = write.await {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(error.context("persist Suno artifact"));
    }
    finalize_file(&task_dir, &tmp_path, &final_path).await?;
    Ok(file_name)
}

/// Persist an in-memory payload (lyrics text) through the same tmp→fsync→rename discipline.
pub async fn store_payload(
    artifact_dir: &Path,
    request_id: &str,
    file_name: &str,
    bytes: &[u8],
) -> Result<String> {
    store_into_dir(&artifact_dir.join(request_id), file_name, bytes).await
}

/// Persist an in-memory payload into an exact directory (the uploads root uses this directly).
pub(crate) async fn store_into_dir(
    task_dir: &Path,
    file_name: &str,
    bytes: &[u8],
) -> Result<String> {
    if bytes.len() > ARTIFACT_MAX_BYTES {
        anyhow::bail!("Suno payload exceeded the artifact bound");
    }
    tokio::fs::create_dir_all(task_dir)
        .await
        .context("create Suno artifact directory")?;
    let tmp_path = task_dir.join(format!(".{file_name}.tmp"));
    let final_path = task_dir.join(file_name);
    let write = async {
        let mut file = tokio::fs::File::create(&tmp_path).await?;
        tokio::io::AsyncWriteExt::write_all(&mut file, bytes).await?;
        tokio::io::AsyncWriteExt::flush(&mut file).await?;
        file.sync_all().await?;
        anyhow::Ok(())
    };
    if let Err(error) = write.await {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(error.context("persist Suno payload"));
    }
    finalize_file(task_dir, &tmp_path, &final_path).await?;
    Ok(file_name.to_string())
}

/// Shared tail: private permissions, atomic rename, directory fsync.
async fn finalize_file(task_dir: &Path, tmp_path: &Path, final_path: &Path) -> Result<()> {
    // Private by construction: artifacts are paid customer content.
    tokio::fs::set_permissions(tmp_path, std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .await
        .context("chmod Suno artifact")?;
    tokio::fs::rename(tmp_path, final_path)
        .await
        .context("rename Suno artifact")?;
    // The rename is durable only once the directory entry is.
    let dir = std::fs::File::open(task_dir).context("open Suno artifact directory")?;
    tokio::task::spawn_blocking(move || dir.sync_all())
        .await
        .context("join Suno artifact dir sync")?
        .context("sync Suno artifact directory")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_derive_from_the_url_path_with_safe_defaults() {
        assert_eq!(
            artifact_file_name("audio_url", "https://cdn.example/x/song.mp3?sig=1"),
            "audio_url.mp3"
        );
        assert_eq!(
            artifact_file_name("audio_url", "https://cdn.example/x/song.wav"),
            "audio_url.wav"
        );
        assert_eq!(
            artifact_file_name("video_url", "https://cdn.example/x/v?sig=1"),
            "video_url.mp4"
        );
        assert_eq!(
            artifact_file_name("image_url", "https://cdn.example/x/cover"),
            "image_url.jpg"
        );
        // An extension outside the bounded charset/length falls back to the default.
        assert_eq!(
            artifact_file_name("audio_url", "https://cdn.example/x/a.toolongextension9"),
            "audio_url.mp3"
        );
    }

    #[test]
    fn store_payload_persists_through_the_durable_rename() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let mut random = [0u8; 8];
            getrandom::fill(&mut random).unwrap();
            let suffix = random.iter().map(|b| format!("{b:02x}")).collect::<String>();
            let root = std::env::temp_dir().join(format!("suno-artifacts-{suffix}"));
            let name = store_payload(&root, "req-1", "lyrics.txt", b"hello world")
                .await
                .unwrap();
            assert_eq!(name, "lyrics.txt");
            assert_eq!(
                std::fs::read(root.join("req-1").join("lyrics.txt")).unwrap(),
                b"hello world"
            );
            // Private by construction.
            let mode = std::fs::metadata(root.join("req-1").join("lyrics.txt"))
                .unwrap()
                .permissions();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(mode.mode() & 0o777, 0o600);
            }
            let _ = mode;
            std::fs::remove_dir_all(&root).unwrap();
        });
    }
}
