//! Strict GPT Image 2 operations backed by one selected ChatGPT subscription home.

use super::{
    new_id, AuthContext, CodexGateway, CodexHome, HomeSelection, ImageDispatchError, ProcessError,
    TurnSlot,
};
use base64::engine::general_purpose::STANDARD as STANDARD_BASE64;
use base64::Engine as _;
use futures_util::StreamExt as _;
use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};
use std::fmt;
use std::io::Cursor;
use std::sync::Arc;

pub const GPT_IMAGE_2: &str = "gpt-image-2";
/// Public discovery identities of the Images API, in the order `/v1/models` lists them.
///
/// These are exactly the ids `image_api::validate_model` admits (the reviewed alias and its
/// immutable snapshot), so discovery can never advertise an id the paid routes would refuse.
/// They are served by `POST /v1/images/{generations,edits}` only; the text lanes reject them.
pub const PUBLIC_IMAGE_MODEL_IDS: &[&str] = &[GPT_IMAGE_2, metering::GPT_IMAGE_2_SNAPSHOT];
const MAX_PROMPT_CHARS: usize = 32_000;
const MAX_PROMPT_BYTES: usize = 128 * 1024;
const MAX_IMAGE_STORAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_IMAGE_DECODED_BYTES: usize = 16 * 1024 * 1024;
const MAX_EDIT_STORAGE_BYTES: usize = 32 * 1024 * 1024;
const MAX_EDIT_DECODED_BYTES: usize = 64 * 1024 * 1024;
const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;
const MAX_IMAGE_BASE64_BYTES: usize = MAX_IMAGE_STORAGE_BYTES
    .checked_add(2)
    .expect("image size constant")
    .checked_div(3)
    .expect("nonzero base64 divisor")
    .checked_mul(4)
    .expect("base64 expansion constant");
const MAX_RESPONSE_ENVELOPE_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: usize = MAX_IMAGE_BASE64_BYTES
    .checked_add(MAX_RESPONSE_ENVELOPE_BYTES)
    .expect("image response limit constant");
const MAX_DIMENSION: u32 = 4096;
const MAX_REQUEST_ID_CHARS: usize = 128;
const MIN_CREATED_UNIX_SECONDS: u64 = 1_577_836_800;
const MAX_CREATED_UNIX_SECONDS: u64 = 4_102_444_800;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GptImage2;

impl GptImage2 {
    pub const ID: &'static str = GPT_IMAGE_2;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageTurnId(String);

impl ImageTurnId {
    pub fn new(value: impl Into<String>) -> Result<Self, CodexImageError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(validation(
                "image turn id must be 1..=128 ASCII alphanumeric, '-', '_' or '.' bytes",
            ));
        }
        Ok(Self(value))
    }

    fn automatic() -> Result<Self, CodexImageError> {
        Self::new(new_id("image_turn"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageBackground {
    #[default]
    Auto,
    Opaque,
}

impl ImageBackground {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Opaque => "opaque",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageQuality {
    Low,
    Medium,
    High,
    #[default]
    Auto,
}

impl ImageQuality {
    fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Auto => "auto",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageSize {
    #[default]
    Auto,
    Exact {
        width: u16,
        height: u16,
    },
}

impl ImageSize {
    pub fn exact(width: u16, height: u16) -> Result<Self, CodexImageError> {
        let long = u32::from(width.max(height));
        let short = u32::from(width.min(height));
        let pixels = u64::from(width) * u64::from(height);
        if width == 0
            || height == 0
            || long > 3_840
            || width % 16 != 0
            || height % 16 != 0
            || long > short.saturating_mul(3)
            || !(655_360..=8_294_400).contains(&pixels)
        {
            return Err(validation("image size is outside GPT Image 2 constraints"));
        }
        Ok(Self::Exact { width, height })
    }

    fn wire_value(self) -> String {
        match self {
            Self::Auto => "auto".to_owned(),
            Self::Exact { width, height } => format!("{width}x{height}"),
        }
    }
}

#[derive(Clone)]
pub struct ImageGenerationRequest {
    prompt: String,
    background: ImageBackground,
    quality: ImageQuality,
    size: ImageSize,
}

impl ImageGenerationRequest {
    pub fn new(prompt: impl Into<String>) -> Result<Self, CodexImageError> {
        let prompt = prompt.into();
        validate_prompt(&prompt)?;
        Ok(Self {
            prompt,
            background: ImageBackground::Auto,
            quality: ImageQuality::Auto,
            size: ImageSize::Auto,
        })
    }

    pub fn with_controls(
        mut self,
        background: ImageBackground,
        quality: ImageQuality,
        size: ImageSize,
    ) -> Self {
        self.background = background;
        self.quality = quality;
        self.size = size;
        self
    }

    pub fn model(&self) -> GptImage2 {
        GptImage2
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    pub fn background(&self) -> ImageBackground {
        self.background
    }

    pub fn quality(&self) -> ImageQuality {
        self.quality
    }

    pub fn size(&self) -> ImageSize {
        self.size
    }
}

impl fmt::Debug for ImageGenerationRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImageGenerationRequest")
            .field("model", &GPT_IMAGE_2)
            .field("prompt", &"REDACTED")
            .field("prompt_chars", &self.prompt.chars().count())
            .finish()
    }
}

#[derive(Clone)]
pub struct ImageReference {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    decoded_bytes: usize,
}

impl ImageReference {
    pub fn new(bytes: Vec<u8>) -> Result<Self, CodexImageError> {
        let metadata = decode_png_metadata(&bytes).map_err(validation)?;
        Ok(Self {
            bytes,
            width: metadata.width,
            height: metadata.height,
            decoded_bytes: metadata.decoded_bytes,
        })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

impl fmt::Debug for ImageReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImageReference")
            .field("bytes", &"REDACTED")
            .field("len", &self.bytes.len())
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

#[derive(Clone)]
pub struct ImageEditRequest {
    prompt: String,
    images: Vec<ImageReference>,
    background: ImageBackground,
    quality: ImageQuality,
    size: ImageSize,
}

impl ImageEditRequest {
    pub fn new(
        prompt: impl Into<String>,
        images: Vec<ImageReference>,
    ) -> Result<Self, CodexImageError> {
        let prompt = prompt.into();
        validate_prompt(&prompt)?;
        validate_references(&images)?;
        Ok(Self {
            prompt,
            images,
            background: ImageBackground::Auto,
            quality: ImageQuality::Auto,
            size: ImageSize::Auto,
        })
    }

    pub fn with_controls(
        mut self,
        background: ImageBackground,
        quality: ImageQuality,
        size: ImageSize,
    ) -> Self {
        self.background = background;
        self.quality = quality;
        self.size = size;
        self
    }

    pub fn model(&self) -> GptImage2 {
        GptImage2
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    pub fn images(&self) -> &[ImageReference] {
        &self.images
    }

    pub fn background(&self) -> ImageBackground {
        self.background
    }

    pub fn quality(&self) -> ImageQuality {
        self.quality
    }

    pub fn size(&self) -> ImageSize {
        self.size
    }
}

impl fmt::Debug for ImageEditRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImageEditRequest")
            .field("model", &GPT_IMAGE_2)
            .field("prompt", &"REDACTED")
            .field("prompt_chars", &self.prompt.chars().count())
            .field("image_count", &self.images.len())
            .field(
                "image_storage_bytes",
                &self
                    .images
                    .iter()
                    .map(|image| image.bytes.len())
                    .sum::<usize>(),
            )
            .finish()
    }
}

#[derive(Clone)]
pub struct CodexImageResult {
    png: Vec<u8>,
    width: u32,
    height: u32,
    created: u64,
    background: String,
    quality: String,
    size: String,
    output_format: Option<String>,
    usage: Option<Value>,
    request_id: Option<String>,
    home_id: String,
    image_turn_id: ImageTurnId,
}

impl CodexImageResult {
    pub fn png(&self) -> &[u8] {
        &self.png
    }

    pub fn into_png(self) -> Vec<u8> {
        self.png
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn created(&self) -> u64 {
        self.created
    }

    pub fn background(&self) -> &str {
        &self.background
    }

    pub fn quality(&self) -> &str {
        &self.quality
    }

    pub fn size(&self) -> &str {
        &self.size
    }

    pub fn output_format(&self) -> Option<&str> {
        self.output_format.as_deref()
    }

    pub fn usage(&self) -> Option<&Value> {
        self.usage.as_ref()
    }

    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    pub fn home_id(&self) -> &str {
        &self.home_id
    }

    pub fn image_turn_id(&self) -> &ImageTurnId {
        &self.image_turn_id
    }
}

impl fmt::Debug for CodexImageResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodexImageResult")
            .field("png", &"REDACTED")
            .field("png_len", &self.png.len())
            .field("width", &self.width)
            .field("height", &self.height)
            .field("created", &self.created)
            .field("background", &self.background)
            .field("quality", &self.quality)
            .field("size", &self.size)
            .field("output_format", &self.output_format)
            .field("usage", &self.usage.as_ref().map(|_| "REDACTED"))
            .field("request_id", &self.request_id)
            .field("home_id", &self.home_id)
            .field("image_turn_id", &self.image_turn_id)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum CodexImageError {
    Validation(&'static str),
    Unavailable,
    AuthenticationRequired(ImageErrorContext),
    UsageLimit(ImageErrorContext),
    BadRequest(ImageErrorContext),
    Status(ImageErrorContext),
    ResponseTimeout(Option<ImageErrorContext>),
    ResponseBodyClosed(ImageErrorContext),
    OutcomeUnknown(Option<ImageErrorContext>),
    InvalidResponse(ImageErrorContext),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageErrorContext {
    status: u16,
    request_id: Option<String>,
}

impl ImageErrorContext {
    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }
}

impl CodexImageError {
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::AuthenticationRequired(context)
            | Self::UsageLimit(context)
            | Self::BadRequest(context)
            | Self::Status(context)
            | Self::ResponseBodyClosed(context)
            | Self::InvalidResponse(context)
            | Self::ResponseTimeout(Some(context))
            | Self::OutcomeUnknown(Some(context)) => Some(context.status),
            Self::Validation(_)
            | Self::Unavailable
            | Self::ResponseTimeout(None)
            | Self::OutcomeUnknown(None) => None,
        }
    }

    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::AuthenticationRequired(context)
            | Self::UsageLimit(context)
            | Self::BadRequest(context)
            | Self::Status(context)
            | Self::ResponseBodyClosed(context)
            | Self::InvalidResponse(context)
            | Self::ResponseTimeout(Some(context))
            | Self::OutcomeUnknown(Some(context)) => context.request_id(),
            Self::Validation(_)
            | Self::Unavailable
            | Self::ResponseTimeout(None)
            | Self::OutcomeUnknown(None) => None,
        }
    }
}

impl fmt::Debug for CodexImageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(message) => {
                formatter.debug_tuple("Validation").field(message).finish()
            }
            Self::Unavailable => formatter.write_str("Unavailable"),
            Self::AuthenticationRequired(context) => formatter
                .debug_tuple("AuthenticationRequired")
                .field(context)
                .finish(),
            Self::UsageLimit(context) => {
                formatter.debug_tuple("UsageLimit").field(context).finish()
            }
            Self::BadRequest(context) => {
                formatter.debug_tuple("BadRequest").field(context).finish()
            }
            Self::Status(context) => formatter.debug_tuple("Status").field(context).finish(),
            Self::ResponseTimeout(context) => formatter
                .debug_tuple("ResponseTimeout")
                .field(context)
                .finish(),
            Self::ResponseBodyClosed(context) => formatter
                .debug_tuple("ResponseBodyClosed")
                .field(context)
                .finish(),
            Self::OutcomeUnknown(context) => formatter
                .debug_tuple("OutcomeUnknown")
                .field(context)
                .finish(),
            Self::InvalidResponse(context) => formatter
                .debug_tuple("InvalidResponse")
                .field(context)
                .finish(),
        }
    }
}

impl fmt::Display for CodexImageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(message) => {
                write!(formatter, "image request validation failed: {message}")
            }
            Self::Unavailable => formatter.write_str("no Codex image home is available"),
            Self::AuthenticationRequired(_) => {
                formatter.write_str("Codex image authentication is required")
            }
            Self::UsageLimit(_) => formatter.write_str("Codex image usage limit exceeded"),
            Self::BadRequest(context) => write!(
                formatter,
                "Codex image request was rejected with HTTP {}",
                context.status
            ),
            Self::Status(context) => write!(
                formatter,
                "Codex image upstream returned HTTP {}",
                context.status
            ),
            Self::ResponseTimeout(_) => formatter.write_str("Codex image response timed out"),
            Self::ResponseBodyClosed(_) => {
                formatter.write_str("Codex image response body closed unexpectedly")
            }
            Self::OutcomeUnknown(_) => formatter.write_str("Codex image outcome is unknown"),
            Self::InvalidResponse(_) => {
                formatter.write_str("Codex image upstream returned an invalid response")
            }
        }
    }
}

impl std::error::Error for CodexImageError {}

#[derive(Clone, Copy)]
enum ImageOperation {
    Generation,
    Edit,
}

enum ImageAttemptError {
    Terminal(CodexImageError),
    Rotate(CodexImageError),
}

impl ImageAttemptError {
    fn into_error(self) -> CodexImageError {
        match self {
            Self::Terminal(error) | Self::Rotate(error) => error,
        }
    }
}

impl From<CodexImageError> for ImageAttemptError {
    fn from(error: CodexImageError) -> Self {
        Self::Terminal(error)
    }
}

impl CodexGateway {
    /// Choose one currently admitted opaque profile without dispatching an image request.
    /// The caller freezes this id and uses only exact-home methods for the paid attempt.
    pub async fn select_image_canary_home(&self) -> Result<String, CodexImageError> {
        if self
            .shutting_down
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(CodexImageError::Unavailable);
        }
        let now = pool::now();
        let mut homes = self.homes().await;
        homes.sort_by_key(|home| home.order());
        for home in homes {
            home.hydrate_health().await;
            let limits = home.rate_limits().await;
            if home.admission(limits.as_ref(), now).is_admitted() {
                return Ok(home.id().to_owned());
            }
        }
        Err(CodexImageError::Unavailable)
    }

    /// Prove the exact opaque profile's OAuth and quota state without running an image turn.
    pub async fn preflight_image_home(&self, home_id: &str) -> Result<(), CodexImageError> {
        if self
            .shutting_down
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(CodexImageError::Unavailable);
        }
        let home = self
            .homes()
            .await
            .into_iter()
            .find(|home| home.id() == home_id)
            .ok_or(CodexImageError::Unavailable)?;
        home.hydrate_health().await;
        match home.probe().await {
            Ok(()) => {
                home.mark_healthy();
                Ok(())
            }
            Err(error) => {
                note_pre_dispatch_error(&home, &error);
                Err(map_pre_dispatch_error(error))
            }
        }
    }

    pub async fn generate_image(
        &self,
        request: &ImageGenerationRequest,
    ) -> Result<CodexImageResult, CodexImageError> {
        validate_prompt(&request.prompt)?;
        self.execute_image_automatically(
            ImageOperation::Generation,
            generation_body(request),
            ImageTurnId::automatic()?,
        )
        .await
    }

    pub async fn generate_image_on_home(
        &self,
        home_id: &str,
        image_turn_id: &ImageTurnId,
        request: &ImageGenerationRequest,
    ) -> Result<CodexImageResult, CodexImageError> {
        validate_prompt(&request.prompt)?;
        let (home, slot) = self.select_exact_image_home(home_id).await?;
        self.execute_image(
            home,
            slot,
            ImageOperation::Generation,
            &generation_body(request),
            image_turn_id,
        )
        .await
        .map_err(ImageAttemptError::into_error)
    }

    pub async fn edit_image(
        &self,
        request: &ImageEditRequest,
    ) -> Result<CodexImageResult, CodexImageError> {
        validate_prompt(&request.prompt)?;
        validate_references(&request.images)?;
        self.execute_image_automatically(
            ImageOperation::Edit,
            edit_body(request),
            ImageTurnId::automatic()?,
        )
        .await
    }

    pub async fn edit_image_on_home(
        &self,
        home_id: &str,
        image_turn_id: &ImageTurnId,
        request: &ImageEditRequest,
    ) -> Result<CodexImageResult, CodexImageError> {
        validate_prompt(&request.prompt)?;
        validate_references(&request.images)?;
        let (home, slot) = self.select_exact_image_home(home_id).await?;
        self.execute_image(
            home,
            slot,
            ImageOperation::Edit,
            &edit_body(request),
            image_turn_id,
        )
        .await
        .map_err(ImageAttemptError::into_error)
    }

    async fn execute_image_automatically(
        &self,
        operation: ImageOperation,
        body: Value,
        image_turn_id: ImageTurnId,
    ) -> Result<CodexImageResult, CodexImageError> {
        let mut tried = Vec::new();
        let mut last_rejection = None;
        loop {
            let (home, slot) = match self.select_home(&tried, None, &[], false, true, None).await {
                HomeSelection::Ready(home, slot) => (home, slot),
                HomeSelection::Unavailable { .. } => {
                    return Err(last_rejection.unwrap_or(CodexImageError::Unavailable));
                }
            };
            tried.push(home.id().to_string());
            match self
                .execute_image(home, slot, operation, &body, &image_turn_id)
                .await
            {
                Ok(result) => return Ok(result),
                Err(ImageAttemptError::Terminal(error)) => return Err(error),
                Err(ImageAttemptError::Rotate(error)) => last_rejection = Some(error),
            }
        }
    }

    async fn select_exact_image_home(
        &self,
        exact_id: &str,
    ) -> Result<(Arc<CodexHome>, TurnSlot), CodexImageError> {
        if self
            .shutting_down
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(CodexImageError::Unavailable);
        }
        let now = pool::now();
        let home = self
            .homes()
            .await
            .into_iter()
            .find(|home| home.id() == exact_id)
            .ok_or(CodexImageError::Unavailable)?;
        let limits = home.rate_limits().await;
        if !home.admission(limits.as_ref(), now).is_admitted() {
            return Err(CodexImageError::Unavailable);
        }
        let slot = home.acquire_turn().ok_or(CodexImageError::Unavailable)?;
        Ok((home, slot))
    }

    async fn execute_image(
        &self,
        home: Arc<CodexHome>,
        _slot: TurnSlot,
        operation: ImageOperation,
        body: &Value,
        image_turn_id: &ImageTurnId,
    ) -> Result<CodexImageResult, ImageAttemptError> {
        let deadline = (home.config().turn_timeout_ms > 0).then(|| {
            tokio::time::Instant::now()
                + std::time::Duration::from_millis(home.config().turn_timeout_ms)
        });
        let url = match operation {
            ImageOperation::Generation => home.config().image_generations_url(),
            ImageOperation::Edit => home.config().image_edits_url(),
        };
        let token = match home.access_token().await {
            Ok(token) => token,
            Err(error) => {
                note_pre_dispatch_error(&home, &error);
                return Err(ImageAttemptError::Terminal(map_pre_dispatch_error(error)));
            }
        };
        let rejected_token = token.clone();
        let mut sensitive_tokens = vec![token.clone()];
        let credential = home.credential.lock().await;
        let account_id = credential.account_id.clone();
        let mut sensitive_refresh_tokens = vec![credential.refresh_token.clone()];
        drop(credential);
        let auth = AuthContext {
            access_token: token,
            account_id: account_id.clone(),
        };
        let mut response =
            dispatch_image(&home, &auth, url.clone(), body, image_turn_id, deadline).await?;
        if response.status().as_u16() == 401 {
            let context = response_context(
                &response,
                &sensitive_tokens,
                &account_id,
                &sensitive_refresh_tokens,
            );
            if let Err(failure) = read_bounded(response, deadline, MAX_ERROR_BODY_BYTES).await {
                note_read_failure(&home, failure);
                return Err(ImageAttemptError::Terminal(
                    CodexImageError::OutcomeUnknown(Some(context)),
                ));
            }
            let token = match home
                .access_token_after_rejection(rejected_token.as_str())
                .await
            {
                Ok(token) => token,
                Err(error) => {
                    note_pre_dispatch_error(&home, &error);
                    return Err(ImageAttemptError::Terminal(
                        CodexImageError::AuthenticationRequired(context),
                    ));
                }
            };
            sensitive_tokens.push(token.clone());
            let credential = home.credential.lock().await;
            if !sensitive_refresh_tokens
                .iter()
                .any(|secret| secret == &credential.refresh_token)
            {
                sensitive_refresh_tokens.push(credential.refresh_token.clone());
            }
            drop(credential);
            let auth = AuthContext {
                access_token: token,
                account_id: account_id.clone(),
            };
            response = dispatch_image(&home, &auth, url, body, image_turn_id, deadline).await?;
        }
        let status = response.status().as_u16();
        let context = response_context(
            &response,
            &sensitive_tokens,
            &account_id,
            &sensitive_refresh_tokens,
        );
        if !response.status().is_success() {
            let retry_after = super::transport::retry_after_seconds(response.headers());
            if let Err(failure) = read_bounded(response, deadline, MAX_ERROR_BODY_BYTES).await {
                note_read_failure(&home, failure);
                return Err(ImageAttemptError::Terminal(
                    CodexImageError::OutcomeUnknown(Some(context)),
                ));
            }
            let process_error = match status {
                400 | 404 | 409 | 422 => ProcessError::BadRequest,
                401 | 403 => ProcessError::AuthenticationRequired,
                429 => ProcessError::UsageLimitExceeded { retry_after },
                _ => ProcessError::Timeout("image upstream status"),
            };
            home.note_turn_error(&process_error);
            return Err(match status {
                401 | 403 => {
                    ImageAttemptError::Rotate(CodexImageError::AuthenticationRequired(context))
                }
                429 => ImageAttemptError::Rotate(CodexImageError::UsageLimit(context)),
                400 | 404 | 409 | 422 => {
                    ImageAttemptError::Terminal(CodexImageError::BadRequest(context))
                }
                _ => ImageAttemptError::Terminal(CodexImageError::Status(context)),
            });
        }
        let response_body = read_bounded(response, deadline, MAX_RESPONSE_BYTES)
            .await
            .map_err(|failure| {
                note_read_failure(&home, failure);
                ImageAttemptError::Terminal(match failure {
                    ReadFailure::Timeout => CodexImageError::ResponseTimeout(Some(context.clone())),
                    ReadFailure::Closed => CodexImageError::ResponseBodyClosed(context.clone()),
                    ReadFailure::Protocol => CodexImageError::InvalidResponse(context.clone()),
                })
            })?;
        let result = parse_success(
            &response_body,
            context.clone(),
            home.id().to_string(),
            image_turn_id.clone(),
        )
        .map_err(|error| {
            home.note_turn_error(&ProcessError::Protocol(
                "image success response invalid".to_string(),
            ));
            ImageAttemptError::Terminal(error)
        })?;
        home.mark_turn_healthy();
        Ok(result)
    }
}

fn generation_body(request: &ImageGenerationRequest) -> Value {
    json!({
        "prompt": request.prompt,
        "background": request.background.as_str(),
        "model": GPT_IMAGE_2,
        "quality": request.quality.as_str(),
        "size": request.size.wire_value()
    })
}

fn edit_body(request: &ImageEditRequest) -> Value {
    json!({
        "prompt": request.prompt,
        "background": request.background.as_str(),
        "model": GPT_IMAGE_2,
        "quality": request.quality.as_str(),
        "size": request.size.wire_value(),
        "images": request.images.iter().map(|image| json!({
            "image_url": format!("data:image/png;base64,{}", STANDARD_BASE64.encode(&image.bytes))
        })).collect::<Vec<_>>()
    })
}

async fn dispatch_image(
    home: &CodexHome,
    auth: &AuthContext,
    url: String,
    body: &Value,
    image_turn_id: &ImageTurnId,
    deadline: Option<tokio::time::Instant>,
) -> Result<wreq::Response, ImageAttemptError> {
    match home
        .transport()
        .run_image_request(auth, url, body, image_turn_id.as_str(), deadline)
        .await
    {
        Ok(response) => Ok(response),
        Err(ImageDispatchError::PreDispatch(error)) => {
            note_pre_dispatch_error(home, &error);
            Err(ImageAttemptError::Terminal(map_pre_dispatch_error(error)))
        }
        Err(ImageDispatchError::Ambiguous(error)) => {
            home.note_turn_error(&error);
            Err(ImageAttemptError::Terminal(match error {
                ProcessError::Timeout(_) => CodexImageError::ResponseTimeout(None),
                _ => CodexImageError::OutcomeUnknown(None),
            }))
        }
    }
}

fn note_pre_dispatch_error(home: &CodexHome, error: &ProcessError) {
    if !matches!(error, ProcessError::InvalidConfig(_)) {
        home.note_turn_error(error);
    }
}

fn map_pre_dispatch_error(error: ProcessError) -> CodexImageError {
    match error {
        ProcessError::AuthenticationRequired | ProcessError::SubscriptionRequired => {
            CodexImageError::AuthenticationRequired(ImageErrorContext {
                status: 401,
                request_id: None,
            })
        }
        ProcessError::UsageLimitExceeded { .. } => CodexImageError::UsageLimit(ImageErrorContext {
            status: 429,
            request_id: None,
        }),
        _ => CodexImageError::Unavailable,
    }
}

fn response_context(
    response: &wreq::Response,
    access_tokens: &[codex_credential::SecretString],
    account_id: &str,
    refresh_tokens: &[String],
) -> ImageErrorContext {
    ImageErrorContext {
        status: response.status().as_u16(),
        request_id: sanitized_request_id(
            response.headers(),
            access_tokens,
            account_id,
            refresh_tokens,
        ),
    }
}

fn sanitized_request_id(
    headers: &wreq::header::HeaderMap,
    access_tokens: &[codex_credential::SecretString],
    account_id: &str,
    refresh_tokens: &[String],
) -> Option<String> {
    let raw = headers
        .get("x-request-id")
        .or_else(|| headers.get("request-id"))?
        .to_str()
        .ok()?;
    if raw.contains(account_id)
        || refresh_tokens.iter().any(|token| raw.contains(token))
        || access_tokens
            .iter()
            .any(|token| raw.contains(token.as_str()))
    {
        return None;
    }
    let value: String = raw
        .chars()
        .take(MAX_REQUEST_ID_CHARS)
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ':') {
                character
            } else {
                '_'
            }
        })
        .collect();
    (!value.is_empty()).then_some(value)
}

#[derive(Clone, Copy)]
enum ReadFailure {
    Timeout,
    Closed,
    Protocol,
}

fn note_read_failure(home: &CodexHome, failure: ReadFailure) {
    match failure {
        ReadFailure::Timeout => home.note_turn_error(&ProcessError::Timeout("image response body")),
        ReadFailure::Closed => home.note_turn_error(&ProcessError::Closed),
        ReadFailure::Protocol => home.note_turn_error(&ProcessError::Protocol(
            "image response exceeded limit".to_string(),
        )),
    }
}

async fn read_bounded(
    response: wreq::Response,
    deadline: Option<tokio::time::Instant>,
    limit: usize,
) -> Result<Vec<u8>, ReadFailure> {
    if let Some(length) = response.content_length() {
        if length > limit as u64 {
            return Err(ReadFailure::Protocol);
        }
    }
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    loop {
        let chunk = match deadline {
            Some(deadline) => tokio::time::timeout_at(deadline, stream.next())
                .await
                .map_err(|_| ReadFailure::Timeout)?,
            None => stream.next().await,
        };
        let Some(chunk) = chunk else {
            break;
        };
        let chunk = chunk.map_err(|error| {
            if error.is_timeout() {
                ReadFailure::Timeout
            } else {
                ReadFailure::Closed
            }
        })?;
        if body
            .len()
            .checked_add(chunk.len())
            .is_none_or(|len| len > limit)
        {
            return Err(ReadFailure::Protocol);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[derive(Deserialize)]
struct SuccessEnvelope {
    created: u64,
    background: String,
    data: Vec<SuccessData>,
    quality: String,
    size: String,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    output_format: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    usage: Option<Value>,
}

#[derive(Deserialize)]
struct SuccessData {
    b64_json: String,
}

fn deserialize_optional_non_null<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

fn parse_success(
    body: &[u8],
    context: ImageErrorContext,
    home_id: String,
    image_turn_id: ImageTurnId,
) -> Result<CodexImageResult, CodexImageError> {
    let envelope: SuccessEnvelope = serde_json::from_slice(body)
        .map_err(|_| CodexImageError::InvalidResponse(context.clone()))?;
    if !(MIN_CREATED_UNIX_SECONDS..=MAX_CREATED_UNIX_SECONDS).contains(&envelope.created)
        || !valid_metadata(&envelope.background)
        || !valid_metadata(&envelope.quality)
        || !valid_metadata(&envelope.size)
        || envelope
            .output_format
            .as_deref()
            .is_some_and(|value| value != "png")
        || envelope.usage.as_ref().is_some_and(Value::is_null)
        || envelope.data.len() != 1
    {
        return Err(CodexImageError::InvalidResponse(context));
    }
    let encoded = envelope.data[0].b64_json.as_bytes();
    let decoded_len = canonical_base64_decoded_len(encoded)
        .filter(|length| *length <= MAX_IMAGE_STORAGE_BYTES)
        .ok_or_else(|| CodexImageError::InvalidResponse(context.clone()))?;
    let png = STANDARD_BASE64
        .decode(encoded)
        .map_err(|_| CodexImageError::InvalidResponse(context.clone()))?;
    if png.len() != decoded_len {
        return Err(CodexImageError::InvalidResponse(context));
    }
    let metadata =
        decode_png_metadata(&png).map_err(|_| CodexImageError::InvalidResponse(context.clone()))?;
    Ok(CodexImageResult {
        png,
        width: metadata.width,
        height: metadata.height,
        created: envelope.created,
        background: envelope.background,
        quality: envelope.quality,
        size: envelope.size,
        output_format: envelope.output_format,
        usage: envelope.usage,
        request_id: context.request_id,
        home_id,
        image_turn_id,
    })
}

fn valid_metadata(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

fn validate_prompt(prompt: &str) -> Result<(), CodexImageError> {
    if prompt.is_empty()
        || prompt.len() > MAX_PROMPT_BYTES
        || prompt.chars().count() > MAX_PROMPT_CHARS
    {
        return Err(validation(
            "prompt must be nonempty, at most 32000 Unicode characters and at most 128 KiB",
        ));
    }
    Ok(())
}

fn validate_references(images: &[ImageReference]) -> Result<(), CodexImageError> {
    if images.is_empty() || images.len() > 5 {
        return Err(validation("edit requires 1..=5 PNG reference images"));
    }
    let storage = images
        .iter()
        .try_fold(0usize, |sum, image| sum.checked_add(image.bytes.len()));
    let decoded = images
        .iter()
        .try_fold(0usize, |sum, image| sum.checked_add(image.decoded_bytes));
    if storage.is_none_or(|bytes| bytes > MAX_EDIT_STORAGE_BYTES) {
        return Err(validation("aggregate PNG storage must not exceed 32 MiB"));
    }
    if decoded.is_none_or(|bytes| bytes > MAX_EDIT_DECODED_BYTES) {
        return Err(validation("aggregate decoded PNGs must not exceed 64 MiB"));
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct PngMetadata {
    width: u32,
    height: u32,
    decoded_bytes: usize,
}

fn decode_png_metadata(bytes: &[u8]) -> Result<PngMetadata, &'static str> {
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_STORAGE_BYTES {
        return Err("PNG storage must be within 1..=16 MiB");
    }
    validate_terminal_iend(bytes)?;
    let limits = png::Limits {
        bytes: MAX_IMAGE_DECODED_BYTES,
    };
    let mut decoder = png::Decoder::new_with_limits(Cursor::new(bytes), limits);
    decoder.set_transformations(png::Transformations::IDENTITY);
    let mut reader = decoder.read_info().map_err(|_| "invalid PNG")?;
    let info = reader.info();
    if info.animation_control.is_some() || info.frame_control.is_some() {
        return Err("animated PNG is not supported");
    }
    if info.width == 0
        || info.height == 0
        || info.width > MAX_DIMENSION
        || info.height > MAX_DIMENSION
    {
        return Err("PNG dimensions must be within 1..=4096");
    }
    let width = info.width;
    let height = info.height;
    let output_size = reader
        .output_buffer_size()
        .filter(|size| *size <= MAX_IMAGE_DECODED_BYTES)
        .ok_or("decoded PNG must not exceed 16 MiB")?;
    let mut pixels = vec![0; output_size];
    let frame = reader.next_frame(&mut pixels).map_err(|_| "invalid PNG")?;
    reader.finish().map_err(|_| "invalid PNG")?;
    Ok(PngMetadata {
        width,
        height,
        decoded_bytes: frame.buffer_size(),
    })
}

fn validate_terminal_iend(bytes: &[u8]) -> Result<(), &'static str> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if !bytes.starts_with(PNG_SIGNATURE) {
        return Err("invalid PNG");
    }
    let mut offset = PNG_SIGNATURE.len();
    loop {
        let header_end = offset.checked_add(8).ok_or("invalid PNG")?;
        let header = bytes.get(offset..header_end).ok_or("invalid PNG")?;
        let length =
            u32::from_be_bytes(header[..4].try_into().map_err(|_| "invalid PNG")?) as usize;
        let chunk_end = header_end
            .checked_add(length)
            .and_then(|end| end.checked_add(4))
            .ok_or("invalid PNG")?;
        if chunk_end > bytes.len() {
            return Err("invalid PNG");
        }
        if &header[4..8] == b"IEND" {
            return if length == 0 && chunk_end == bytes.len() {
                Ok(())
            } else {
                Err("PNG must end at IEND")
            };
        }
        offset = chunk_end;
    }
}

fn canonical_base64_decoded_len(encoded: &[u8]) -> Option<usize> {
    if encoded.is_empty() || encoded.len() % 4 != 0 {
        return None;
    }
    let padding = if encoded.ends_with(b"==") {
        2
    } else if encoded.ends_with(b"=") {
        1
    } else {
        0
    };
    let data_len = encoded.len().checked_sub(padding)?;
    if encoded[..data_len]
        .iter()
        .any(|byte| base64_sextet(*byte).is_none())
        || encoded[data_len..].iter().any(|byte| *byte != b'=')
        || encoded[..data_len].contains(&b'=')
    {
        return None;
    }
    match padding {
        0 => {}
        1 if data_len % 4 == 3 && base64_sextet(encoded[data_len - 1])? & 0b11 == 0 => {}
        2 if data_len % 4 == 2 && base64_sextet(encoded[data_len - 1])? & 0b1111 == 0 => {}
        _ => return None,
    }
    encoded
        .len()
        .checked_div(4)?
        .checked_mul(3)?
        .checked_sub(padding)
}

fn base64_sextet(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn validation(message: &'static str) -> CodexImageError {
    CodexImageError::Validation(message)
}

#[cfg(test)]
mod tests;
