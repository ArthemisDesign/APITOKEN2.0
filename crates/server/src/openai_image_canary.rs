use anyhow::{bail, Context, Result};
use metering::{openai_image_tariff, GPT_IMAGE_2_ALIAS};
use serde::Serialize;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: u8 = 1;
const MAX_PROMPT_BYTES: usize = 512;
const MAX_PROMPT_CHARS: usize = 512;
const LOW_1024_OUTPUT_TOKENS: u64 = 196;
const LIVE_BLOCKERS: [&str; 4] = [
    "no_free_preflight",
    "spend_above_default_cap",
    "no_exact_green_sha",
    "reserve_ceiling_unproved",
];

pub(crate) struct OpenAiImageCanaryArgs {
    pub prompt_file: PathBuf,
    pub output: PathBuf,
    pub checkpoint: PathBuf,
    pub budget_nanousd: u64,
    pub execute: bool,
    pub model: String,
}

struct ValidatedCanary {
    model: String,
    canonical_model: &'static str,
    tariff_schedule: String,
    tariff_effective_from: i64,
    prompt_bytes: usize,
    prompt_chars: usize,
    proposed_budget_nanousd: u64,
    estimated_official_list_cost_nanousd: u64,
    execute: bool,
}

#[derive(Serialize)]
struct CanaryPlan<'a> {
    schema_version: u8,
    state: &'static str,
    executable: bool,
    blockers: &'static [&'static str],
    operation: &'static str,
    model: &'a str,
    canonical_model: &'static str,
    tariff_schedule: &'a str,
    tariff_effective_from: i64,
    implementation_sha: Option<&'static str>,
    proposed_budget_nanousd: u64,
    estimated_official_list_cost_nanousd: u64,
    prompt_bytes: usize,
    prompt_chars: usize,
    timestamp_unix_seconds: u64,
}

pub(crate) fn run(args: OpenAiImageCanaryArgs) -> Result<()> {
    let validated = validate(args)?;
    if validated.execute {
        bail!("GPT Image 2 live execution is blocked");
    }
    print_plan(&plan(&validated))
}

fn validate(args: OpenAiImageCanaryArgs) -> Result<ValidatedCanary> {
    #[cfg(not(unix))]
    bail!("openai image canary requires Unix file permission semantics");

    #[cfg(unix)]
    {
        let prompt = read_private_prompt(&args.prompt_file)?;
        validate_new_file(&args.output, "png", false)?;
        validate_new_file(&args.checkpoint, "json", true)?;
        if args.model != GPT_IMAGE_2_ALIAS {
            bail!("image canary model must be the reviewed gpt-image-2 alias");
        }

        let prompt_bytes = prompt.len();
        let prompt_chars = prompt.chars().count();
        let tariff = openai_image_tariff(&args.model)
            .map_err(|_| anyhow::anyhow!("image canary tariff identity is unavailable"))?;
        let estimated_official_list_cost_nanousd = estimated_official_list_cost(
            prompt_bytes,
            tariff.prices.fresh_text_input,
            tariff.prices.image_output,
        )?;
        if args.budget_nanousd < estimated_official_list_cost_nanousd {
            bail!("proposed image canary budget is below the official-list estimate");
        }

        Ok(ValidatedCanary {
            model: args.model,
            canonical_model: tariff.canonical_model_id,
            tariff_schedule: tariff.tariff_schedule_id.as_str().to_owned(),
            tariff_effective_from: tariff.schedule_effective_from,
            prompt_bytes,
            prompt_chars,
            proposed_budget_nanousd: args.budget_nanousd,
            estimated_official_list_cost_nanousd,
            execute: args.execute,
        })
    }
}

fn plan(validated: &ValidatedCanary) -> CanaryPlan<'_> {
    CanaryPlan {
        schema_version: SCHEMA_VERSION,
        state: "blocked",
        executable: false,
        blockers: &LIVE_BLOCKERS,
        operation: "generation",
        model: &validated.model,
        canonical_model: validated.canonical_model,
        tariff_schedule: &validated.tariff_schedule,
        tariff_effective_from: validated.tariff_effective_from,
        implementation_sha: None,
        proposed_budget_nanousd: validated.proposed_budget_nanousd,
        estimated_official_list_cost_nanousd: validated.estimated_official_list_cost_nanousd,
        prompt_bytes: validated.prompt_bytes,
        prompt_chars: validated.prompt_chars,
        timestamp_unix_seconds: unix_timestamp(),
    }
}

#[cfg(unix)]
fn read_private_prompt(path: &Path) -> Result<String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let before = std::fs::symlink_metadata(path)
        .map_err(|_| anyhow::anyhow!("prompt file is missing or inaccessible"))?;
    if before.file_type().is_symlink() || !before.is_file() {
        bail!("prompt file must be a regular non-symlink file");
    }
    if before.permissions().mode() & 0o7777 != 0o600 {
        bail!("prompt file must have exact mode 0600");
    }
    if before.len() > MAX_PROMPT_BYTES as u64 {
        bail!("prompt must be at most 512 UTF-8 bytes");
    }

    let file = File::open(path).map_err(|_| anyhow::anyhow!("prompt file could not be opened"))?;
    let opened = file
        .metadata()
        .map_err(|_| anyhow::anyhow!("prompt file metadata could not be read"))?;
    if !opened.is_file()
        || opened.dev() != before.dev()
        || opened.ino() != before.ino()
        || opened.permissions().mode() & 0o7777 != 0o600
    {
        bail!("prompt file changed during validation");
    }

    let mut bytes = Vec::with_capacity(MAX_PROMPT_BYTES + 1);
    file.take((MAX_PROMPT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow::anyhow!("prompt file could not be read"))?;
    if bytes.is_empty() || bytes.len() > MAX_PROMPT_BYTES {
        bail!("prompt must be nonempty and at most 512 UTF-8 bytes");
    }
    let prompt =
        String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("prompt must be valid UTF-8"))?;
    if prompt.chars().count() > MAX_PROMPT_CHARS {
        bail!("prompt must be at most 512 Unicode characters");
    }
    Ok(prompt)
}

fn validate_new_file(path: &Path, extension: &str, require_utf8_basename: bool) -> Result<()> {
    let basename = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("image canary target basename is missing"))?;
    if require_utf8_basename && basename.to_str().is_none() {
        bail!("image canary checkpoint basename must be valid UTF-8");
    }
    if path.extension().and_then(|value| value.to_str()) != Some(extension) {
        bail!("image canary target has an invalid required extension");
    }
    match std::fs::symlink_metadata(path) {
        Ok(_) => bail!("image canary target already exists"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => bail!("image canary target is inaccessible"),
    }

    let parent = path
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let metadata = std::fs::symlink_metadata(parent)
        .map_err(|_| anyhow::anyhow!("image canary target parent is missing or inaccessible"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("image canary target parent must be an actual non-symlink directory");
    }
    Ok(())
}

fn estimated_official_list_cost(
    prompt_bytes: usize,
    fresh_text_rate_nanousd: i128,
    image_output_rate_nanousd: i128,
) -> Result<u64> {
    let prompt_bytes = i128::try_from(prompt_bytes).context("prompt byte count overflow")?;
    let estimate = prompt_bytes
        .checked_mul(fresh_text_rate_nanousd)
        .and_then(|input| {
            i128::from(LOW_1024_OUTPUT_TOKENS)
                .checked_mul(image_output_rate_nanousd)
                .and_then(|output| input.checked_add(output))
        })
        .context("image canary official-list estimate overflow")?;
    u64::try_from(estimate).context("image canary official-list estimate is invalid")
}

fn print_plan(evidence: &CanaryPlan<'_>) -> Result<()> {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    serde_json::to_writer(&mut lock, evidence).context("serialize image canary plan")?;
    lock.write_all(b"\n").context("write image canary plan")?;
    Ok(())
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let sequence = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "claude-api-image-canary-test-{}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }

        fn prompt(&self, value: &[u8]) -> PathBuf {
            let path = self.path("prompt.txt");
            std::fs::write(&path, value).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            path
        }

        fn args(
            &self,
            prompt_file: PathBuf,
            execute: bool,
            budget_nanousd: u64,
        ) -> OpenAiImageCanaryArgs {
            OpenAiImageCanaryArgs {
                prompt_file,
                output: self.path("output.png"),
                checkpoint: self.path("checkpoint.json"),
                budget_nanousd,
                execute,
                model: GPT_IMAGE_2_ALIAS.to_owned(),
            }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn estimate_uses_prompt_bytes_and_fixed_low_output() {
        assert_eq!(
            estimated_official_list_cost(1, 5_000, 30_000).unwrap(),
            5_885_000
        );
        assert_eq!(
            estimated_official_list_cost(512, 5_000, 30_000).unwrap(),
            8_440_000
        );
        assert_eq!(
            estimated_official_list_cost("é".len(), 5_000, 30_000).unwrap(),
            5_890_000
        );
    }

    #[test]
    fn dry_run_creates_no_files_and_plan_is_explicitly_blocked() {
        let dir = TestDir::new();
        let prompt = dir.prompt("private prompt".as_bytes());
        let args = dir.args(prompt, false, 9_000_000);
        let output = args.output.clone();
        let checkpoint = args.checkpoint.clone();
        let validated = validate(args).unwrap();
        let value = serde_json::to_value(plan(&validated)).unwrap();

        assert_eq!(value["state"], "blocked");
        assert_eq!(value["executable"], false);
        assert_eq!(value["implementation_sha"], serde_json::Value::Null);
        assert_eq!(value["canonical_model"], "gpt-image-2-2026-04-21");
        assert_eq!(value["tariff_schedule"], "openai/gpt-image-2/2026-04-21/v1");
        assert_eq!(value["tariff_effective_from"], 0);
        assert_eq!(value["proposed_budget_nanousd"], 9_000_000);
        assert_eq!(
            value["blockers"],
            serde_json::json!([
                "no_free_preflight",
                "spend_above_default_cap",
                "no_exact_green_sha",
                "reserve_ceiling_unproved"
            ])
        );
        assert!(!output.exists());
        assert!(!checkpoint.exists());
    }

    #[test]
    fn execute_is_blocked_after_validation_without_creating_files() {
        let dir = TestDir::new();
        let prompt = dir.prompt(b"x");
        let args = dir.args(prompt, true, 9_000_000);
        let output = args.output.clone();
        let checkpoint = args.checkpoint.clone();
        let error = run(args).unwrap_err().to_string();
        assert_eq!(error, "GPT Image 2 live execution is blocked");
        assert!(!output.exists());
        assert!(!checkpoint.exists());
    }

    #[test]
    fn proposal_below_estimate_is_rejected_for_dry_run_and_execute() {
        for execute in [false, true] {
            let dir = TestDir::new();
            let prompt = dir.prompt(b"x");
            let args = dir.args(prompt, execute, 5_884_999);
            assert!(validate(args).is_err());
            assert!(!dir.path("output.png").exists());
            assert!(!dir.path("checkpoint.json").exists());
        }
    }

    #[test]
    fn prompt_requires_exact_0600_regular_non_symlink_utf8_file() {
        let dir = TestDir::new();
        let prompt = dir.prompt(b"ok");
        assert_eq!(read_private_prompt(&prompt).unwrap(), "ok");

        for mode in [0o640, 0o4600] {
            std::fs::set_permissions(&prompt, std::fs::Permissions::from_mode(mode)).unwrap();
            assert!(read_private_prompt(&prompt).is_err());
        }
        std::fs::set_permissions(&prompt, std::fs::Permissions::from_mode(0o600)).unwrap();

        let link = dir.path("prompt-link.txt");
        symlink(&prompt, &link).unwrap();
        assert!(read_private_prompt(&link).is_err());

        let invalid = dir.path("invalid.txt");
        std::fs::write(&invalid, [0xff]).unwrap();
        std::fs::set_permissions(&invalid, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_private_prompt(&invalid).is_err());

        let too_large = dir.path("too-large.txt");
        std::fs::write(&too_large, vec![b'x'; 513]).unwrap();
        std::fs::set_permissions(&too_large, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_private_prompt(&too_large).is_err());
    }

    #[test]
    fn target_constraints_reject_existing_symlink_and_unsafe_parent() {
        let dir = TestDir::new();
        assert!(validate_new_file(&dir.path("image.png"), "png", false).is_ok());
        assert!(validate_new_file(&dir.path("image.jpg"), "png", false).is_err());
        assert!(validate_new_file(&dir.path("checkpoint.txt"), "json", true).is_err());

        let existing = dir.path("existing.png");
        std::fs::write(&existing, b"occupied").unwrap();
        assert!(validate_new_file(&existing, "png", false).is_err());

        let target = dir.path("target.png");
        let link = dir.path("linked.png");
        symlink(&target, &link).unwrap();
        assert!(validate_new_file(&link, "png", false).is_err());

        let real_parent = dir.path("real-parent");
        std::fs::create_dir(&real_parent).unwrap();
        let linked_parent = dir.path("linked-parent");
        symlink(&real_parent, &linked_parent).unwrap();
        assert!(validate_new_file(&linked_parent.join("image.png"), "png", false).is_err());
        assert!(validate_new_file(&dir.path("missing/image.png"), "png", false).is_err());
    }

    #[test]
    fn checkpoint_basename_must_be_utf8() {
        let dir = TestDir::new();
        let invalid = dir
            .0
            .join(OsString::from_vec(b"checkpoint-\xff.json".to_vec()));
        assert!(validate_new_file(&invalid, "json", true).is_err());
    }

    #[test]
    fn plan_excludes_prompt_key_and_paths() {
        let dir = TestDir::new();
        let prompt_secret = "PRIVATE PROMPT";
        let key_secret = "PRIVATE-KEY-0123456789-abcdefghijklmnop";
        let prompt = dir.prompt(prompt_secret.as_bytes());
        let validated = validate(dir.args(prompt, false, 9_000_000)).unwrap();
        let json = serde_json::to_string(&plan(&validated)).unwrap();
        assert!(!json.contains(prompt_secret));
        assert!(!json.contains(key_secret));
        assert!(!json.contains(dir.0.to_string_lossy().as_ref()));
        assert!(!json.contains("output.png"));
        assert!(!json.contains("checkpoint.json"));
    }
}
