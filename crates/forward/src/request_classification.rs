//! Privacy-bounded v1 structural request classification.
//!
//! Classifiers in this module are pure. Native Anthropic count-tokens, OpenAI Responses input-token
//! counting, Anthropic-plane OpenAI Chat/Responses, and Gemini native/universal generation are their
//! narrow runtime consumers; remaining stage 6/7 producer integration is incomplete. They accept only request values
//! already admitted by the owning parser; they
//! never retain arbitrary strings or request content.

use registry::request_facts::{
    MODALITY_AUDIO, MODALITY_IMAGE, MODALITY_PDF, MODALITY_TEXT, MODALITY_VIDEO,
    TOOL_CLASS_CODE_EXECUTION, TOOL_CLASS_COMPUTER, TOOL_CLASS_CUSTOM_FUNCTION,
    TOOL_CLASS_CUSTOM_TOOL, TOOL_CLASS_MCP, TOOL_CLASS_OTHER_REVIEWED, TOOL_CLASS_WEB_SEARCH,
};
use serde_json::{Map, Value};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ToolClass {
    CustomFunction,
    CustomTool,
    WebSearch,
    Computer,
    CodeExecution,
    Mcp,
    OtherReviewed,
}

impl ToolClass {
    const fn bit(self) -> i32 {
        match self {
            Self::CustomFunction => TOOL_CLASS_CUSTOM_FUNCTION,
            Self::CustomTool => TOOL_CLASS_CUSTOM_TOOL,
            Self::WebSearch => TOOL_CLASS_WEB_SEARCH,
            Self::Computer => TOOL_CLASS_COMPUTER,
            Self::CodeExecution => TOOL_CLASS_CODE_EXECUTION,
            Self::Mcp => TOOL_CLASS_MCP,
            Self::OtherReviewed => TOOL_CLASS_OTHER_REVIEWED,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ToolChoiceMode {
    Auto,
    Required,
    None,
    Named,
    Unknown,
}

impl From<ToolChoiceMode> for registry::request_facts::ToolChoiceMode {
    fn from(value: ToolChoiceMode) -> Self {
        match value {
            ToolChoiceMode::Auto => Self::Auto,
            ToolChoiceMode::Required => Self::Required,
            ToolChoiceMode::None => Self::None,
            ToolChoiceMode::Named => Self::Named,
            ToolChoiceMode::Unknown => Self::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ServiceTier {
    Standard,
    Priority,
}

impl ServiceTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Priority => "priority",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Modality {
    Text,
    Image,
    Audio,
    Video,
    Pdf,
}

impl Modality {
    const fn bit(self) -> i32 {
        match self {
            Self::Text => MODALITY_TEXT,
            Self::Image => MODALITY_IMAGE,
            Self::Audio => MODALITY_AUDIO,
            Self::Video => MODALITY_VIDEO,
            Self::Pdf => MODALITY_PDF,
        }
    }
}

/// Closed structural evidence. Private fields prevent downstream code from manufacturing strings.
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct RequestClassification {
    tools_declared_count: Option<i32>,
    tool_classes: Option<i32>,
    tool_choice_mode: Option<ToolChoiceMode>,
    parallel_tools_requested: Option<bool>,
    tool_results_in_input: Option<bool>,
    structured_output_flag: Option<bool>,
    reasoning_flag: Option<bool>,
    service_tier: Option<ServiceTier>,
    input_modalities: Option<i32>,
    output_modalities: Option<i32>,
}

impl RequestClassification {
    pub(crate) fn tools_declared_count(&self) -> Option<i32> {
        self.tools_declared_count
    }

    pub(crate) fn tool_classes(&self) -> Option<i32> {
        self.tool_classes
    }

    pub(crate) fn tool_choice_mode(&self) -> Option<registry::request_facts::ToolChoiceMode> {
        self.tool_choice_mode.map(Into::into)
    }

    pub(crate) fn parallel_tools_requested(&self) -> Option<bool> {
        self.parallel_tools_requested
    }

    pub(crate) fn tool_results_in_input(&self) -> Option<bool> {
        self.tool_results_in_input
    }

    pub(crate) fn structured_output_flag(&self) -> Option<bool> {
        self.structured_output_flag
    }

    pub(crate) fn reasoning_flag(&self) -> Option<bool> {
        self.reasoning_flag
    }

    pub(crate) fn service_tier(&self) -> Option<&'static str> {
        self.service_tier.map(ServiceTier::as_str)
    }

    pub(crate) fn input_modalities(&self) -> Option<i32> {
        self.input_modalities
    }

    pub(crate) fn output_modalities(&self) -> Option<i32> {
        self.output_modalities
    }
}

impl Default for RequestClassification {
    fn default() -> Self {
        Self {
            tools_declared_count: None,
            tool_classes: None,
            tool_choice_mode: None,
            parallel_tools_requested: None,
            tool_results_in_input: None,
            structured_output_flag: None,
            reasoning_flag: None,
            service_tier: None,
            input_modalities: None,
            output_modalities: None,
        }
    }
}

impl fmt::Debug for RequestClassification {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RequestClassification(<redacted>)")
    }
}

#[derive(Default)]
pub(crate) struct RequestClassificationBuilder(RequestClassification);

impl RequestClassificationBuilder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// `None` is deliberately preserved when the source length cannot fit the durable i32 field.
    pub(crate) fn tools_reviewed(
        mut self,
        count: usize,
        classes: impl IntoIterator<Item = ToolClass>,
    ) -> Self {
        if let Ok(count) = i32::try_from(count) {
            self.0.tools_declared_count = Some(count);
            self.0.tool_classes = Some(
                classes
                    .into_iter()
                    .fold(0, |bits, class| bits | class.bit()),
            );
        }
        self
    }

    /// Preserve a measurable declaration count without claiming an exhaustive class bitset.
    pub(crate) fn tools_unreviewed(mut self, count: usize) -> Self {
        self.0.tools_declared_count = i32::try_from(count).ok();
        self
    }

    pub(crate) fn tool_choice(mut self, value: ToolChoiceMode) -> Self {
        self.0.tool_choice_mode = Some(value);
        self
    }

    pub(crate) fn parallel_tools_requested(mut self, value: bool) -> Self {
        self.0.parallel_tools_requested = Some(value);
        self
    }

    pub(crate) fn tool_results_in_input(mut self, value: bool) -> Self {
        self.0.tool_results_in_input = Some(value);
        self
    }

    pub(crate) fn structured_output(mut self, value: bool) -> Self {
        self.0.structured_output_flag = Some(value);
        self
    }

    pub(crate) fn reasoning(mut self, value: bool) -> Self {
        self.0.reasoning_flag = Some(value);
        self
    }

    pub(crate) fn service_tier(mut self, value: ServiceTier) -> Self {
        self.0.service_tier = Some(value);
        self
    }

    pub(crate) fn input_modalities(mut self, values: impl IntoIterator<Item = Modality>) -> Self {
        self.0.input_modalities = Some(
            values
                .into_iter()
                .fold(0, |bits, modality| bits | modality.bit()),
        );
        self
    }

    pub(crate) fn output_modalities(mut self, values: impl IntoIterator<Item = Modality>) -> Self {
        self.0.output_modalities = Some(
            values
                .into_iter()
                .fold(0, |bits, modality| bits | modality.bit()),
        );
        self
    }

    pub(crate) fn build(self) -> RequestClassification {
        self.0
    }
}

/// Pure classifier for an already-validated Anthropic Messages shape.
///
/// This covers accepted client Messages shapes before any universal adapter translation.
pub(crate) fn classify_anthropic_messages(request: &Value) -> RequestClassification {
    let Some(object) = request.as_object() else {
        return RequestClassification::default();
    };
    classify_anthropic_messages_object(object)
}

/// Pure classifier for an already-validated OpenAI Chat Completions shape.
///
/// This covers accepted client Chat shapes on every provider plane before translation.
pub(crate) fn classify_openai_chat(request: &Value) -> RequestClassification {
    let Some(object) = request.as_object() else {
        return RequestClassification::default();
    };
    classify_openai_chat_object(object)
}

/// Pure classifier for an already-validated OpenAI Responses shape.
///
/// This covers accepted client Responses shapes on every provider plane before translation.
pub(crate) fn classify_openai_responses(request: &Value) -> RequestClassification {
    let Some(object) = request.as_object() else {
        return RequestClassification::default();
    };
    classify_openai_responses_object(object)
}

/// Pure classifier for an already-canonicalized, already-validated Gemini GenerateContent shape.
///
/// This covers canonical native Gemini GenerateContent client shapes before transport wrapping.
/// `countTokens` callers pass the nested `generateContentRequest` object when it is present, just
/// as the owning native parser does before validating generation controls.
pub(crate) fn classify_gemini_generate_content(request: &Value) -> RequestClassification {
    let Some(object) = request.as_object() else {
        return RequestClassification::default();
    };
    classify_gemini_object(object)
}

fn classify_anthropic_messages_object(object: &Map<String, Value>) -> RequestClassification {
    let mut builder = RequestClassificationBuilder::new();
    if let Some(tools) = explicit_array(object, "tools") {
        let classes = tools
            .iter()
            .map(anthropic_tool_class)
            .collect::<Option<Vec<_>>>();
        builder = match classes {
            Some(classes) => builder.tools_reviewed(tools.len(), classes),
            None => builder.tools_unreviewed(tools.len()),
        };
    }
    if let Some(choice) = object.get("tool_choice").filter(|value| !value.is_null()) {
        builder = builder.tool_choice(match choice.get("type").and_then(Value::as_str) {
            Some("auto") => ToolChoiceMode::Auto,
            Some("any") => ToolChoiceMode::Required,
            Some("none") => ToolChoiceMode::None,
            Some("tool") if choice.get("name").and_then(Value::as_str).is_some() => {
                ToolChoiceMode::Named
            }
            _ => ToolChoiceMode::Unknown,
        });
        if let Some(disabled) = choice
            .get("disable_parallel_tool_use")
            .and_then(Value::as_bool)
        {
            builder = builder.parallel_tools_requested(!disabled);
        }
    }
    if let Some(has_results) = contains_anthropic_tool_result(object.get("messages")) {
        builder = builder.tool_results_in_input(has_results);
    }
    if let Some(output_config) = object.get("output_config").filter(|value| !value.is_null()) {
        if let Some(output_config) = output_config.as_object() {
            if let Some(structured) = classify_anthropic_output_format(output_config.get("format"))
            {
                builder = builder.structured_output(structured);
            }
            if let Some(reasoning) = classify_effort(output_config.get("effort")) {
                builder = builder.reasoning(reasoning);
            }
        }
    }
    // Native thinking is independent of output_config.format and is the stronger direct signal.
    if let Some(reasoning) = object
        .get("thinking")
        .filter(|value| !value.is_null())
        .and_then(classify_anthropic_thinking)
    {
        builder = builder.reasoning(reasoning);
    }
    if let Some(tier) = anthropic_service_tier(object) {
        builder = builder.service_tier(tier);
    }
    if let Some(modalities) = classify_anthropic_input_modalities(object) {
        builder = builder.input_modalities(modalities);
    }
    builder.build()
}

fn anthropic_tool_class(tool: &Value) -> Option<ToolClass> {
    match tool.get("type") {
        None | Some(Value::Null) => Some(ToolClass::CustomFunction),
        Some(Value::String(kind)) => match kind.as_str() {
            "custom" => Some(ToolClass::CustomFunction),
            "web_search_20250305" => Some(ToolClass::WebSearch),
            "computer_20250124" => Some(ToolClass::Computer),
            "code_execution_20250522" => Some(ToolClass::CodeExecution),
            // No current native validator proves an MCP tool-type vocabulary. In particular,
            // neither names nor schemas can manufacture the MCP bit.
            _ => None,
        },
        Some(_) => None,
    }
}

fn classify_anthropic_input_modalities(object: &Map<String, Value>) -> Option<Vec<Modality>> {
    let system = object.get("system");
    let messages = object.get("messages");
    if system.is_none() && messages.is_none() {
        return None;
    }
    let mut modalities = Vec::new();
    if let Some(system) = system {
        classify_anthropic_content(system, &mut modalities)?;
    }
    if let Some(messages) = messages {
        let messages = messages.as_array()?;
        for message in messages {
            classify_anthropic_content(message.get("content")?, &mut modalities)?;
        }
    }
    Some(modalities)
}

fn classify_anthropic_content(value: &Value, modalities: &mut Vec<Modality>) -> Option<()> {
    match value {
        Value::String(_) => push_unique(modalities, Modality::Text),
        Value::Array(blocks) => {
            for block in blocks {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => push_unique(modalities, Modality::Text),
                    Some("image") => push_unique(modalities, Modality::Image),
                    Some("document") => push_unique(modalities, Modality::Pdf),
                    // These reviewed blocks are structural history/control, not input modalities.
                    // In particular, tool_result content is deliberately not traversed.
                    Some(
                        "tool_use"
                        | "tool_result"
                        | "server_tool_use"
                        | "web_search_tool_result"
                        | "thinking"
                        | "redacted_thinking",
                    ) => {}
                    _ => return None,
                }
            }
        }
        _ => return None,
    }
    Some(())
}

fn anthropic_service_tier(object: &Map<String, Value>) -> Option<ServiceTier> {
    object
        .get("speed")
        .and_then(Value::as_str)
        .filter(|speed| speed.eq_ignore_ascii_case("fast"))
        .map(|_| ServiceTier::Priority)
}

fn classify_openai_chat_object(object: &Map<String, Value>) -> RequestClassification {
    let mut builder = RequestClassificationBuilder::new();
    let tools = explicit_array(object, "tools").or_else(|| explicit_array(object, "functions"));
    if let Some(tools) = tools {
        builder = builder.tools_reviewed(
            tools.len(),
            std::iter::repeat(ToolClass::CustomFunction).take(tools.len()),
        );
    }
    let choice = object
        .get("tool_choice")
        .filter(|value| !value.is_null())
        .or_else(|| object.get("function_call").filter(|value| !value.is_null()));
    if let Some(choice) = choice {
        builder = builder.tool_choice(openai_tool_choice(choice));
    }
    if let Some(parallel) = object.get("parallel_tool_calls").and_then(Value::as_bool) {
        builder = builder.parallel_tools_requested(parallel);
    }
    if let Some(has_results) = contains_openai_chat_tool_result(object.get("messages")) {
        builder = builder.tool_results_in_input(has_results);
    }
    if let Some(structured) = classify_openai_output_format(
        object
            .get("response_format")
            .filter(|value| !value.is_null()),
    ) {
        builder = builder.structured_output(structured);
    }
    if let Some(reasoning) = classify_effort(object.get("reasoning_effort")) {
        builder = builder.reasoning(reasoning);
    }
    if let Some(tier) = openai_service_tier(object.get("service_tier")) {
        builder = builder.service_tier(tier);
    }
    if let Some(modalities) = classify_openai_chat_input_modalities(object.get("messages")) {
        builder = builder.input_modalities(modalities);
    }
    if let Some(modalities) = classify_openai_output_modalities(object.get("modalities")) {
        builder = builder.output_modalities(modalities);
    }
    builder.build()
}

fn classify_openai_responses_object(object: &Map<String, Value>) -> RequestClassification {
    let mut builder = RequestClassificationBuilder::new();
    if let Some(tools) = openai_responses_declared_tools(object) {
        let classes = tools
            .iter()
            .map(openai_responses_tool_classes)
            .collect::<Option<Vec<_>>>()
            .map(|classes| classes.into_iter().flatten());
        builder = match classes {
            Some(classes) => builder.tools_reviewed(tools.len(), classes),
            None => builder.tools_unreviewed(tools.len()),
        };
    }
    if let Some(choice) = object.get("tool_choice").filter(|value| !value.is_null()) {
        builder = builder.tool_choice(openai_tool_choice(choice));
    }
    if let Some(parallel) = object.get("parallel_tool_calls").and_then(Value::as_bool) {
        builder = builder.parallel_tools_requested(parallel);
    }
    if let Some(has_results) = contains_openai_responses_tool_result(object.get("input")) {
        builder = builder.tool_results_in_input(has_results);
    }
    if let Some(structured) = object
        .get("text")
        .filter(|value| !value.is_null())
        .and_then(|text| classify_openai_output_format(text.get("format")))
    {
        builder = builder.structured_output(structured);
    }
    if let Some(reasoning) = object
        .get("reasoning")
        .and_then(|reasoning| classify_effort(reasoning.get("effort")))
    {
        builder = builder.reasoning(reasoning);
    }
    if let Some(tier) = openai_service_tier(object.get("service_tier")) {
        builder = builder.service_tier(tier);
    }
    if let Some(modalities) = classify_openai_responses_input_modalities(object) {
        builder = builder.input_modalities(modalities);
    }
    // The supported text Responses parser has no validated output-modalities field.
    builder.build()
}

fn openai_responses_declared_tools(object: &Map<String, Value>) -> Option<&Vec<Value>> {
    let additional = object
        .get("input")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|item| item.get("type").and_then(Value::as_str) == Some("additional_tools"))
        .and_then(|item| item.get("tools"))
        .and_then(Value::as_array);
    additional.or_else(|| explicit_array(object, "tools"))
}

fn openai_responses_tool_classes(tool: &Value) -> Option<Vec<ToolClass>> {
    match tool.get("type").and_then(Value::as_str) {
        Some("function") => Some(vec![ToolClass::CustomFunction]),
        Some("custom") => Some(vec![ToolClass::CustomTool]),
        // Hosted web_search is forwarded to the Codex backend. This records the declared
        // client intent; settlement later counts completed web_search_call items.
        Some("web_search") => Some(vec![ToolClass::WebSearch]),
        // Explicit `tool_search` declaration. Client-executed form (`execution:"client"`) is
        // later rewritten to `__codex_client_tool_search`; hosted form (omitted/`server`/`hosted`)
        // stays `type:tool_search`. Classification records the declared type, not the rewrite.
        // The synthetic function name is not counted again.
        Some("tool_search") => Some(vec![ToolClass::OtherReviewed]),
        // A namespace is one top-level declaration, while its reviewed callable children retain
        // their actual function/custom classes. No namespace names or child names are retained.
        Some("namespace") => tool
            .get("tools")
            .and_then(Value::as_array)?
            .iter()
            .map(|child| match child.get("type").and_then(Value::as_str) {
                Some("function") => Some(ToolClass::CustomFunction),
                Some("custom") => Some(ToolClass::CustomTool),
                _ => None,
            })
            .collect::<Option<Vec<_>>>(),
        _ => None,
    }
}

fn openai_tool_choice(value: &Value) -> ToolChoiceMode {
    match value {
        Value::String(value) => match value.as_str() {
            "auto" => ToolChoiceMode::Auto,
            "required" => ToolChoiceMode::Required,
            "none" => ToolChoiceMode::None,
            _ => ToolChoiceMode::Unknown,
        },
        Value::Object(object) if object.get("type").and_then(Value::as_str) == Some("function") => {
            ToolChoiceMode::Named
        }
        _ => ToolChoiceMode::Unknown,
    }
}

fn classify_anthropic_output_format(value: Option<&Value>) -> Option<bool> {
    let Some(value) = value else {
        return Some(false);
    };
    match value.get("type").and_then(Value::as_str)? {
        "json_schema" => Some(true),
        _ => None,
    }
}

fn classify_openai_output_format(value: Option<&Value>) -> Option<bool> {
    match value?.get("type").and_then(Value::as_str)? {
        "text" => Some(false),
        "json_object" | "json_schema" => Some(true),
        _ => None,
    }
}

fn classify_effort(value: Option<&Value>) -> Option<bool> {
    match value?.as_str()? {
        "none" => Some(false),
        "minimal" | "low" | "medium" | "high" | "xhigh" | "max" => Some(true),
        _ => None,
    }
}

fn classify_anthropic_thinking(value: &Value) -> Option<bool> {
    match value.get("type").and_then(Value::as_str)? {
        "disabled" => Some(false),
        "enabled" | "adaptive" => Some(true),
        _ => None,
    }
}

fn classify_gemini_thinking(value: &Value) -> Option<bool> {
    let config = value.as_object()?;
    if let Some(level) = config.get("thinkingLevel") {
        return match level.as_str()? {
            "minimal" | "low" | "medium" | "high" => Some(true),
            _ => None,
        };
    }
    if let Some(budget) = config.get("thinkingBudget") {
        return match budget.as_i64()? {
            0 => Some(false),
            -1 => Some(true),
            budget if budget > 0 => Some(true),
            _ => None,
        };
    }
    None
}

fn openai_service_tier(value: Option<&Value>) -> Option<ServiceTier> {
    match value.and_then(Value::as_str) {
        Some("priority" | "fast") => Some(ServiceTier::Priority),
        Some("default" | "standard") => Some(ServiceTier::Standard),
        _ => None,
    }
}

fn classify_gemini_object(object: &Map<String, Value>) -> RequestClassification {
    let mut builder = RequestClassificationBuilder::new();
    if let Some(tools) = explicit_array(object, "tools") {
        let classes = tools
            .iter()
            .map(gemini_tool_classes)
            .collect::<Option<Vec<_>>>()
            .map(|classes| classes.into_iter().flatten());
        builder = match classes {
            Some(classes) => builder.tools_reviewed(tools.len(), classes),
            None => builder.tools_unreviewed(tools.len()),
        };
    }
    if let Some(function_calling) = object
        .get("toolConfig")
        .and_then(|value| value.get("functionCallingConfig"))
    {
        if let Some(mode) = function_calling.get("mode").and_then(Value::as_str) {
            builder = builder.tool_choice(match mode {
                "AUTO" => ToolChoiceMode::Auto,
                "ANY" => {
                    if function_calling
                        .get("allowedFunctionNames")
                        .and_then(Value::as_array)
                        .is_some_and(|names| names.len() == 1)
                    {
                        ToolChoiceMode::Named
                    } else {
                        ToolChoiceMode::Required
                    }
                }
                "NONE" => ToolChoiceMode::None,
                _ => ToolChoiceMode::Unknown,
            });
        }
    }
    if let Some(has_results) = contains_gemini_tool_result(object.get("contents")) {
        builder = builder.tool_results_in_input(has_results);
    }
    if let Some(config) = object.get("generationConfig").and_then(Value::as_object) {
        if let Some(structured) = classify_gemini_structured_output(config) {
            builder = builder.structured_output(structured);
        }
        if let Some(reasoning) = config
            .get("thinkingConfig")
            .and_then(classify_gemini_thinking)
        {
            builder = builder.reasoning(reasoning);
        }
        if let Some(modalities) =
            classify_gemini_output_modalities(config.get("responseModalities"))
        {
            builder = builder.output_modalities(modalities);
        }
    }
    if let Some(modalities) = classify_gemini_input_modalities(object) {
        builder = builder.input_modalities(modalities);
    }
    // Native Gemini explicitly rejects serviceTier, and does not validate a parallel-tools control.
    builder.build()
}

fn classify_gemini_structured_output(config: &Map<String, Value>) -> Option<bool> {
    let schemas = [
        config.get("responseSchema"),
        config.get("responseJsonSchema"),
    ];
    if schemas
        .into_iter()
        .flatten()
        .any(|schema| !schema.is_null())
    {
        return Some(true);
    }
    if schemas.into_iter().flatten().any(Value::is_null) {
        return None;
    }
    match config.get("responseMimeType") {
        Some(Value::String(mime)) if mime == "application/json" => Some(true),
        Some(Value::String(mime)) if mime == "text/plain" => Some(false),
        Some(Value::Null) | Some(_) => None,
        None => None,
    }
}

fn gemini_tool_classes(tool: &Value) -> Option<Vec<ToolClass>> {
    let tool = tool.as_object()?;
    if tool.is_empty() {
        return None;
    }
    tool.keys()
        .map(|key| match key.as_str() {
            "functionDeclarations" => Some(ToolClass::CustomFunction),
            "googleSearch" | "googleSearchRetrieval" => Some(ToolClass::WebSearch),
            "computerUse" => Some(ToolClass::Computer),
            "codeExecution" => Some(ToolClass::CodeExecution),
            // urlContext is a reviewed native Gemini server tool without a narrower v1 class.
            "urlContext" => Some(ToolClass::OtherReviewed),
            _ => None,
        })
        .collect()
}

fn explicit_array<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a Vec<Value>> {
    object
        .get(field)
        .filter(|value| !value.is_null())?
        .as_array()
}

fn contains_anthropic_tool_result(messages: Option<&Value>) -> Option<bool> {
    let messages = messages?.as_array()?;
    let mut unreviewed = false;
    for message in messages {
        match message.get("content") {
            Some(Value::String(_)) => {}
            Some(Value::Array(blocks)) => {
                for block in blocks {
                    match block.get("type").and_then(Value::as_str) {
                        Some("tool_result" | "web_search_tool_result") => return Some(true),
                        Some(
                            "text" | "image" | "document" | "tool_use" | "server_tool_use"
                            | "thinking" | "redacted_thinking",
                        ) => {}
                        _ => unreviewed = true,
                    }
                }
            }
            _ => unreviewed = true,
        }
    }
    (!unreviewed).then_some(false)
}

fn contains_openai_chat_tool_result(messages: Option<&Value>) -> Option<bool> {
    let messages = messages?.as_array()?;
    if messages.iter().any(|message| {
        matches!(
            message.get("role").and_then(Value::as_str),
            Some("tool" | "function")
        )
    }) {
        return Some(true);
    }
    messages
        .iter()
        .all(openai_chat_message_is_reviewed)
        .then_some(false)
}

fn openai_chat_message_is_reviewed(message: &Value) -> bool {
    let Some(role) = message.get("role").and_then(Value::as_str) else {
        return false;
    };
    if !matches!(role, "system" | "developer" | "user" | "assistant") {
        return false;
    }
    match message.get("content") {
        Some(Value::String(_)) => true,
        Some(Value::Array(parts)) => parts.iter().all(|part| {
            matches!(
                part.get("type").and_then(Value::as_str),
                Some("text" | "input_text" | "output_text" | "image_url" | "input_image")
            )
        }),
        Some(Value::Null) if role == "assistant" => true,
        _ => false,
    }
}

fn contains_openai_responses_tool_result(input: Option<&Value>) -> Option<bool> {
    match input? {
        Value::String(_) => Some(false),
        Value::Array(items) => {
            if items.iter().any(|item| {
                matches!(
                    item.get("type").and_then(Value::as_str),
                    Some("function_call_output" | "custom_tool_call_output" | "tool_search_output")
                )
            }) {
                return Some(true);
            }
            items
                .iter()
                .all(openai_responses_item_is_reviewed)
                .then_some(false)
        }
        _ => None,
    }
}

fn openai_responses_item_is_reviewed(item: &Value) -> bool {
    match item.get("type").and_then(Value::as_str) {
        Some("message") => {
            let mut modalities = Vec::new();
            item.get("content").is_some_and(|content| {
                classify_openai_responses_message_content(content, &mut modalities).is_some()
            })
        }
        None if item.get("role").and_then(Value::as_str).is_some() => {
            let mut modalities = Vec::new();
            item.get("content").is_some_and(|content| {
                classify_openai_responses_message_content(content, &mut modalities).is_some()
            })
        }
        Some(
            "reasoning" | "function_call" | "custom_tool_call" | "tool_search_call"
            | "agent_message" | "additional_tools",
        ) => true,
        _ => false,
    }
}

fn contains_gemini_tool_result(contents: Option<&Value>) -> Option<bool> {
    let contents = contents?.as_array()?;
    let mut unreviewed = false;
    for content in contents {
        let Some(parts) = content.get("parts").and_then(Value::as_array) else {
            unreviewed = true;
            continue;
        };
        for part in parts {
            let Some(part) = part.as_object() else {
                unreviewed = true;
                continue;
            };
            if part.contains_key("functionResponse") || part.contains_key("codeExecutionResult") {
                return Some(true);
            }
            if !gemini_part_is_reviewed(part) {
                unreviewed = true;
            }
        }
    }
    (!unreviewed).then_some(false)
}

fn gemini_part_is_reviewed(part: &Map<String, Value>) -> bool {
    !part.is_empty()
        && part.keys().all(|key| {
            matches!(
                key.as_str(),
                "text"
                    | "inlineData"
                    | "fileData"
                    | "functionCall"
                    | "functionResponse"
                    | "executableCode"
                    | "codeExecutionResult"
                    | "thought"
                    | "thoughtSignature"
            )
        })
}

fn classify_openai_chat_input_modalities(messages: Option<&Value>) -> Option<Vec<Modality>> {
    let messages = messages?.as_array()?;
    let mut modalities = Vec::new();
    for message in messages {
        let role = message.get("role").and_then(Value::as_str)?;
        if matches!(role, "tool" | "function") {
            continue;
        }
        match message.get("content") {
            Some(Value::String(_)) => push_unique(&mut modalities, Modality::Text),
            Some(Value::Array(parts)) => {
                for part in parts {
                    match part.get("type").and_then(Value::as_str) {
                        Some("text" | "input_text" | "output_text") => {
                            push_unique(&mut modalities, Modality::Text)
                        }
                        Some("image_url" | "input_image") => {
                            push_unique(&mut modalities, Modality::Image)
                        }
                        _ => return None,
                    }
                }
            }
            Some(Value::Null) if role == "assistant" => {}
            _ => return None,
        }
    }
    Some(modalities)
}

fn classify_openai_responses_input_modalities(
    object: &Map<String, Value>,
) -> Option<Vec<Modality>> {
    let input = object.get("input");
    let instructions = object.get("instructions");
    if input.is_none() && instructions.is_none() {
        return None;
    }
    let mut modalities = Vec::new();
    if let Some(instructions) = instructions {
        if !instructions.is_string() {
            return None;
        }
        push_unique(&mut modalities, Modality::Text);
    }
    let Some(input) = input else {
        return Some(modalities);
    };
    if input.is_string() {
        push_unique(&mut modalities, Modality::Text);
        return Some(modalities);
    }
    for item in input.as_array()? {
        match item.get("type").and_then(Value::as_str) {
            Some("message") | None if item.get("role").and_then(Value::as_str).is_some() => {
                classify_openai_responses_message_content(item.get("content")?, &mut modalities)?;
            }
            // The owning parser always turns a validated agent_message into model-visible text.
            Some("agent_message") => push_unique(&mut modalities, Modality::Text),
            // Reviewed history/result/control items do not themselves prove a content modality.
            Some(
                "reasoning"
                | "additional_tools"
                | "function_call"
                | "function_call_output"
                | "custom_tool_call"
                | "custom_tool_call_output"
                | "tool_search_call"
                | "tool_search_output",
            ) => {}
            // The owning parser deliberately accepts unknown future history item types. Their
            // modality is therefore unmeasured, so no partial bitset is emitted.
            _ => return None,
        }
    }
    Some(modalities)
}

fn classify_openai_responses_message_content(
    content: &Value,
    modalities: &mut Vec<Modality>,
) -> Option<()> {
    match content {
        Value::String(_) => push_unique(modalities, Modality::Text),
        Value::Array(parts) => {
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("input_text" | "output_text" | "text") => {
                        push_unique(modalities, Modality::Text)
                    }
                    Some("input_image" | "image_url") => push_unique(modalities, Modality::Image),
                    _ => return None,
                }
            }
        }
        _ => return None,
    }
    Some(())
}

fn classify_openai_output_modalities(value: Option<&Value>) -> Option<Vec<Modality>> {
    let values = value?.as_array()?;
    let mut modalities = Vec::new();
    for value in values {
        match value.as_str()? {
            "text" => modalities.push(Modality::Text),
            "audio" => modalities.push(Modality::Audio),
            _ => return None,
        }
    }
    Some(modalities)
}

fn classify_gemini_input_modalities(object: &Map<String, Value>) -> Option<Vec<Modality>> {
    let contents = object.get("contents")?.as_array()?;
    let mut modalities = Vec::new();
    for content in contents.iter().chain(object.get("systemInstruction")) {
        let parts = content.get("parts")?.as_array()?;
        for part in parts {
            let part = part.as_object()?;
            if !gemini_part_is_reviewed(part) {
                return None;
            }
            if part.get("text").and_then(Value::as_str).is_some() {
                push_unique(&mut modalities, Modality::Text);
            }
            for media in [part.get("inlineData"), part.get("fileData")]
                .into_iter()
                .flatten()
            {
                match media.get("mimeType").and_then(Value::as_str)? {
                    mime if mime.starts_with("image/") => {
                        push_unique(&mut modalities, Modality::Image)
                    }
                    mime if mime.starts_with("audio/") => {
                        push_unique(&mut modalities, Modality::Audio)
                    }
                    mime if mime.starts_with("video/") => {
                        push_unique(&mut modalities, Modality::Video)
                    }
                    "application/pdf" => push_unique(&mut modalities, Modality::Pdf),
                    _ => return None,
                }
            }
        }
    }
    Some(modalities)
}

fn classify_gemini_output_modalities(value: Option<&Value>) -> Option<Vec<Modality>> {
    let values = value?.as_array()?;
    let mut modalities = Vec::new();
    for value in values {
        match value.as_str()? {
            "TEXT" => modalities.push(Modality::Text),
            "IMAGE" => modalities.push(Modality::Image),
            _ => return None,
        }
    }
    Some(modalities)
}

fn push_unique(values: &mut Vec<Modality>, modality: Modality) {
    if !values.contains(&modality) {
        values.push(modality);
    }
}

#[cfg(test)]
mod tests;
