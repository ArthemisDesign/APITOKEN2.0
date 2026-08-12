//! Artifact store for the Tripo3D (VAST / Holymolly) plane.
//!
//! Upstream result URLs expire in ≤60 s (conflicting official sources; conservative reading,
//! `docs/engine/TRIPO3D_PROVIDER.md` §5.4), so a finalized successful task's artifacts are
//! downloaded IMMEDIATELY into the plane's own storage and customers are served from there —
//! the upstream signed URL never crosses the plane boundary.
//!
//! Persistence discipline: every artifact is streamed to a temporary sibling file bounded by
//! [`ARTIFACT_MAX_BYTES`], fsynced, mode-0600, then atomically renamed into
//! `<artifact_dir>/<request_id>/<name>` and the directory is fsynced. A reader therefore only
//! ever sees a complete file or nothing, and a crash mid-write leaves a tmp orphan, never a
//! half artifact under a served name.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use futures_util::StreamExt;

/// Per-artifact bound. 3D models with textures are tens of MB; 512 MiB is the documented
/// conservative ceiling (ours, not the provider's — the provider publishes none).
pub const ARTIFACT_MAX_BYTES: usize = 512 * 1024 * 1024;

/// The file name an artifact is stored and served under: `<field><ext>`, with the extension
/// derived from the signed URL's path (`.glb` default for model fields, `.jpg` for the preview
/// image — the SDK's own rule). Bounded and sanitized: the served path is always our generated
/// name, never client input.
pub fn artifact_file_name(field: &str, url: &str) -> String {
    let extension = url
        .split('?')
        .next()
        .and_then(|path| path.rsplit('/').next())
        .and_then(|file| file.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()))
        .filter(|ext| !ext.is_empty() && ext.len() <= 8 && ext.bytes().all(|b| b.is_ascii_alphanumeric()));
    let extension = extension.unwrap_or_else(|| {
        if field == "rendered_image" {
            "jpg".to_string()
        } else {
            "glb".to_string()
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
        .context("Tripo3D artifact fetch transport failure")?;
    if !response.status().is_success() {
        anyhow::bail!("Tripo3D artifact fetch returned HTTP {}", response.status().as_u16());
    }

    let task_dir = artifact_dir.join(request_id);
    tokio::fs::create_dir_all(&task_dir)
        .await
        .context("create Tripo3D artifact directory")?;
    let tmp_path = task_dir.join(format!(".{file_name}.tmp"));
    let final_path = task_dir.join(&file_name);

    let write = async {
        let mut file = tokio::fs::File::create(&tmp_path).await?;
        let mut stream = response.bytes_stream();
        let mut total = 0usize;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Tripo3D artifact stream failure")?;
            total = total.saturating_add(chunk.len());
            if total > ARTIFACT_MAX_BYTES {
                anyhow::bail!("Tripo3D artifact exceeded the bound");
            }
            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
        }
        tokio::io::AsyncWriteExt::flush(&mut file).await?;
        file.sync_all().await?;
        anyhow::Ok(())
    };
    if let Err(error) = write.await {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(error.context("persist Tripo3D artifact"));
    }
    // Private by construction: artifacts are paid customer content.
    tokio::fs::set_permissions(&tmp_path, std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .await
        .context("chmod Tripo3D artifact")?;
    tokio::fs::rename(&tmp_path, &final_path)
        .await
        .context("rename Tripo3D artifact")?;
    // The rename is durable only once the directory entry is.
    let dir = std::fs::File::open(&task_dir).context("open Tripo3D artifact directory")?;
    tokio::task::spawn_blocking(move || dir.sync_all())
        .await
        .context("join Tripo3D artifact dir sync")?
        .context("sync Tripo3D artifact directory")?;
    Ok(file_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_derive_from_the_url_path_with_safe_defaults() {
        assert_eq!(
            artifact_file_name("model", "https://cdn.example/x/model.glb?sig=1"),
            "model.glb"
        );
        assert_eq!(
            artifact_file_name("pbr_model", "https://cdn.example/x/pbr.GLB?sig=1"),
            "pbr_model.glb"
        );
        assert_eq!(
            artifact_file_name("rendered_image", "https://cdn.example/x/r?sig=1"),
            "rendered_image.jpg"
        );
        assert_eq!(artifact_file_name("model", "https://cdn.example/noext"), "model.glb");
        // An extension outside the bounded charset/length falls back to the default.
        assert_eq!(
            artifact_file_name("model", "https://cdn.example/x/m.toolongextension9"),
            "model.glb"
        );
    }
}
