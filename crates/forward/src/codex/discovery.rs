//! Discovery of authenticated Codex profiles on disk.
//!
//! The Claude fleet grows by writing a subscription to the registry and letting the engine reload
//! it. A ChatGPT subscription cannot work that way: `codex login` does not yield a token we may
//! store, it writes an auth store into a `CODEX_HOME` directory, and this gateway deliberately
//! never reads or replays what is inside. So the unit that grows the Codex pool is a *directory*,
//! and this module is the equivalent of that reload: it turns a directory tree into the list of
//! profiles the pool may route to.
//!
//! Nothing here reads the auth store. It looks only at whether one exists.

use super::CodexHomeSpec;
use std::path::Path;

/// Marker written by `codex login`. Its presence is what distinguishes a finished purchase from a
/// directory the authbot created moments ago and has not authenticated yet.
const AUTH_STORE: &str = "auth.json";
/// Optional per-home egress, written by whoever provisioned the account.
const PROXY_FILE: &str = "proxy.url";
/// Host-supervisor fence used while one daemon drains before a rolling restart.
const DAEMON_DRAIN_FILE: &str = ".app-server-draining";
const DAEMON_DRAIN_SENTINEL: &str = "openai-bluegreen-drain-v1";

/// Stable, non-identifying id for a home directory.
///
/// Metric labels and logs must not carry an account identity, and a positional index is not stable
/// once the pool grows or shrinks. A short digest of the directory name is both: constant across
/// restarts, unaffected by its neighbours, and meaningless to anyone reading the metrics.
pub(crate) fn home_id(path: &str) -> String {
    let name = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path);
    format!("{:08x}", pool::stable_hash64(name.as_bytes()) as u32)
}

/// Read a home's dedicated proxy, if it has one.
///
/// A proxy URL routinely embeds credentials, so it is treated as a secret: the value is never
/// logged, and a world-readable file is refused rather than used. Refusing is safe here because it
/// costs one home in a pool, while silently egressing an account through a proxy whose credentials
/// leaked to every local user is not recoverable.
fn read_proxy(dir: &Path) -> Result<Option<String>, String> {
    let path = dir.join(PROXY_FILE);
    let meta = match std::fs::metadata(&path) {
        Ok(meta) => meta,
        Err(_) => return Ok(None),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err("proxy file is readable by other users".to_string());
        }
    }
    let value =
        std::fs::read_to_string(&path).map_err(|_| "proxy file is unreadable".to_string())?;
    let value = value.trim().to_string();
    if value.is_empty() {
        return Ok(None);
    }
    // Only the standard proxy schemes reach the child; anything else would be silently ignored by
    // the transport and would look like a working egress while it was not.
    let scheme_ok = ["http://", "https://", "socks5://", "socks5h://"]
        .iter()
        .any(|scheme| value.starts_with(scheme));
    if !scheme_ok {
        return Err("proxy file does not name a supported scheme".to_string());
    }
    Ok(Some(value))
}

/// Whether a directory holds a completed login.
fn is_authenticated(dir: &Path) -> bool {
    dir.join(AUTH_STORE).is_file()
}

/// Fail closed while the host supervisor drains this home. The hot-path check prevents a request
/// from racing with the signal-driven rediscovery; malformed or exposed markers also drain rather
/// than accidentally admitting work during an uncertain ownership transition.
pub(crate) fn daemon_draining(dir: &Path) -> bool {
    let path = dir.join(DAEMON_DRAIN_FILE);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(_) => return true,
    };
    if !meta.file_type().is_file() {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return true;
        }
    }
    std::fs::read_to_string(path)
        .map(|value| value.trim() == DAEMON_DRAIN_SENTINEL)
        .unwrap_or(true)
}

/// Build one spec from an explicitly configured home path.
///
/// Explicit homes are trusted to exist: they are named by an operator, so a missing auth store is
/// reported by preflight and the health probe rather than silently dropping the home.
pub(crate) fn explicit_spec(path: &str, inspect_proxy: bool) -> CodexHomeSpec {
    let proxy = if inspect_proxy {
        read_proxy(Path::new(path)).unwrap_or_else(|reason| {
            eprintln!(
                "Codex home {} ignoring its proxy file: {reason}",
                home_id(path)
            );
            None
        })
    } else {
        None
    };
    CodexHomeSpec {
        path: path.to_string(),
        id: home_id(path),
        proxy,
    }
}

/// Scan `dir` for authenticated profiles, in a deterministic order.
///
/// A missing directory is not an error: it simply means no account has been purchased yet. An
/// unreadable entry, or one whose proxy secret is exposed, is skipped with a diagnostic that names
/// the home only by its non-identifying id.
pub(crate) fn scan(dir: &str, inspect_proxy: bool) -> Vec<CodexHomeSpec> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            eprintln!("Codex homes directory is unreadable [{}]", error.kind());
            return Vec::new();
        }
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match path.file_name().and_then(|name| name.to_str()) {
            Some(name) if !name.starts_with('.') => name.to_string(),
            _ => continue,
        };
        if !is_authenticated(&path) {
            // A purchase in progress: the directory exists, the device flow has not completed.
            // Admitting it would fail every request routed to it and page for an account nobody
            // has finished buying.
            continue;
        }
        if !inspect_proxy && daemon_draining(&path) {
            continue;
        }
        let Some(path_str) = path.to_str().map(str::to_string) else {
            continue;
        };
        let proxy = match inspect_proxy {
            false => None,
            true => match read_proxy(&path) {
                Ok(proxy) => proxy,
                Err(reason) => {
                    eprintln!(
                        "Codex home {} excluded from the pool: {reason}",
                        home_id(&path_str)
                    );
                    continue;
                }
            },
        };
        found.push((
            name,
            CodexHomeSpec {
                id: home_id(&path_str),
                path: path_str,
                proxy,
            },
        ));
    }
    // Deterministic order keeps selection and any positional reporting reproducible across restarts.
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found.into_iter().map(|(_, spec)| spec).collect()
}

/// The complete set of profiles: explicitly configured ones first, then discovered ones.
///
/// A path configured explicitly and also present in the scanned directory is kept once, so moving
/// the original single-home deployment under the pool directory cannot double-count its capacity.
pub(crate) fn discover(
    homes: &[String],
    homes_dir: Option<&str>,
    inspect_proxy: bool,
) -> Vec<CodexHomeSpec> {
    let mut specs: Vec<CodexHomeSpec> = homes
        .iter()
        .filter(|path| inspect_proxy || !daemon_draining(Path::new(path)))
        .map(|path| explicit_spec(path, inspect_proxy))
        .collect();
    if let Some(dir) = homes_dir {
        for spec in scan(dir, inspect_proxy) {
            if !specs.iter().any(|existing| existing.path == spec.path) {
                specs.push(spec);
            }
        }
    }
    specs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    struct Dir(std::path::PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn workspace() -> Dir {
        let path = std::env::temp_dir().join(format!(
            "codex-discovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Dir(path)
    }

    fn home(root: &Path, name: &str, authenticated: bool) -> std::path::PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        if authenticated {
            std::fs::write(dir.join(AUTH_STORE), "{}").unwrap();
        }
        dir
    }

    #[cfg(unix)]
    fn write_proxy(dir: &Path, value: &str, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(PROXY_FILE);
        std::fs::write(&path, value).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn only_authenticated_directories_join_the_pool() {
        let ws = workspace();
        home(&ws.0, "bought", true);
        home(&ws.0, "in-progress", false);
        std::fs::write(ws.0.join("loose-file"), "x").unwrap();
        let found = scan(ws.0.to_str().unwrap(), true);
        assert_eq!(found.len(), 1);
        assert!(found[0].path.ends_with("bought"));
    }

    #[test]
    fn a_missing_directory_is_an_empty_pool_not_an_error() {
        assert!(scan("/nonexistent/codex/homes", true).is_empty());
    }

    #[test]
    fn discovery_is_deterministic_and_ids_are_stable_and_non_identifying() {
        let ws = workspace();
        home(&ws.0, "b-account", true);
        home(&ws.0, "a-account", true);
        let first = scan(ws.0.to_str().unwrap(), true);
        let second = scan(ws.0.to_str().unwrap(), true);
        assert_eq!(first, second);
        assert!(first[0].path.ends_with("a-account"));
        // Adding a home must not change an existing home's id, or every dashboard and alert would
        // re-label whenever an account is purchased.
        home(&ws.0, "aa-inserted", true);
        let third = scan(ws.0.to_str().unwrap(), true);
        let a_before = &first[0].id;
        let a_after = &third
            .iter()
            .find(|s| s.path.ends_with("a-account"))
            .unwrap()
            .id;
        assert_eq!(a_before, a_after);
        for spec in &third {
            assert_eq!(spec.id.len(), 8);
            assert!(!spec.id.contains("account"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_world_readable_proxy_secret_takes_the_home_out_of_rotation() {
        let ws = workspace();
        let dir = home(&ws.0, "leaky", true);
        write_proxy(&dir, "http://user:pass@example.test:8080", 0o644);
        assert!(scan(ws.0.to_str().unwrap(), true).is_empty());

        write_proxy(&dir, "http://user:pass@example.test:8080", 0o600);
        let found = scan(ws.0.to_str().unwrap(), true);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].proxy.as_deref(),
            Some("http://user:pass@example.test:8080")
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_unsupported_proxy_scheme_is_refused_rather_than_silently_ignored() {
        let ws = workspace();
        let dir = home(&ws.0, "weird", true);
        write_proxy(&dir, "ftp://example.test:21", 0o600);
        assert!(scan(ws.0.to_str().unwrap(), true).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn an_empty_proxy_file_means_the_shared_egress() {
        let ws = workspace();
        let dir = home(&ws.0, "plain", true);
        write_proxy(&dir, "\n", 0o600);
        let found = scan(ws.0.to_str().unwrap(), true);
        assert_eq!(found.len(), 1);
        assert!(found[0].proxy.is_none());
    }

    #[test]
    fn an_explicit_home_is_never_counted_twice_when_it_also_lives_in_the_pool_directory() {
        let ws = workspace();
        let dir = home(&ws.0, "shared", true);
        let explicit = vec![dir.to_str().unwrap().to_string()];
        let all = discover(&explicit, Some(ws.0.to_str().unwrap()), true);
        assert_eq!(all.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn shared_daemon_discovery_does_not_reparse_the_daemons_proxy_secret() {
        let ws = workspace();
        let dir = home(&ws.0, "daemon-owned", true);
        write_proxy(&dir, "disabled://blue-green-transition", 0o600);

        let found = scan(ws.0.to_str().unwrap(), false);
        assert_eq!(found.len(), 1);
        assert!(found[0].proxy.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn shared_daemon_drain_marker_excludes_explicit_and_scanned_homes() {
        use std::os::unix::fs::PermissionsExt;

        let ws = workspace();
        let drained = home(&ws.0, "drained", true);
        let ready = home(&ws.0, "ready", true);
        let marker = drained.join(DAEMON_DRAIN_FILE);
        std::fs::write(&marker, format!("{DAEMON_DRAIN_SENTINEL}\n")).unwrap();
        std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o600)).unwrap();

        let explicit = vec![drained.to_str().unwrap().to_string()];
        let found = discover(&explicit, Some(ws.0.to_str().unwrap()), false);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, ready.to_str().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn malformed_drain_marker_fails_closed() {
        use std::os::unix::fs::PermissionsExt;

        let ws = workspace();
        let drained = home(&ws.0, "drained", true);
        let marker = drained.join(DAEMON_DRAIN_FILE);
        std::fs::write(&marker, "unexpected\n").unwrap();
        std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o644)).unwrap();

        assert!(scan(ws.0.to_str().unwrap(), false).is_empty());
    }
}
