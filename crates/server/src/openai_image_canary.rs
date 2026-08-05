use crate::config::Settings;
use anyhow::{bail, Context, Result};
use forward::{
    CodexGateway, CodexImageError, CodexImageResult, ImageBackground, ImageEditRequest,
    ImageGenerationRequest, ImageQuality, ImageReference, ImageSize, ImageTurnId, GPT_IMAGE_2,
};
use serde::Serialize;
use serde_json::{Map, Value};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: u8 = 1;
const MAX_PROMPT_BYTES: usize = 512;
const MAX_PROMPT_CHARS: usize = 512;
const MAX_REFERENCE_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_CANARY_CAP_NANOUSD: u64 = 100_000;
const MAX_OUTPUT_EDGE: u32 = 3_840;
const MIN_OUTPUT_PIXELS: u64 = 655_360;
const MAX_OUTPUT_PIXELS: u64 = 8_294_400;
const LOW_OUTPUT_TOKEN_LIMIT: u64 = match low_output_tokens(2_880, 2_880) {
    Some(tokens) => tokens,
    None => panic!("valid GPT Image 2 maximum-token resolution"),
};
const TEXT_INPUT_NANOUSD_PER_TOKEN: u64 = 5_000;
const IMAGE_INPUT_NANOUSD_PER_TOKEN: u64 = 8_000;
const IMAGE_OUTPUT_NANOUSD_PER_TOKEN: u64 = 30_000;
const GENERATION_CEILING_NANOUSD: u64 = LOW_OUTPUT_TOKEN_LIMIT * IMAGE_OUTPUT_NANOUSD_PER_TOKEN
    + MAX_PROMPT_BYTES as u64 * TEXT_INPUT_NANOUSD_PER_TOKEN;
// OpenAI publishes no GPT Image 2 high-fidelity input-token formula. Its model page does publish
// 8,000,000 TPM as the highest tier, which is an absolute per-minute token admission envelope and
// therefore a conservative bound for one request. Charge the whole envelope at the more expensive
// fresh image-input rate, then add the independently bounded prompt and low output.
const MAX_PUBLISHED_MODEL_TPM: u64 = 8_000_000;
const EDIT_CEILING_NANOUSD: u64 =
    MAX_PUBLISHED_MODEL_TPM * IMAGE_INPUT_NANOUSD_PER_TOKEN + GENERATION_CEILING_NANOUSD;
const GENERATION_BUDGET_BLOCKER: &str = "paid_dispatch_requires_generation_ceiling_authorization";
const EDIT_BUDGET_BLOCKER: &str = "paid_dispatch_requires_edit_ceiling_authorization";
const EDIT_REFERENCE_BLOCKER: &str = "paid_dispatch_requires_exactly_one_reference";

pub(crate) struct OpenAiImageCanaryArgs {
    pub profile: Option<String>,
    pub prompt_file: PathBuf,
    pub references: Vec<PathBuf>,
    pub output: PathBuf,
    pub checkpoint: PathBuf,
    pub budget_nanousd: u64,
    pub execute: bool,
}

struct ValidatedCanary {
    profile: Option<String>,
    prompt: String,
    references: Vec<ImageReference>,
    output: ValidatedTarget,
    checkpoint: ValidatedTarget,
    run_dir: ValidatedRunDirectory,
    prompt_bytes: usize,
    prompt_chars: usize,
    reference_bytes: usize,
    authorization_budget_nanousd: u64,
    execute: bool,
}

struct ValidatedTarget {
    path: PathBuf,
    parent: PathBuf,
    parent_dev: u64,
    parent_ino: u64,
}

struct ValidatedRunDirectory {
    path: PathBuf,
    parent: PathBuf,
    parent_dev: u64,
    parent_ino: u64,
}

struct ActiveRunDirectory {
    path: PathBuf,
    dev: u64,
    ino: u64,
}

#[derive(Serialize)]
struct CanaryPlan<'a> {
    schema_version: u8,
    state: &'static str,
    executable: bool,
    execution_blocker: Option<&'static str>,
    operation: &'static str,
    profile: &'a str,
    model: &'static str,
    background: &'static str,
    quality: &'static str,
    size: &'static str,
    reference_count: usize,
    prompt_bytes: usize,
    prompt_chars: usize,
    reference_bytes: usize,
    authorization_budget_nanousd: u64,
    required_ceiling_nanousd: Option<u64>,
    repository_default_cap_nanousd: u64,
    implementation_sha: Option<&'static str>,
}

#[derive(Serialize)]
struct CanaryJournal<'a> {
    schema_version: u8,
    state: &'static str,
    profile: &'a str,
    operation: &'static str,
    model: &'static str,
    image_turn_id: &'a str,
    implementation_sha: &'a str,
    authorization_budget_nanousd: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    returned: Option<ReturnedEvidence<'a>>,
}

#[derive(Serialize)]
struct ReturnedEvidence<'a> {
    exact_home: bool,
    exact_turn: bool,
    width: u32,
    height: u32,
    created: u64,
    provider: ProviderMetadata<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<Value>,
    request_id: Option<String>,
    output_sha256: String,
}

#[derive(Serialize)]
struct CanaryCheckpoint<'a> {
    schema_version: u8,
    profile: &'a str,
    operation: &'static str,
    model: &'static str,
    image_turn_id: &'a str,
    width: u32,
    height: u32,
    created: u64,
    provider: ProviderMetadata<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<Value>,
    request_id: Option<String>,
    output_sha256: String,
    implementation_sha: &'a str,
    authorization_budget_nanousd: u64,
}

#[derive(Serialize)]
struct ProviderMetadata<'a> {
    background: &'a str,
    quality: &'a str,
    size: &'a str,
    output_format: Option<&'a str>,
}

pub(crate) fn run(args: OpenAiImageCanaryArgs) -> Result<()> {
    let validated = validate(args)?;
    if !validated.execute_requested() {
        return print_plan(&plan(&validated));
    }

    let (implementation_sha, settings) =
        execution_prerequisites(implementation_sha_source(), &validated, Settings::from_env)?;
    let codex = settings
        .codex
        .context("openai image canary requires the configured Codex provider")?;
    if !codex.enabled || !settings.provider.serves_openai() {
        bail!("openai image canary requires the configured OpenAI Codex provider");
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("create OpenAI image canary runtime")?;
    runtime.block_on(execute(validated, codex, implementation_sha))
}

impl ValidatedCanary {
    fn execute_requested(&self) -> bool {
        self.execute
    }
}

fn validate(args: OpenAiImageCanaryArgs) -> Result<ValidatedCanary> {
    #[cfg(not(unix))]
    bail!("openai image canary requires Unix file permission semantics");

    #[cfg(unix)]
    {
        if let Some(profile) = args.profile.as_deref() {
            codex_credential::validate_profile_id(profile)
                .context("image canary profile must be an opaque Codex profile id")?;
        }
        if args.references.len() > 5 {
            bail!("image canary accepts at most five PNG references");
        }
        if args.output == args.checkpoint {
            bail!("image canary output and checkpoint must be different paths");
        }

        validate_authorization_budget(args.budget_nanousd)?;
        let prompt = read_private_prompt(&args.prompt_file)?;
        let prompt_bytes = prompt.len();
        let prompt_chars = prompt.chars().count();
        let references = read_references(&args.references)?;
        let output = validate_new_target(&args.output, "png", false)?;
        let checkpoint = validate_new_target(&args.checkpoint, "json", true)?;
        let run_dir = validate_run_directory(&checkpoint)?;
        let reference_bytes = references
            .iter()
            .try_fold(0usize, |sum, image| sum.checked_add(image.bytes().len()))
            .context("image canary reference size overflow")?;

        Ok(ValidatedCanary {
            profile: args.profile,
            prompt,
            references,
            output,
            checkpoint,
            run_dir,
            prompt_bytes,
            prompt_chars,
            reference_bytes,
            authorization_budget_nanousd: args.budget_nanousd,
            execute: args.execute,
        })
    }
}

fn plan(validated: &ValidatedCanary) -> CanaryPlan<'_> {
    let (executable, execution_blocker, required_ceiling_nanousd) = operation_gate(validated);
    CanaryPlan {
        schema_version: SCHEMA_VERSION,
        state: if executable { "ready" } else { "blocked" },
        executable,
        execution_blocker,
        operation: operation(validated),
        profile: validated.profile.as_deref().unwrap_or("auto-admitted"),
        model: GPT_IMAGE_2,
        background: "opaque",
        quality: "low",
        size: "auto",
        reference_count: validated.references.len(),
        prompt_bytes: validated.prompt_bytes,
        prompt_chars: validated.prompt_chars,
        reference_bytes: validated.reference_bytes,
        authorization_budget_nanousd: validated.authorization_budget_nanousd,
        required_ceiling_nanousd,
        repository_default_cap_nanousd: DEFAULT_CANARY_CAP_NANOUSD,
        implementation_sha: implementation_sha_source().and_then(valid_implementation_sha),
    }
}

fn operation_gate(validated: &ValidatedCanary) -> (bool, Option<&'static str>, Option<u64>) {
    if validated.references.len() > 1 {
        return (
            false,
            Some(EDIT_REFERENCE_BLOCKER),
            Some(EDIT_CEILING_NANOUSD),
        );
    }
    let (ceiling, blocker) = if validated.references.is_empty() {
        (GENERATION_CEILING_NANOUSD, GENERATION_BUDGET_BLOCKER)
    } else {
        (EDIT_CEILING_NANOUSD, EDIT_BUDGET_BLOCKER)
    };
    if validated.authorization_budget_nanousd < ceiling {
        return (false, Some(blocker), Some(ceiling));
    }
    (true, None, Some(ceiling))
}

fn implementation_sha_source() -> Option<&'static str> {
    option_env!("CLAUDE_API_IMPLEMENTATION_SHA")
}

fn valid_implementation_sha(value: &str) -> Option<&str> {
    (value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')))
    .then_some(value)
}

fn require_implementation_sha(value: Option<&str>) -> Result<&str> {
    value
        .and_then(valid_implementation_sha)
        .context("openai image canary execution requires exact lowercase 40-hex CLAUDE_API_IMPLEMENTATION_SHA")
}

fn execution_prerequisites<'a, T>(
    implementation_sha: Option<&'a str>,
    validated: &ValidatedCanary,
    load_settings: impl FnOnce() -> T,
) -> Result<(&'a str, T)> {
    let implementation_sha = require_implementation_sha(implementation_sha)?;
    if let (_, Some(blocker), _) = operation_gate(validated) {
        bail!(blocker);
    }
    Ok((implementation_sha, load_settings()))
}

async fn execute(
    validated: ValidatedCanary,
    codex: forward::CodexConfig,
    implementation_sha: &str,
) -> Result<()> {
    let gateway = CodexGateway::new(codex).context("create Codex image gateway")?;
    let outcome = execute_with_gateway(&validated, &gateway, implementation_sha).await;
    gateway.shutdown().await;
    outcome
}

async fn execute_with_gateway(
    validated: &ValidatedCanary,
    gateway: &CodexGateway,
    implementation_sha: &str,
) -> Result<()> {
    let profile = match validated.profile.as_deref() {
        Some(profile) => profile.to_owned(),
        None => gateway
            .select_image_canary_home()
            .await
            .context("select admitted Codex image profile")?,
    };
    gateway
        .preflight_image_home(&profile)
        .await
        .context("exact-profile Codex image preflight")?;

    enum Request {
        Generation(ImageGenerationRequest),
        Edit(ImageEditRequest),
    }
    let request = if validated.references.is_empty() {
        Request::Generation(
            ImageGenerationRequest::new(validated.prompt.clone())
                .context("validate image generation request")?
                .with_controls(ImageBackground::Opaque, ImageQuality::Low, ImageSize::Auto),
        )
    } else {
        Request::Edit(
            ImageEditRequest::new(validated.prompt.clone(), validated.references.clone())
                .context("validate image edit request")?
                .with_controls(ImageBackground::Opaque, ImageQuality::Low, ImageSize::Auto),
        )
    };
    let image_turn_id = new_image_turn_id()?;
    let run_dir = create_run_directory(&validated.run_dir)?;
    persist_journal(
        &run_dir,
        validated,
        &profile,
        &image_turn_id,
        implementation_sha,
        "prepared",
        None,
    )?;

    let result = match &request {
        Request::Generation(request) => {
            gateway
                .generate_image_on_home(&profile, &image_turn_id, request)
                .await
        }
        Request::Edit(request) => {
            gateway
                .edit_image_on_home(&profile, &image_turn_id, request)
                .await
        }
    };
    match result {
        Ok(result) => persist_result(
            validated,
            &profile,
            &run_dir,
            &image_turn_id,
            implementation_sha,
            &result,
        ),
        Err(error) => {
            persist_journal(
                &run_dir,
                validated,
                &profile,
                &image_turn_id,
                implementation_sha,
                journal_state_for_error(&error),
                None,
            )?;
            Err(error).context("execute exact-home image operation")
        }
    }
}

fn new_image_turn_id() -> Result<ImageTurnId> {
    ImageTurnId::new(format!(
        "image-canary-{}-{}",
        std::process::id(),
        unix_timestamp_nanos()
    ))
    .context("create stable image turn id")
}

fn journal_state_for_error(error: &CodexImageError) -> &'static str {
    match error {
        CodexImageError::AuthenticationRequired(_)
        | CodexImageError::UsageLimit(_)
        | CodexImageError::BadRequest(_)
        | CodexImageError::Status(_)
        | CodexImageError::Validation(_)
        | CodexImageError::Unavailable => "rejected",
        CodexImageError::ResponseTimeout(_)
        | CodexImageError::ResponseBodyClosed(_)
        | CodexImageError::OutcomeUnknown(_)
        | CodexImageError::InvalidResponse(_) => "outcome_unknown",
    }
}

fn evidence_incomplete_state(
    exact_home: bool,
    exact_turn: bool,
    exact_metadata: bool,
    has_usage: bool,
) -> Option<&'static str> {
    if !exact_home {
        Some("evidence_home_mismatch")
    } else if !exact_turn {
        Some("evidence_turn_mismatch")
    } else if !exact_metadata {
        Some("evidence_controls_mismatch")
    } else if !has_usage {
        Some("evidence_usage_missing")
    } else {
        None
    }
}

fn valid_auto_output_dimensions(width: u32, height: u32) -> bool {
    let short = u64::from(width.min(height));
    let long = u64::from(width.max(height));
    let pixels = u64::from(width) * u64::from(height);
    short > 0
        && long <= u64::from(MAX_OUTPUT_EDGE)
        && pixels >= MIN_OUTPUT_PIXELS
        && pixels <= MAX_OUTPUT_PIXELS
        && long <= short * 3
}

fn valid_auto_output_size_metadata(size: &str, width: u32, height: u32) -> bool {
    size == "auto" || size == format!("{width}x{height}")
}

const fn low_output_tokens(width: u32, height: u32) -> Option<u64> {
    if width == 0
        || height == 0
        || width > MAX_OUTPUT_EDGE
        || height > MAX_OUTPUT_EDGE
        || width % 16 != 0
        || height % 16 != 0
    {
        return None;
    }
    let short = if width < height { width } else { height } as u64;
    let long = if width > height { width } else { height } as u64;
    let pixels = width as u64 * height as u64;
    if pixels < MIN_OUTPUT_PIXELS || pixels > MAX_OUTPUT_PIXELS || long > short * 3 {
        return None;
    }

    let short_grid = (16 * short + long / 2) / long;
    let grid_cells = 16 * short_grid;
    let numerator = grid_cells * (2_000_000 + pixels);
    Some((numerator + 3_999_999) / 4_000_000)
}

fn persist_result(
    validated: &ValidatedCanary,
    profile: &str,
    run_dir: &ActiveRunDirectory,
    image_turn_id: &ImageTurnId,
    implementation_sha: &str,
    result: &CodexImageResult,
) -> Result<()> {
    let usage = sanitize_usage(result.usage());
    let request_id = result.request_id().and_then(sanitize_request_id);
    let exact_home = result.home_id() == profile;
    let exact_turn = result.image_turn_id() == image_turn_id;
    let exact_metadata = valid_auto_output_dimensions(result.width(), result.height())
        && result.background() == "opaque"
        && result.quality() == "low"
        && valid_auto_output_size_metadata(result.size(), result.width(), result.height())
        && result.output_format().is_none_or(|format| format == "png");
    let output_sha256 = format!("sha256:{}", sha256_hex(result.png()));
    if let Some(state) =
        evidence_incomplete_state(exact_home, exact_turn, exact_metadata, usage.is_some())
    {
        let returned = ReturnedEvidence {
            exact_home,
            exact_turn,
            width: result.width(),
            height: result.height(),
            created: result.created(),
            provider: ProviderMetadata {
                background: result.background(),
                quality: result.quality(),
                size: result.size(),
                output_format: result.output_format(),
            },
            usage,
            request_id,
            output_sha256,
        };
        persist_journal(
            run_dir,
            validated,
            profile,
            image_turn_id,
            implementation_sha,
            state,
            Some(returned),
        )?;
        bail!("Codex image result did not provide complete exact canary evidence");
    }
    let checkpoint = CanaryCheckpoint {
        schema_version: SCHEMA_VERSION,
        profile,
        operation: operation(validated),
        model: GPT_IMAGE_2,
        image_turn_id: image_turn_id.as_str(),
        width: result.width(),
        height: result.height(),
        created: result.created(),
        provider: ProviderMetadata {
            background: result.background(),
            quality: result.quality(),
            size: result.size(),
            output_format: result.output_format(),
        },
        usage,
        request_id,
        output_sha256,
        implementation_sha,
        authorization_budget_nanousd: validated.authorization_budget_nanousd,
    };
    let mut checkpoint_bytes = serde_json::to_vec_pretty(&checkpoint)
        .context("serialize OpenAI image canary checkpoint")?;
    checkpoint_bytes.push(b'\n');

    persist_internal_artifact(run_dir, "result.png", result.png())?;
    persist_internal_artifact(run_dir, "checkpoint.json", &checkpoint_bytes)?;
    persist_journal(
        run_dir,
        validated,
        profile,
        image_turn_id,
        implementation_sha,
        "success",
        None,
    )?;
    publish_external_artifact(&validated.output, result.png(), "output")?;
    publish_external_artifact(&validated.checkpoint, &checkpoint_bytes, "checkpoint")
}

fn operation(validated: &ValidatedCanary) -> &'static str {
    if validated.references.is_empty() {
        "generation"
    } else {
        "edit"
    }
}

fn validate_authorization_budget(budget_nanousd: u64) -> Result<()> {
    if budget_nanousd <= DEFAULT_CANARY_CAP_NANOUSD {
        bail!(
            "image canary authorization budget must explicitly exceed the repository default cap of 100000 nanoUSD"
        );
    }
    Ok(())
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

#[cfg(unix)]
fn read_references(paths: &[PathBuf]) -> Result<Vec<ImageReference>> {
    use std::os::unix::fs::MetadataExt;

    let mut references = Vec::with_capacity(paths.len());
    for path in paths {
        let before = std::fs::symlink_metadata(path)
            .map_err(|_| anyhow::anyhow!("reference PNG is missing or inaccessible"))?;
        if before.file_type().is_symlink() || !before.is_file() {
            bail!("reference PNG must be a regular non-symlink file");
        }
        if before.len() == 0 || before.len() > MAX_REFERENCE_BYTES as u64 {
            bail!("reference PNG must be within 1..=16 MiB");
        }
        let file =
            File::open(path).map_err(|_| anyhow::anyhow!("reference PNG could not be opened"))?;
        let opened = file
            .metadata()
            .map_err(|_| anyhow::anyhow!("reference PNG metadata could not be read"))?;
        if !opened.is_file() || opened.dev() != before.dev() || opened.ino() != before.ino() {
            bail!("reference PNG changed during validation");
        }
        let mut bytes = Vec::with_capacity(before.len() as usize);
        file.take((MAX_REFERENCE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| anyhow::anyhow!("reference PNG could not be read"))?;
        if bytes.len() > MAX_REFERENCE_BYTES {
            bail!("reference PNG must not exceed 16 MiB");
        }
        references.push(ImageReference::new(bytes).context("validate strict reference PNG")?);
    }
    Ok(references)
}

#[cfg(unix)]
fn validate_new_target(
    path: &Path,
    extension: &str,
    require_utf8_basename: bool,
) -> Result<ValidatedTarget> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

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
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let metadata = std::fs::symlink_metadata(&parent)
        .map_err(|_| anyhow::anyhow!("image canary target parent is missing or inaccessible"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("image canary target parent must be an actual non-symlink directory");
    }
    if metadata.permissions().mode() & 0o022 != 0 {
        bail!("image canary target parent must not be group- or world-writable");
    }
    Ok(ValidatedTarget {
        path: path.to_path_buf(),
        parent,
        parent_dev: metadata.dev(),
        parent_ino: metadata.ino(),
    })
}

#[cfg(unix)]
fn validate_run_directory(checkpoint: &ValidatedTarget) -> Result<ValidatedRunDirectory> {
    let basename = checkpoint
        .path
        .file_name()
        .and_then(|value| value.to_str())
        .context("image canary checkpoint basename must be valid UTF-8")?;
    let path = checkpoint
        .parent
        .join(format!(".{basename}.openai-image-canary-run"));
    match std::fs::symlink_metadata(&path) {
        Ok(_) => bail!("image canary run directory already exists; inspect it for recovery"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => bail!("image canary run directory is inaccessible"),
    }
    Ok(ValidatedRunDirectory {
        path,
        parent: checkpoint.parent.clone(),
        parent_dev: checkpoint.parent_dev,
        parent_ino: checkpoint.parent_ino,
    })
}

#[cfg(unix)]
fn revalidate_parent(parent: &Path, expected_dev: u64, expected_ino: u64) -> Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let metadata = std::fs::symlink_metadata(parent)
        .map_err(|_| anyhow::anyhow!("image canary target parent became inaccessible"))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.dev() != expected_dev
        || metadata.ino() != expected_ino
        || metadata.permissions().mode() & 0o022 != 0
    {
        bail!("image canary target parent changed before persistence");
    }
    Ok(())
}

#[cfg(unix)]
fn revalidate_target(target: &ValidatedTarget) -> Result<()> {
    match std::fs::symlink_metadata(&target.path) {
        Ok(_) => bail!("image canary target appeared before persistence"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => bail!("image canary target became inaccessible"),
    }
    revalidate_parent(&target.parent, target.parent_dev, target.parent_ino)
}

#[cfg(unix)]
fn create_run_directory(validated: &ValidatedRunDirectory) -> Result<ActiveRunDirectory> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

    revalidate_parent(
        &validated.parent,
        validated.parent_dev,
        validated.parent_ino,
    )?;
    let mut builder = std::fs::DirBuilder::new();
    builder.mode(0o700);
    builder
        .create(&validated.path)
        .context("create exclusive image canary run directory")?;
    let metadata = std::fs::symlink_metadata(&validated.path)
        .context("read image canary run directory metadata")?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.permissions().mode() & 0o7777 != 0o700
    {
        bail!("image canary run directory does not have exact safe mode 0700");
    }
    sync_directory(&validated.parent)?;
    Ok(ActiveRunDirectory {
        path: validated.path.clone(),
        dev: metadata.dev(),
        ino: metadata.ino(),
    })
}

#[cfg(unix)]
fn revalidate_run_directory(run_dir: &ActiveRunDirectory) -> Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let metadata = std::fs::symlink_metadata(&run_dir.path)
        .context("read image canary run directory metadata")?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.dev() != run_dir.dev
        || metadata.ino() != run_dir.ino
        || metadata.permissions().mode() & 0o7777 != 0o700
    {
        bail!("image canary run directory changed");
    }
    Ok(())
}

#[cfg(unix)]
fn persist_journal<'a>(
    run_dir: &ActiveRunDirectory,
    validated: &ValidatedCanary,
    profile: &'a str,
    image_turn_id: &'a ImageTurnId,
    implementation_sha: &'a str,
    state: &'static str,
    returned: Option<ReturnedEvidence<'a>>,
) -> Result<()> {
    let journal = CanaryJournal {
        schema_version: SCHEMA_VERSION,
        state,
        profile,
        operation: operation(validated),
        model: GPT_IMAGE_2,
        image_turn_id: image_turn_id.as_str(),
        implementation_sha,
        authorization_budget_nanousd: validated.authorization_budget_nanousd,
        returned,
    };
    let mut bytes =
        serde_json::to_vec_pretty(&journal).context("serialize image canary journal")?;
    bytes.push(b'\n');
    persist_run_file(run_dir, "journal.json", &bytes, state != "prepared")
}

#[cfg(unix)]
fn persist_internal_artifact(run_dir: &ActiveRunDirectory, name: &str, bytes: &[u8]) -> Result<()> {
    persist_run_file(run_dir, name, bytes, false)
}

#[cfg(unix)]
fn persist_run_file(
    run_dir: &ActiveRunDirectory,
    name: &str,
    bytes: &[u8],
    replace: bool,
) -> Result<()> {
    revalidate_run_directory(run_dir)?;
    let target = run_dir.path.join(name);
    if !replace {
        match std::fs::symlink_metadata(&target) {
            Ok(_) => bail!("image canary internal artifact already exists"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => bail!("image canary internal artifact is inaccessible"),
        }
    }
    let temp = private_temp_in(&run_dir.path, name, bytes)?;
    if let Err(error) = std::fs::rename(&temp, &target) {
        let _ = std::fs::remove_file(&temp);
        return Err(error).context("atomically persist image canary run file");
    }
    sync_directory(&run_dir.path)
}

#[cfg(unix)]
fn private_temp_in(parent: &Path, basename: &str, bytes: &[u8]) -> Result<PathBuf> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    for attempt in 0..16u32 {
        let temp = parent.join(format!(
            ".{basename}.{}-{}-{attempt}.tmp",
            std::process::id(),
            unix_timestamp_nanos()
        ));
        let opened = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp);
        let mut file = match opened {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error).context("create private image canary temp file"),
        };
        let write_result = (|| -> Result<()> {
            std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600))
                .context("set private image canary temp permissions")?;
            file.write_all(bytes)
                .context("write private image canary temp file")?;
            file.sync_all()
                .context("sync private image canary temp file")
        })();
        if let Err(error) = write_result {
            drop(file);
            let _ = std::fs::remove_file(&temp);
            return Err(error);
        }
        return Ok(temp);
    }
    bail!("could not allocate a private image canary temp file")
}

#[cfg(unix)]
fn publish_external_artifact(target: &ValidatedTarget, bytes: &[u8], tag: &str) -> Result<()> {
    revalidate_target(target)?;
    let basename = target
        .path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("artifact");
    let temp = private_temp_in(&target.parent, basename, bytes)?;
    if let Err(error) = std::fs::hard_link(&temp, &target.path) {
        let _ = std::fs::remove_file(&temp);
        return Err(error).with_context(|| format!("publish image canary {tag} exclusively"));
    }
    std::fs::remove_file(&temp).context("remove image canary external temp link")?;
    sync_directory(&target.parent)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .context("sync image canary directory")
}

fn sanitize_request_id(value: &str) -> Option<String> {
    if value.is_empty() || value.len() > 128 {
        return None;
    }
    let sanitized: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ':') {
                character
            } else {
                '_'
            }
        })
        .collect();
    (!sanitized.is_empty()).then_some(sanitized)
}

fn sanitize_usage(usage: Option<&Value>) -> Option<Value> {
    const TOP_LEVEL: [&str; 5] = [
        "input_tokens",
        "input_tokens_details",
        "output_tokens",
        "output_tokens_details",
        "total_tokens",
    ];
    const DETAILS: [&str; 4] = [
        "text_tokens",
        "image_tokens",
        "cached_tokens",
        "total_tokens",
    ];

    let object = usage?.as_object()?;
    let mut safe = Map::new();
    for key in TOP_LEVEL {
        let Some(value) = object.get(key) else {
            continue;
        };
        if let Some(number) = value.as_u64() {
            safe.insert(key.to_owned(), Value::from(number));
        } else if key.ends_with("_details") {
            let Some(details) = value.as_object() else {
                continue;
            };
            let mut safe_details = Map::new();
            for detail in DETAILS {
                if let Some(number) = details.get(detail).and_then(Value::as_u64) {
                    safe_details.insert(detail.to_owned(), Value::from(number));
                }
            }
            if !safe_details.is_empty() {
                safe.insert(key.to_owned(), Value::Object(safe_details));
            }
        }
    }
    (!safe.is_empty()).then_some(Value::Object(safe))
}

fn print_plan(evidence: &CanaryPlan<'_>) -> Result<()> {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    serde_json::to_writer(&mut lock, evidence).context("serialize image canary plan")?;
    lock.write_all(b"\n").context("write image canary plan")?;
    Ok(())
}

fn unix_timestamp_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

fn sha256_hex(bytes: &[u8]) -> String {
    const INITIAL: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    let mut padded = bytes.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    let mut hash = INITIAL;
    for chunk in padded.chunks_exact(64) {
        let mut words = [0u32; 64];
        for (index, word) in words.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes(chunk[index * 4..index * 4 + 4].try_into().unwrap());
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7)
                ^ words[index - 15].rotate_right(18)
                ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17)
                ^ words[index - 2].rotate_right(19)
                ^ (words[index - 2] >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = hash;
        for index in 0..64 {
            let sigma1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(sigma1)
                .wrapping_add(choice)
                .wrapping_add(K[index])
                .wrapping_add(words[index]);
            let sigma0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = sigma0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (state, value) in hash.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *state = state.wrapping_add(value);
        }
    }
    hash.iter().map(|word| format!("{word:08x}")).collect()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::sync::atomic::{AtomicU64, Ordering};

    const EXACT_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
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
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
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

        fn args(&self, execute: bool, budget_nanousd: u64) -> OpenAiImageCanaryArgs {
            OpenAiImageCanaryArgs {
                profile: Some("opaque_profile-1".to_owned()),
                prompt_file: self.prompt(b"private prompt secret"),
                references: Vec::new(),
                output: self.path("output.png"),
                checkpoint: self.path("checkpoint.json"),
                budget_nanousd,
                execute,
            }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn file_mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o7777
    }

    #[test]
    fn ready_generation_plan_is_sanitized_and_creates_no_artifacts() {
        let dir = TestDir::new();
        let validated = validate(dir.args(false, GENERATION_CEILING_NANOUSD)).unwrap();
        let value = serde_json::to_value(plan(&validated)).unwrap();
        let serialized = serde_json::to_string(&value).unwrap();
        assert_eq!(value["state"], "ready");
        assert_eq!(value["executable"], true);
        assert!(value["execution_blocker"].is_null());
        assert_eq!(
            value["authorization_budget_nanousd"],
            GENERATION_CEILING_NANOUSD
        );
        assert_eq!(
            value["required_ceiling_nanousd"],
            GENERATION_CEILING_NANOUSD
        );
        assert_eq!(value["profile"], "opaque_profile-1");
        assert_eq!(value["model"], GPT_IMAGE_2);
        assert_eq!(value["operation"], "generation");
        assert_eq!(value["background"], "opaque");
        assert_eq!(value["quality"], "low");
        assert_eq!(value["size"], "auto");
        assert!(!serialized.contains("private prompt secret"));
        assert!(!serialized.contains(dir.0.to_string_lossy().as_ref()));
        assert!(!validated.run_dir.path.exists());
        assert!(!dir.path("output.png").exists());
        assert!(!dir.path("checkpoint.json").exists());
    }

    #[test]
    fn auto_profile_and_single_reference_edit_gate_are_explicit() {
        let dir = TestDir::new();
        let mut args = dir.args(false, GENERATION_CEILING_NANOUSD);
        args.profile = None;
        args.references.push(dir.prompt(b"not a png"));
        assert!(validate(args).is_err());

        let mut validated = validate(dir.args(false, GENERATION_CEILING_NANOUSD)).unwrap();
        validated.profile = None;
        assert_eq!(
            serde_json::to_value(plan(&validated)).unwrap()["profile"],
            "auto-admitted"
        );
        let png = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            b"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
        )
        .unwrap();
        validated
            .references
            .push(ImageReference::new(png.clone()).unwrap());
        let blocked = serde_json::to_value(plan(&validated)).unwrap();
        assert_eq!(blocked["state"], "blocked");
        assert_eq!(blocked["execution_blocker"], EDIT_BUDGET_BLOCKER);
        assert_eq!(blocked["required_ceiling_nanousd"], EDIT_CEILING_NANOUSD);

        validated.authorization_budget_nanousd = EDIT_CEILING_NANOUSD;
        let ready = serde_json::to_value(plan(&validated)).unwrap();
        assert_eq!(ready["state"], "ready");
        assert_eq!(ready["executable"], true);
        assert!(ready["execution_blocker"].is_null());
        assert_eq!(ready["required_ceiling_nanousd"], EDIT_CEILING_NANOUSD);
        assert_eq!(ready["operation"], "edit");
        assert_eq!(ready["reference_count"], 1);

        validated.references.push(ImageReference::new(png).unwrap());
        let multiple = serde_json::to_value(plan(&validated)).unwrap();
        assert_eq!(multiple["state"], "blocked");
        assert_eq!(multiple["execution_blocker"], EDIT_REFERENCE_BLOCKER);
        assert_eq!(multiple["required_ceiling_nanousd"], EDIT_CEILING_NANOUSD);
    }

    #[test]
    fn auto_output_contract_accepts_native_dimensions_and_rejects_unbounded_metadata() {
        assert!(valid_auto_output_dimensions(1_254, 1_254));
        assert!(valid_auto_output_dimensions(3_840, 2_160));
        assert!(!valid_auto_output_dimensions(0, 1_024));
        assert!(!valid_auto_output_dimensions(4_096, 2_048));
        assert!(!valid_auto_output_dimensions(3_840, 1_024));
        assert!(!valid_auto_output_dimensions(800, 800));
        assert!(valid_auto_output_size_metadata("auto", 1_254, 1_254));
        assert!(valid_auto_output_size_metadata("1254x1254", 1_254, 1_254));
        assert!(!valid_auto_output_size_metadata("1024x1024", 1_254, 1_254));
    }

    #[test]
    fn official_low_output_token_formula_is_bounded() {
        assert_eq!(low_output_tokens(1_024, 1_024), Some(196));
        assert_eq!(low_output_tokens(2_880, 2_880), Some(659));
        assert_eq!(low_output_tokens(3_840, 2_160), Some(371));
        assert_eq!(low_output_tokens(1_254, 1_254), None);
        assert_eq!(low_output_tokens(3_840, 1_024), None);

        let mut maximum = 0;
        for width in (16..=MAX_OUTPUT_EDGE).step_by(16) {
            for height in (16..=MAX_OUTPUT_EDGE).step_by(16) {
                if let Some(tokens) = low_output_tokens(width, height) {
                    maximum = maximum.max(tokens);
                }
            }
        }
        assert_eq!(maximum, LOW_OUTPUT_TOKEN_LIMIT);
        assert_eq!(GENERATION_CEILING_NANOUSD, 22_330_000);
        assert_eq!(EDIT_CEILING_NANOUSD, 64_022_330_000);
    }

    #[test]
    fn authorization_budget_must_exceed_repository_cap() {
        let dir = TestDir::new();
        assert!(validate(dir.args(true, DEFAULT_CANARY_CAP_NANOUSD)).is_err());
        assert!(validate(dir.args(true, DEFAULT_CANARY_CAP_NANOUSD + 1)).is_ok());
    }

    #[test]
    fn implementation_identity_accepts_only_exact_lowercase_sha() {
        assert_eq!(
            require_implementation_sha(Some(EXACT_SHA)).unwrap(),
            EXACT_SHA
        );
        assert!(require_implementation_sha(None).is_err());
        assert!(
            require_implementation_sha(Some("0123456789ABCDEF0123456789ABCDEF01234567")).is_err()
        );
        assert!(require_implementation_sha(Some(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ))
        .is_err());
    }

    #[test]
    fn identity_and_operation_gate_run_before_settings_loader() {
        let dir = TestDir::new();
        let ready = validate(dir.args(true, GENERATION_CEILING_NANOUSD)).unwrap();
        let loads = Cell::new(0u8);
        let missing = execution_prerequisites::<()>(None, &ready, || loads.set(loads.get() + 1));
        assert!(missing.is_err());
        assert_eq!(loads.get(), 0);

        let blocked = validate(dir.args(true, GENERATION_CEILING_NANOUSD - 1)).unwrap();
        let blocked =
            execution_prerequisites(Some(EXACT_SHA), &blocked, || loads.set(loads.get() + 1));
        assert!(blocked.is_err());
        assert_eq!(loads.get(), 0);

        assert!(execution_prerequisites(Some(EXACT_SHA), &ready, || {
            loads.set(loads.get() + 1)
        })
        .is_ok());
        assert_eq!(loads.get(), 1);
    }

    #[test]
    fn existing_run_directory_blocks_validation_even_when_empty() {
        let dir = TestDir::new();
        let validated = validate(dir.args(false, GENERATION_CEILING_NANOUSD)).unwrap();
        std::fs::create_dir(&validated.run_dir.path).unwrap();
        std::fs::set_permissions(
            &validated.run_dir.path,
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        assert!(validate(dir.args(false, GENERATION_CEILING_NANOUSD)).is_err());
    }

    #[test]
    fn journal_transitions_are_private_and_contain_no_request_secrets() {
        let dir = TestDir::new();
        let validated = validate(dir.args(true, GENERATION_CEILING_NANOUSD)).unwrap();
        let run_dir = create_run_directory(&validated.run_dir).unwrap();
        let turn_id = ImageTurnId::new("stable-image-turn-123").unwrap();
        assert_eq!(file_mode(&run_dir.path), 0o700);

        for state in ["prepared", "success", "rejected", "outcome_unknown"] {
            persist_journal(
                &run_dir,
                &validated,
                "opaque_profile-1",
                &turn_id,
                EXACT_SHA,
                state,
                None,
            )
            .unwrap();
            let journal_path = run_dir.path.join("journal.json");
            let bytes = std::fs::read(&journal_path).unwrap();
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            let serialized = String::from_utf8(bytes).unwrap();
            assert_eq!(value["state"], state);
            assert_eq!(value["profile"], "opaque_profile-1");
            assert_eq!(value["image_turn_id"], "stable-image-turn-123");
            assert_eq!(value["implementation_sha"], EXACT_SHA);
            assert_eq!(
                value["authorization_budget_nanousd"],
                GENERATION_CEILING_NANOUSD
            );
            assert!(value.get("returned").is_none());
            assert_eq!(file_mode(&journal_path), 0o600);
            assert!(!serialized.contains("private prompt secret"));
            assert!(!serialized.contains(dir.0.to_string_lossy().as_ref()));
            assert!(!serialized.contains("base64"));
            assert!(!serialized.contains("token"));
        }
    }

    #[test]
    fn mismatch_journal_retains_only_sanitized_returned_evidence() {
        let dir = TestDir::new();
        let validated = validate(dir.args(true, GENERATION_CEILING_NANOUSD)).unwrap();
        let run_dir = create_run_directory(&validated.run_dir).unwrap();
        let turn_id = ImageTurnId::new("stable-image-turn-123").unwrap();
        let returned = ReturnedEvidence {
            exact_home: true,
            exact_turn: true,
            width: 1_254,
            height: 1_254,
            created: 1_765_000_000,
            provider: ProviderMetadata {
                background: "auto",
                quality: "low",
                size: "1254x1254",
                output_format: Some("png"),
            },
            usage: sanitize_usage(Some(&serde_json::json!({
                "input_tokens": 5,
                "input_tokens_details": {"text_tokens": 5, "unsafe": "secret"},
                "output_tokens": 100,
                "private": "secret"
            }))),
            request_id: sanitize_request_id("request/unsafe 1"),
            output_sha256:
                "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned(),
        };

        persist_journal(
            &run_dir,
            &validated,
            "opaque_profile-1",
            &turn_id,
            EXACT_SHA,
            "evidence_controls_mismatch",
            Some(returned),
        )
        .unwrap();

        let journal_path = run_dir.path.join("journal.json");
        let bytes = std::fs::read(&journal_path).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let serialized = String::from_utf8(bytes).unwrap();
        assert_eq!(file_mode(&journal_path), 0o600);
        assert_eq!(value["returned"]["exact_home"], true);
        assert_eq!(value["returned"]["exact_turn"], true);
        assert_eq!(value["returned"]["width"], 1_254);
        assert_eq!(value["returned"]["height"], 1_254);
        assert_eq!(value["returned"]["created"], 1_765_000_000u64);
        assert_eq!(value["returned"]["provider"]["background"], "auto");
        assert_eq!(value["returned"]["provider"]["quality"], "low");
        assert_eq!(value["returned"]["provider"]["size"], "1254x1254");
        assert_eq!(value["returned"]["provider"]["output_format"], "png");
        assert_eq!(value["returned"]["usage"]["input_tokens"], 5);
        assert_eq!(value["returned"]["usage"]["output_tokens"], 100);
        assert_eq!(value["returned"]["request_id"], "request_unsafe_1");
        assert_eq!(
            value["returned"]["output_sha256"],
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
        assert!(!serialized.contains("private prompt secret"));
        assert!(!serialized.contains(dir.0.to_string_lossy().as_ref()));
        assert!(!serialized.contains("unsafe\":\"secret"));
        assert!(!serialized.contains("private"));
        assert!(!serialized.contains("base64"));
        assert!(!run_dir.path.join("result.png").exists());
        assert!(!run_dir.path.join("checkpoint.json").exists());
        assert!(!dir.path("output.png").exists());
        assert!(!dir.path("checkpoint.json").exists());
    }

    #[test]
    fn error_classification_separates_rejection_from_ambiguous_outcome() {
        assert_eq!(
            journal_state_for_error(&CodexImageError::Validation("invalid")),
            "rejected"
        );
        assert_eq!(
            journal_state_for_error(&CodexImageError::Unavailable),
            "rejected"
        );
        assert_eq!(
            journal_state_for_error(&CodexImageError::ResponseTimeout(None)),
            "outcome_unknown"
        );
        assert_eq!(
            journal_state_for_error(&CodexImageError::OutcomeUnknown(None)),
            "outcome_unknown"
        );
    }

    #[test]
    fn provider_request_id_is_optional_but_exact_evidence_is_not() {
        assert_eq!(evidence_incomplete_state(true, true, true, true), None);
        assert_eq!(
            evidence_incomplete_state(false, true, true, true),
            Some("evidence_home_mismatch")
        );
        assert_eq!(
            evidence_incomplete_state(true, false, true, true),
            Some("evidence_turn_mismatch")
        );
        assert_eq!(
            evidence_incomplete_state(true, true, false, true),
            Some("evidence_controls_mismatch")
        );
        assert_eq!(
            evidence_incomplete_state(true, true, true, false),
            Some("evidence_usage_missing")
        );
    }

    #[test]
    fn internal_recovery_precedes_separate_exclusive_external_publication() {
        let dir = TestDir::new();
        let validated = validate(dir.args(true, GENERATION_CEILING_NANOUSD)).unwrap();
        let run_dir = create_run_directory(&validated.run_dir).unwrap();
        persist_internal_artifact(&run_dir, "result.png", b"png").unwrap();
        persist_internal_artifact(&run_dir, "checkpoint.json", b"{}\n").unwrap();
        assert_eq!(
            std::fs::read(run_dir.path.join("result.png")).unwrap(),
            b"png"
        );
        assert_eq!(
            std::fs::read(run_dir.path.join("checkpoint.json")).unwrap(),
            b"{}\n"
        );
        assert_eq!(file_mode(&run_dir.path.join("result.png")), 0o600);
        assert_eq!(file_mode(&run_dir.path.join("checkpoint.json")), 0o600);

        publish_external_artifact(&validated.output, b"png", "output").unwrap();
        publish_external_artifact(&validated.checkpoint, b"{}\n", "checkpoint").unwrap();
        assert_eq!(file_mode(&validated.output.path), 0o600);
        assert_eq!(file_mode(&validated.checkpoint.path), 0o600);
        assert!(publish_external_artifact(&validated.output, b"new", "output").is_err());
        assert_eq!(std::fs::read(&validated.output.path).unwrap(), b"png");
        assert_eq!(
            std::fs::read(run_dir.path.join("result.png")).unwrap(),
            b"png"
        );
    }

    #[test]
    fn prompt_and_targets_enforce_private_safe_filesystem_contract() {
        let dir = TestDir::new();
        let prompt = dir.prompt(b"ok");
        assert_eq!(read_private_prompt(&prompt).unwrap(), "ok");
        std::fs::set_permissions(&prompt, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(read_private_prompt(&prompt).is_err());
        std::fs::set_permissions(&prompt, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = dir.path("prompt-link.txt");
        symlink(&prompt, &link).unwrap();
        assert!(read_private_prompt(&link).is_err());

        assert!(validate_new_target(&dir.path("output.png"), "png", false).is_ok());
        let existing = dir.path("existing.png");
        std::fs::write(&existing, b"occupied").unwrap();
        assert!(validate_new_target(&existing, "png", false).is_err());
        let unsafe_parent = dir.path("unsafe");
        std::fs::create_dir(&unsafe_parent).unwrap();
        std::fs::set_permissions(&unsafe_parent, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(validate_new_target(&unsafe_parent.join("output.png"), "png", false).is_err());
    }

    #[test]
    fn usage_and_request_id_are_privacy_sanitized() {
        let usage = serde_json::json!({
            "input_tokens": 10,
            "input_tokens_details": {"text_tokens": 3, "secret": "token"},
            "account": "private@example.com"
        });
        assert_eq!(
            sanitize_usage(Some(&usage)).unwrap(),
            serde_json::json!({
                "input_tokens": 10,
                "input_tokens_details": {"text_tokens": 3}
            })
        );
        assert_eq!(
            sanitize_request_id("req/unsafe"),
            Some("req_unsafe".to_owned())
        );
    }

    #[test]
    fn sha256_matches_standard_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
