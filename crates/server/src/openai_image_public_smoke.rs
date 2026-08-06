use anyhow::{bail, Context, Result};
use base64::Engine as _;
use forward::ImageReference;
use reqwest::{header::HeaderMap, Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::config::openai_image_public_smoke_database_url;

const SCHEMA_VERSION: u32 = 1;
const MODEL: &str = "gpt-image-2";
const DATED_MODEL: &str = "gpt-image-2-2026-04-21";
const PUBLIC_ORIGIN: &str = "https://openai.api.apitoken.sale";
const MAX_RESPONSE_BYTES: usize = 24 * 1024 * 1024;
const SETTLEMENT_WAIT: Duration = Duration::from_secs(150);
const SETTLEMENT_POLL: Duration = Duration::from_millis(500);

pub(crate) struct OpenAiImagePublicSmokeArgs {
    pub output: PathBuf,
    pub preflight_only: bool,
    pub execute: bool,
}

#[derive(Serialize)]
struct SmokePlan<'a> {
    schema_version: u32,
    state: &'static str,
    origin: &'a str,
    model: &'a str,
    operations: [&'static str; 2],
    controls: Value,
    implementation_sha: Option<&'static str>,
}

#[derive(Serialize)]
struct Journal<'a> {
    schema_version: u32,
    state: &'a str,
    implementation_sha: &'a str,
    generation_dispatched: bool,
    edit_dispatched: bool,
    generation_request_id: Option<&'a str>,
    edit_request_id: Option<&'a str>,
}

#[derive(Deserialize, Serialize)]
struct ImageResponse {
    data: Vec<ImageData>,
    usage: PublicUsage,
}

#[derive(Deserialize, Serialize)]
struct ImageData {
    b64_json: String,
}

#[derive(Deserialize, Serialize)]
struct PublicUsage {
    input_tokens: u64,
    input_tokens_details: InputTokenDetails,
    output_tokens: u64,
    output_tokens_details: OutputTokenDetails,
    total_tokens: u64,
}

#[derive(Deserialize, Serialize)]
struct InputTokenDetails {
    text_tokens: u64,
    image_tokens: u64,
}

#[derive(Deserialize, Serialize)]
struct OutputTokenDetails {
    image_tokens: u64,
}

#[derive(Serialize)]
struct OperationEvidence {
    request_id: String,
    width: u32,
    height: u32,
    png_bytes: usize,
    png_sha256: String,
    usage: PublicUsage,
    settlement: registry::pg::OpenAiImageSettlementEvidence,
}

#[derive(Serialize)]
struct SmokeEvidence<'a> {
    schema_version: u32,
    state: &'static str,
    implementation_sha: &'a str,
    origin: &'a str,
    model: &'a str,
    discovery_hidden_before_publication: bool,
    generation: OperationEvidence,
    edit: OperationEvidence,
    account_balance_unchanged: bool,
    account_spent_unchanged: bool,
    account_reserved_unchanged: bool,
    key_spent_unchanged: bool,
    key_reserved_unchanged: bool,
}

struct CompletedImage {
    request_id: String,
    png: Vec<u8>,
    width: u32,
    height: u32,
    usage: PublicUsage,
}

pub(crate) fn run(args: OpenAiImagePublicSmokeArgs) -> Result<()> {
    if args.preflight_only && args.execute {
        bail!("public image smoke modes are mutually exclusive");
    }
    if !args.preflight_only && !args.execute {
        let plan = SmokePlan {
            schema_version: SCHEMA_VERSION,
            state: "ready",
            origin: PUBLIC_ORIGIN,
            model: MODEL,
            operations: ["generation", "edit"],
            controls: controls(),
            implementation_sha: implementation_sha(),
        };
        println!("{}", serde_json::to_string_pretty(&plan)?);
        return Ok(());
    }

    #[cfg(not(unix))]
    bail!("OpenAI image public smoke requires Unix file permission semantics");

    let implementation_sha = implementation_sha()
        .filter(|value| valid_sha(value))
        .context("public image smoke requires exact compile-time CLAUDE_API_IMPLEMENTATION_SHA")?;
    create_private_directory(&args.output)?;
    persist_pre_dispatch_journal(&args.output, implementation_sha, "database_url_loading")?;

    let database_url = openai_image_public_smoke_database_url()
        .context("public image smoke requires CLAUDE_API_DATABASE_URL")?;
    persist_pre_dispatch_journal(&args.output, implementation_sha, "database_connecting")?;
    let mut registry = registry::pg::PgStore::connect_with_application_name(
        &database_url,
        "gpt-image-2-public-smoke",
    )?;
    persist_pre_dispatch_journal(&args.output, implementation_sha, "schema_verifying")?;
    registry.verify_schema()?;
    persist_pre_dispatch_journal(&args.output, implementation_sha, "credential_selecting")?;
    let credential = registry.openai_image_smoke_credential()?;

    persist_pre_dispatch_journal(&args.output, implementation_sha, "runtime_building")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("create public image smoke runtime")?;
    runtime.block_on(preflight_and_execute(
        args.output,
        implementation_sha,
        credential,
        registry,
        args.execute,
    ))
}

async fn preflight_and_execute(
    output: PathBuf,
    implementation_sha: &str,
    credential: registry::pg::OpenAiImageSmokeCredential,
    mut registry: registry::pg::PgStore,
    execute: bool,
) -> Result<()> {
    persist_pre_dispatch_journal(&output, implementation_sha, "client_building")?;
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(600))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("apitoken-gpt-image-2-public-smoke/1")
        .build()?;
    persist_pre_dispatch_journal(&output, implementation_sha, "discovery_checking")?;
    verify_discovery_hidden(&client, credential.authorization_key()).await?;
    persist_pre_dispatch_journal(&output, implementation_sha, "preflight_success")?;
    if !execute {
        println!("GPT Image 2 public smoke preflight GREEN for {implementation_sha}");
        return Ok(());
    }
    persist_journal(
        &output,
        &Journal {
            schema_version: SCHEMA_VERSION,
            state: "generation_dispatching",
            implementation_sha,
            generation_dispatched: true,
            edit_dispatched: false,
            generation_request_id: None,
            edit_request_id: None,
        },
    )?;

    let generation_result = generate(&client, credential.authorization_key()).await;
    let generation = match generation_result {
        Ok(value) => value,
        Err(error) => {
            persist_journal(
                &output,
                &Journal {
                    schema_version: SCHEMA_VERSION,
                    state: "generation_outcome_unknown",
                    implementation_sha,
                    generation_dispatched: true,
                    edit_dispatched: false,
                    generation_request_id: None,
                    edit_request_id: None,
                },
            )?;
            return Err(
                error.context("public image generation failed after dispatch; replay forbidden")
            );
        }
    };
    persist_journal(
        &output,
        &Journal {
            schema_version: SCHEMA_VERSION,
            state: "generation_received",
            implementation_sha,
            generation_dispatched: true,
            edit_dispatched: false,
            generation_request_id: Some(&generation.request_id),
            edit_request_id: None,
        },
    )?;
    write_private_new(&output.join("generation.png"), &generation.png)?;
    let generation_settlement = match wait_for_settlement(
        &mut registry,
        &generation.request_id,
        &credential,
        "generation",
        &generation.usage,
    ) {
        Ok(value) => value,
        Err(error) => {
            persist_journal(
                &output,
                &Journal {
                    schema_version: SCHEMA_VERSION,
                    state: "generation_settlement_failed",
                    implementation_sha,
                    generation_dispatched: true,
                    edit_dispatched: false,
                    generation_request_id: Some(&generation.request_id),
                    edit_request_id: None,
                },
            )?;
            return Err(error.context(
                "public image generation settlement failed after dispatch; replay forbidden",
            ));
        }
    };

    persist_journal(
        &output,
        &Journal {
            schema_version: SCHEMA_VERSION,
            state: "edit_dispatching",
            implementation_sha,
            generation_dispatched: true,
            edit_dispatched: true,
            generation_request_id: Some(&generation.request_id),
            edit_request_id: None,
        },
    )?;
    let edit_result = edit(&client, credential.authorization_key(), &generation.png).await;
    let edit = match edit_result {
        Ok(value) => value,
        Err(error) => {
            persist_journal(
                &output,
                &Journal {
                    schema_version: SCHEMA_VERSION,
                    state: "edit_outcome_unknown",
                    implementation_sha,
                    generation_dispatched: true,
                    edit_dispatched: true,
                    generation_request_id: Some(&generation.request_id),
                    edit_request_id: None,
                },
            )?;
            return Err(error.context("public image edit failed after dispatch; replay forbidden"));
        }
    };
    persist_journal(
        &output,
        &Journal {
            schema_version: SCHEMA_VERSION,
            state: "edit_received",
            implementation_sha,
            generation_dispatched: true,
            edit_dispatched: true,
            generation_request_id: Some(&generation.request_id),
            edit_request_id: Some(&edit.request_id),
        },
    )?;
    write_private_new(&output.join("edit.png"), &edit.png)?;
    if edit.png == generation.png {
        persist_journal(
            &output,
            &Journal {
                schema_version: SCHEMA_VERSION,
                state: "edit_validation_failed",
                implementation_sha,
                generation_dispatched: true,
                edit_dispatched: true,
                generation_request_id: Some(&generation.request_id),
                edit_request_id: Some(&edit.request_id),
            },
        )?;
        bail!("public image edit returned byte-identical PNG; replay forbidden");
    }
    let edit_settlement = match wait_for_settlement(
        &mut registry,
        &edit.request_id,
        &credential,
        "edit",
        &edit.usage,
    ) {
        Ok(value) => value,
        Err(error) => {
            persist_journal(
                &output,
                &Journal {
                    schema_version: SCHEMA_VERSION,
                    state: "edit_settlement_failed",
                    implementation_sha,
                    generation_dispatched: true,
                    edit_dispatched: true,
                    generation_request_id: Some(&generation.request_id),
                    edit_request_id: Some(&edit.request_id),
                },
            )?;
            return Err(error
                .context("public image edit settlement failed after dispatch; replay forbidden"));
        }
    };

    let unchanged_at_both_settlements = |generation_value: i64, edit_value: i64, baseline: i64| {
        generation_value == baseline && edit_value == baseline
    };
    let evidence = SmokeEvidence {
        schema_version: SCHEMA_VERSION,
        state: "success",
        implementation_sha,
        origin: PUBLIC_ORIGIN,
        model: MODEL,
        discovery_hidden_before_publication: true,
        account_balance_unchanged: unchanged_at_both_settlements(
            generation_settlement.balance_nano,
            edit_settlement.balance_nano,
            credential.balance_nano,
        ),
        account_spent_unchanged: unchanged_at_both_settlements(
            generation_settlement.spent_nano,
            edit_settlement.spent_nano,
            credential.spent_nano,
        ),
        account_reserved_unchanged: unchanged_at_both_settlements(
            generation_settlement.reserved_nano,
            edit_settlement.reserved_nano,
            credential.reserved_nano,
        ),
        key_spent_unchanged: unchanged_at_both_settlements(
            generation_settlement.key_spent_nano,
            edit_settlement.key_spent_nano,
            credential.key_spent_nano,
        ),
        key_reserved_unchanged: unchanged_at_both_settlements(
            generation_settlement.key_reserved_nano,
            edit_settlement.key_reserved_nano,
            credential.key_reserved_nano,
        ),
        generation: operation_evidence(generation, generation_settlement),
        edit: operation_evidence(edit, edit_settlement),
    };
    if !evidence.account_balance_unchanged
        || !evidence.account_spent_unchanged
        || !evidence.account_reserved_unchanged
        || !evidence.key_spent_unchanged
        || !evidence.key_reserved_unchanged
    {
        persist_journal(
            &output,
            &Journal {
                schema_version: SCHEMA_VERSION,
                state: "money_invariant_failed",
                implementation_sha,
                generation_dispatched: true,
                edit_dispatched: true,
                generation_request_id: Some(&evidence.generation.request_id),
                edit_request_id: Some(&evidence.edit.request_id),
            },
        )?;
        bail!("meter-only public image smoke changed account or key money aggregates");
    }
    persist_json(&output.join("evidence.json"), &evidence)?;
    persist_journal(
        &output,
        &Journal {
            schema_version: SCHEMA_VERSION,
            state: "success",
            implementation_sha,
            generation_dispatched: true,
            edit_dispatched: true,
            generation_request_id: Some(&evidence.generation.request_id),
            edit_request_id: Some(&evidence.edit.request_id),
        },
    )?;
    println!(
        "GPT Image 2 public smoke GREEN: generation={}x{} input={} output={}; edit={}x{} image_input={} output={}; meter_only real_nano={}/{} charge_nano=0/0",
        evidence.generation.width,
        evidence.generation.height,
        evidence.generation.usage.input_tokens,
        evidence.generation.usage.output_tokens,
        evidence.edit.width,
        evidence.edit.height,
        evidence.edit.usage.input_tokens_details.image_tokens,
        evidence.edit.usage.output_tokens,
        evidence.generation.settlement.real_nano,
        evidence.edit.settlement.real_nano,
    );
    Ok(())
}

async fn verify_discovery_hidden(client: &Client, key: &str) -> Result<()> {
    let response = client
        .get(format!("{PUBLIC_ORIGIN}/v1/models"))
        .bearer_auth(key)
        .send()
        .await
        .context("authenticated public model discovery")?;
    if !response.status().is_success() {
        bail!(
            "authenticated public model discovery returned {}",
            response.status()
        );
    }
    let body = bounded_body(response).await?;
    let value: Value = serde_json::from_slice(&body).context("decode public model discovery")?;
    let ids = value
        .get("data")
        .and_then(Value::as_array)
        .context("public model discovery lacks data array")?;
    if ids.iter().any(|entry| {
        entry.get("id").and_then(Value::as_str).is_some_and(|id| {
            matches!(
                id.strip_prefix("openai/").unwrap_or(id),
                MODEL | DATED_MODEL
            )
        })
    }) {
        bail!("GPT Image 2 is already published before its public smoke");
    }
    Ok(())
}

async fn generate(client: &Client, key: &str) -> Result<CompletedImage> {
    let response = client
        .post(format!("{PUBLIC_ORIGIN}/v1/images/generations"))
        .bearer_auth(key)
        .json(&json!({
            "model": MODEL,
            "prompt": "A simple flat illustration of a bright blue ceramic mug on a plain beige background. No text, logos, people, brands, or copyrighted characters.",
            "n": 1,
            "background": "opaque",
            "quality": "low",
            "size": "auto",
            "output_format": "png",
            "response_format": "b64_json"
        }))
        .send()
        .await
        .context("dispatch public image generation")?;
    decode_image_response(response, "generation").await
}

async fn edit(client: &Client, key: &str, reference: &[u8]) -> Result<CompletedImage> {
    let form = reqwest::multipart::Form::new()
        .text("model", MODEL.to_owned())
        .text("prompt", "Edit the supplied image so the ceramic mug is bright red instead of blue. Keep the plain beige background and simple flat illustration style. Add no text, logos, people, brands, or copyrighted characters.")
        .text("n", "1")
        .text("background", "opaque")
        .text("quality", "low")
        .text("size", "auto")
        .text("output_format", "png")
        .text("response_format", "b64_json")
        .part(
            "image",
            reqwest::multipart::Part::bytes(reference.to_vec())
                .file_name("generation.png")
                .mime_str("image/png")?,
        );
    let response = client
        .post(format!("{PUBLIC_ORIGIN}/v1/images/edits"))
        .bearer_auth(key)
        .multipart(form)
        .send()
        .await
        .context("dispatch public image edit")?;
    decode_image_response(response, "edit").await
}

async fn decode_image_response(response: Response, operation: &str) -> Result<CompletedImage> {
    let status = response.status();
    let request_id = request_id(response.headers())?;
    let body = bounded_body(response).await?;
    if !status.is_success() {
        bail!("public image {operation} returned HTTP {status}");
    }
    let decoded: ImageResponse =
        serde_json::from_slice(&body).context("decode public image response")?;
    if decoded.data.len() != 1 {
        bail!("public image {operation} returned other than one image");
    }
    validate_public_usage(&decoded.usage, operation)?;
    let png = base64::engine::general_purpose::STANDARD
        .decode(&decoded.data[0].b64_json)
        .context("decode public image base64")?;
    let image = ImageReference::new(png.clone()).context("validate returned public PNG")?;
    Ok(CompletedImage {
        request_id,
        width: image.width(),
        height: image.height(),
        png,
        usage: decoded.usage,
    })
}

fn validate_public_usage(usage: &PublicUsage, operation: &str) -> Result<()> {
    if usage.input_tokens
        != usage
            .input_tokens_details
            .text_tokens
            .checked_add(usage.input_tokens_details.image_tokens)
            .context("public image input token sum overflow")?
        || usage.output_tokens != usage.output_tokens_details.image_tokens
        || usage.total_tokens
            != usage
                .input_tokens
                .checked_add(usage.output_tokens)
                .context("public image total token sum overflow")?
        || usage.output_tokens == 0
        || (operation == "generation" && usage.input_tokens_details.image_tokens != 0)
        || (operation == "edit" && usage.input_tokens_details.image_tokens == 0)
    {
        bail!("public image {operation} usage is incomplete or inconsistent");
    }
    Ok(())
}

fn wait_for_settlement(
    registry: &mut registry::pg::PgStore,
    request_id: &str,
    credential: &registry::pg::OpenAiImageSmokeCredential,
    operation: &str,
    usage: &PublicUsage,
) -> Result<registry::pg::OpenAiImageSettlementEvidence> {
    let deadline = Instant::now() + SETTLEMENT_WAIT;
    loop {
        if let Some(evidence) = registry.openai_image_settlement_evidence(request_id)? {
            validate_settlement(&evidence, credential, operation, usage)?;
            return Ok(evidence);
        }
        let now = Instant::now();
        let Some(delay) = settlement_poll_delay(now, deadline) else {
            bail!("authoritative public image {operation} settlement did not become visible");
        };
        thread::sleep(delay);
    }
}

fn settlement_poll_delay(now: Instant, deadline: Instant) -> Option<Duration> {
    (now < deadline).then(|| SETTLEMENT_POLL.min(deadline - now))
}

fn validate_settlement(
    evidence: &registry::pg::OpenAiImageSettlementEvidence,
    credential: &registry::pg::OpenAiImageSmokeCredential,
    operation: &str,
    usage: &PublicUsage,
) -> Result<()> {
    let modifiers = evidence
        .official_cost
        .get("premium_modifiers")
        .context("image settlement lacks premium modifiers")?;
    let tariff = metering::openai_image_tariff(MODEL)
        .map_err(|_| anyhow::anyhow!("resolve exact GPT Image 2 settlement tariff"))?;
    let expected_input_nano = u128::from(usage.input_tokens_details.text_tokens)
        .checked_mul(tariff.prices.fresh_text_input as u128)
        .and_then(|value| {
            u128::from(usage.input_tokens_details.image_tokens)
                .checked_mul(tariff.prices.fresh_image_input as u128)
                .and_then(|image| value.checked_add(image))
        })
        .context("public image input cost overflow")?;
    let expected_output_nano = u128::from(usage.output_tokens)
        .checked_mul(tariff.prices.image_output as u128)
        .context("public image output cost overflow")?;
    let expected_real_nano = expected_input_nano
        .checked_add(expected_output_nano)
        .context("public image total cost overflow")?;
    let expected_input_nano = i64::try_from(expected_input_nano)
        .context("public image input cost is outside signed nanoUSD")?;
    let expected_output_nano = i64::try_from(expected_output_nano)
        .context("public image output cost is outside signed nanoUSD")?;
    let expected_real_nano = i64::try_from(expected_real_nano)
        .context("public image total cost is outside signed nanoUSD")?;
    let input_tokens = i64::try_from(usage.input_tokens)
        .context("public image input usage is outside signed tokens")?;
    let output_tokens = i64::try_from(usage.output_tokens)
        .context("public image output usage is outside signed tokens")?;
    if !valid_request_id(&evidence.request_id)
        || evidence.account_id != credential.account_id
        || evidence.key_id != credential.key_id
        || evidence.reservation_state != "settled"
        || evidence.reservation_hold_nano != 0
        || evidence.reservation_actual_nano != Some(0)
        || evidence.outbox_state != "done"
        || evidence.outbox_disposition != "settle"
        || evidence.release_generation <= 0
        || evidence.release_billing_mode != "meter_only"
        || evidence.provider_id != registry::PROVIDER_OPENAI
        || evidence.provider != registry::PROVIDER_OPENAI
        || evidence.canonical_model_id != tariff.canonical_model_id
        || evidence.model != MODEL
        || evidence.tariff_schedule_id != tariff.tariff_schedule_id.as_str()
        || evidence.official_hold_nano <= 0
        || evidence.charged_hold_nano != 0
        || evidence.real_nano != expected_real_nano
        || evidence.charge_nano != 0
        || evidence.input_tokens != input_tokens
        || evidence.output_tokens != output_tokens
        || evidence.cache_read_tokens != 0
        || evidence.input_nano != expected_input_nano
        || evidence.output_nano != expected_output_nano
        || evidence.cache_read_nano != 0
        || evidence.priced_ts <= 0
        || evidence
            .official_cost
            .get("alias_generation")
            .and_then(Value::as_i64)
            != Some(metering::OPENAI_IMAGE_ALIAS_GENERATION)
        || evidence
            .official_cost
            .get("requested_model_id")
            .and_then(Value::as_str)
            != Some(MODEL)
        || modifiers.get("kind").and_then(Value::as_str) != Some("openai_image_v1")
        || modifiers.get("operation").and_then(Value::as_str) != Some(operation)
        || modifiers.get("background").and_then(Value::as_str) != Some("opaque")
        || modifiers.get("quality").and_then(Value::as_str) != Some("low")
        || modifiers.get("size").and_then(Value::as_str) != Some("auto")
        || modifiers.get("reference_count").and_then(Value::as_i64)
            != Some(if operation == "edit" { 1 } else { 0 })
    {
        bail!("authoritative public image {operation} settlement failed exact validation");
    }
    Ok(())
}

async fn bounded_body(mut response: Response) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        bail!("public image response exceeds the 24 MiB bound");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            bail!("public image response exceeds the 24 MiB bound");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn request_id(headers: &HeaderMap) -> Result<String> {
    let value = headers
        .get("x-request-id")
        .context("public image response lacks x-request-id")?
        .to_str()
        .context("public image x-request-id is not ASCII")?;
    if !valid_request_id(value) {
        bail!("public image x-request-id is not the engine reservation identity");
    }
    Ok(value.to_owned())
}

fn valid_request_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes[8] == b'-'
        && bytes[13] == b'-'
        && bytes[18] == b'-'
        && bytes[23] == b'-'
        && bytes[14] == b'4'
        && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 8 | 13 | 18 | 23)
                || byte.is_ascii_digit()
                || matches!(*byte, b'a'..=b'f')
        })
}

fn operation_evidence(
    image: CompletedImage,
    settlement: registry::pg::OpenAiImageSettlementEvidence,
) -> OperationEvidence {
    OperationEvidence {
        request_id: image.request_id,
        width: image.width,
        height: image.height,
        png_bytes: image.png.len(),
        png_sha256: format!(
            "sha256:{}",
            crate::openai_image_canary::sha256_hex(&image.png)
        ),
        usage: image.usage,
        settlement,
    }
}

fn controls() -> Value {
    json!({
        "n": 1,
        "background": "opaque",
        "quality": "low",
        "size": "auto",
        "output_format": "png",
        "response_format": "b64_json",
        "edit_references": 1
    })
}

fn implementation_sha() -> Option<&'static str> {
    option_env!("CLAUDE_API_IMPLEMENTATION_SHA")
}

fn valid_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

#[cfg(unix)]
fn create_private_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    match std::fs::symlink_metadata(path) {
        Ok(_) => bail!("public image smoke output already exists; paid replay forbidden"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if parent.as_os_str().is_empty() || !parent.is_absolute() {
        bail!("public image smoke output must have an absolute private parent");
    }
    let metadata = std::fs::symlink_metadata(parent)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.permissions().mode() & 0o077 != 0
    {
        bail!("public image smoke parent must be a mode-private actual directory");
    }
    let mut builder = std::fs::DirBuilder::new();
    builder.mode(0o700).create(path)?;
    Ok(())
}

#[cfg(unix)]
fn write_private_new(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn persist_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    write_private_new(path, &bytes)
}

#[cfg(unix)]
fn persist_pre_dispatch_journal(path: &Path, implementation_sha: &str, state: &str) -> Result<()> {
    persist_journal(
        path,
        &Journal {
            schema_version: SCHEMA_VERSION,
            state,
            implementation_sha,
            generation_dispatched: false,
            edit_dispatched: false,
            generation_request_id: None,
            edit_request_id: None,
        },
    )
}

#[cfg(unix)]
fn persist_journal(path: &Path, value: &Journal<'_>) -> Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt;
    let target = path.join("journal.json");
    let temporary = path.join(format!(
        ".journal-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    std::fs::rename(&temporary, &target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settlement_poll_obeys_wall_clock_deadline() {
        let now = Instant::now();
        assert_eq!(
            settlement_poll_delay(now, now + Duration::from_secs(1)),
            Some(SETTLEMENT_POLL)
        );
        assert_eq!(
            settlement_poll_delay(now, now + Duration::from_millis(100)),
            Some(Duration::from_millis(100))
        );
        assert_eq!(settlement_poll_delay(now, now), None);
    }

    #[test]
    fn request_id_requires_engine_money_identity() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-request-id",
            "123e4567-e89b-42d3-a456-426614174000".parse().unwrap(),
        );
        assert_eq!(
            request_id(&headers).unwrap(),
            "123e4567-e89b-42d3-a456-426614174000"
        );
        for invalid in [
            "123e4567-e89b-12d3-a456-426614174000",
            "123e4567-e89b-42d3-7456-426614174000",
            "123E4567-E89B-42D3-A456-426614174000",
            "req_0123456789abcdef0123456789abcdef",
        ] {
            headers.insert("x-request-id", invalid.parse().unwrap());
            assert!(request_id(&headers).is_err());
        }
    }

    #[test]
    fn usage_validation_requires_exact_modalities() {
        let generation = PublicUsage {
            input_tokens: 5,
            input_tokens_details: InputTokenDetails {
                text_tokens: 5,
                image_tokens: 0,
            },
            output_tokens: 7,
            output_tokens_details: OutputTokenDetails { image_tokens: 7 },
            total_tokens: 12,
        };
        assert!(validate_public_usage(&generation, "generation").is_ok());
        assert!(validate_public_usage(&generation, "edit").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn output_directory_is_absolute_private_and_replay_fenced() {
        use std::os::unix::fs::PermissionsExt;

        let unique = format!(
            "claude-api-image-public-smoke-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let parent = std::env::temp_dir().join(unique);
        std::fs::create_dir(&parent).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        let output = parent.join("evidence");

        create_private_directory(&output).unwrap();
        assert_eq!(
            std::fs::metadata(&output).unwrap().permissions().mode() & 0o777,
            0o700
        );
        persist_pre_dispatch_journal(
            &output,
            "0123456789abcdef0123456789abcdef01234567",
            "preflight_success",
        )
        .unwrap();
        let journal: Value =
            serde_json::from_slice(&std::fs::read(output.join("journal.json")).unwrap()).unwrap();
        assert_eq!(journal["state"], "preflight_success");
        assert_eq!(journal["generation_dispatched"], false);
        assert_eq!(journal["edit_dispatched"], false);
        assert!(journal["generation_request_id"].is_null());
        assert!(journal["edit_request_id"].is_null());
        assert_eq!(
            std::fs::metadata(output.join("journal.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(create_private_directory(&output).is_err());

        std::fs::remove_dir_all(&output).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o750)).unwrap();
        assert!(create_private_directory(&output).is_err());
        std::fs::remove_dir(&parent).unwrap();
    }
}
