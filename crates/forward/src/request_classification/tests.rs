use super::*;
use serde_json::json;

const NAME_SENTINEL: &str = "PRIVATE_NAME_SENTINEL_7b8842";
const DESCRIPTION_SENTINEL: &str = "PRIVATE_DESCRIPTION_SENTINEL_a901cf";
const SCHEMA_SENTINEL: &str = "PRIVATE_SCHEMA_SENTINEL_2e3571";
const ARGUMENTS_SENTINEL: &str = "PRIVATE_ARGUMENTS_SENTINEL_5e0cdd";
const RESULT_SENTINEL: &str = "PRIVATE_RESULT_SENTINEL_86bc20";
const MCP_SENTINEL: &str = "PRIVATE_MCP_SENTINEL_eb246d";
const CONTENT_SENTINEL: &str = "PRIVATE_CONTENT_SENTINEL_a41e77";

fn assert_redacted(value: &RequestClassification) {
    let visible = format!("{value:?}");
    assert_eq!(visible, "RequestClassification(<redacted>)");
    for sentinel in [
        NAME_SENTINEL,
        DESCRIPTION_SENTINEL,
        SCHEMA_SENTINEL,
        ARGUMENTS_SENTINEL,
        RESULT_SENTINEL,
        MCP_SENTINEL,
        CONTENT_SENTINEL,
    ] {
        assert!(!visible.contains(sentinel), "leaked {sentinel}");
    }
}

fn has_class(value: &RequestClassification, class: ToolClass) -> Option<bool> {
    value
        .tool_classes()
        .map(|classes| classes & class.bit() != 0)
}

fn assert_class(value: &RequestClassification, class: ToolClass) {
    assert_eq!(has_class(value, class), Some(true), "{class:?}");
}

fn has_input_modality(value: &RequestClassification, modality: Modality) -> Option<bool> {
    value
        .input_modalities()
        .map(|modalities| modalities & modality.bit() != 0)
}

fn has_output_modality(value: &RequestClassification, modality: Modality) -> Option<bool> {
    value
        .output_modalities()
        .map(|modalities| modalities & modality.bit() != 0)
}

#[test]
fn typed_builder_covers_every_closed_bit_combination_and_count_conversion() {
    let all = [
        ToolClass::CustomFunction,
        ToolClass::CustomTool,
        ToolClass::WebSearch,
        ToolClass::Computer,
        ToolClass::CodeExecution,
        ToolClass::Mcp,
        ToolClass::OtherReviewed,
    ];
    for class in all {
        let classified = RequestClassificationBuilder::new()
            .tools_reviewed(1, [class])
            .build();
        assert_eq!(classified.tools_declared_count(), Some(1));
        assert_class(&classified, class);
    }
    let combined = RequestClassificationBuilder::new()
        .tools_reviewed(7, all)
        .tool_choice(ToolChoiceMode::Named)
        .parallel_tools_requested(true)
        .tool_results_in_input(false)
        .structured_output(true)
        .reasoning(false)
        .service_tier(ServiceTier::Priority)
        .input_modalities([
            Modality::Text,
            Modality::Image,
            Modality::Audio,
            Modality::Video,
            Modality::Pdf,
        ])
        .output_modalities([Modality::Text, Modality::Image])
        .build();
    assert_eq!(combined.tools_declared_count(), Some(7));
    assert_eq!(
        combined.tool_classes(),
        Some(registry::request_facts::TOOL_CLASS_MASK)
    );
    assert_eq!(
        combined.tool_choice_mode(),
        Some(ToolChoiceMode::Named.into())
    );
    assert_eq!(combined.parallel_tools_requested(), Some(true));
    assert_eq!(combined.tool_results_in_input(), Some(false));
    assert_eq!(combined.structured_output_flag(), Some(true));
    assert_eq!(combined.reasoning_flag(), Some(false));
    assert_eq!(combined.service_tier(), Some("priority"));
    for modality in [
        Modality::Text,
        Modality::Image,
        Modality::Audio,
        Modality::Video,
        Modality::Pdf,
    ] {
        assert_eq!(has_input_modality(&combined, modality), Some(true));
    }
    assert_eq!(has_output_modality(&combined, Modality::Text), Some(true));
    assert_eq!(has_output_modality(&combined, Modality::Image), Some(true));
    assert_eq!(has_output_modality(&combined, Modality::Audio), Some(false));

    let empty = RequestClassificationBuilder::new()
        .tools_reviewed(0, [])
        .input_modalities([])
        .output_modalities([])
        .build();
    assert_eq!(empty.tools_declared_count(), Some(0));
    assert_eq!(empty.tool_classes(), Some(0));
    assert_eq!(empty.input_modalities(), Some(0));
    assert_eq!(empty.output_modalities(), Some(0));

    let overflow = RequestClassificationBuilder::new()
        .tools_reviewed(i32::MAX as usize + 1, [ToolClass::CustomFunction])
        .build();
    assert_eq!(overflow.tools_declared_count(), None);
    assert_eq!(overflow.tool_classes(), None);
}

#[test]
fn unknown_is_none_and_debug_is_redacted_without_serialization() {
    let unknown = RequestClassification::default();
    assert_eq!(unknown.tools_declared_count(), None);
    assert_eq!(unknown.tool_classes(), None);
    assert_eq!(unknown.tool_choice_mode(), None);
    assert_eq!(unknown.parallel_tools_requested(), None);
    assert_eq!(unknown.tool_results_in_input(), None);
    assert_eq!(unknown.structured_output_flag(), None);
    assert_eq!(unknown.reasoning_flag(), None);
    assert_eq!(unknown.service_tier(), None);
    assert_eq!(unknown.input_modalities(), None);
    assert_eq!(unknown.output_modalities(), None);
    assert_redacted(&unknown);
}

#[test]
fn anthropic_native_maps_reviewed_tools_choices_results_and_flags() {
    let request = json!({
        "model": "claude-opus-4-8",
        "tools": [
            {"name": NAME_SENTINEL, "description": DESCRIPTION_SENTINEL,
             "input_schema": {"private": SCHEMA_SENTINEL}},
            {"type": "web_search_20250305", "name": "web"},
            {"type": "computer_20250124", "name": "computer"},
            {"type": "code_execution_20250522", "name": "code"},
            {"type": "custom", "name": "another_function", "input_schema": {}},
            {"name": "third_function", "input_schema": {}}
        ],
        "tool_choice": {"type": "tool", "name": NAME_SENTINEL,
                        "disable_parallel_tool_use": false},
        "messages": [{"role": "user", "content": [
            {"type": "text", "text": CONTENT_SENTINEL},
            {"type": "image", "source": {"type":"base64", "media_type":"image/png", "data":"AA"}},
            {"type": "document", "source": {"type":"base64", "media_type":"application/pdf", "data":"AA"}},
            {"type": "tool_result", "tool_use_id": "t", "content": RESULT_SENTINEL}
        ]}],
        "output_config": {"format": {"type": "json_schema", "schema": {"x": SCHEMA_SENTINEL}},
                          "effort": "high"},
        "speed": "fast"
    });
    let classified = classify_anthropic_messages(&request);
    assert_eq!(classified.tools_declared_count(), Some(6));
    for class in [
        ToolClass::CustomFunction,
        ToolClass::WebSearch,
        ToolClass::Computer,
        ToolClass::CodeExecution,
    ] {
        assert_class(&classified, class);
    }
    for class in [
        ToolClass::CustomTool,
        ToolClass::Mcp,
        ToolClass::OtherReviewed,
    ] {
        assert_eq!(has_class(&classified, class), Some(false), "{class:?}");
    }
    assert_eq!(
        classified.tool_choice_mode(),
        Some(ToolChoiceMode::Named.into())
    );
    assert_eq!(classified.parallel_tools_requested(), Some(true));
    assert_eq!(classified.tool_results_in_input(), Some(true));
    assert_eq!(classified.structured_output_flag(), Some(true));
    assert_eq!(classified.reasoning_flag(), Some(true));
    assert_eq!(classified.service_tier(), Some("priority"));
    assert_eq!(
        classified.input_modalities(),
        Some(MODALITY_TEXT | MODALITY_IMAGE | MODALITY_PDF)
    );
    assert_redacted(&classified);
}

#[test]
fn anthropic_absent_empty_unknown_and_choice_forms_stay_distinct() {
    let absent = classify_anthropic_messages(&json!({"messages": []}));
    assert_eq!(absent.tools_declared_count(), None);
    assert_eq!(absent.tool_results_in_input(), Some(false));

    let unproven_tier = classify_anthropic_messages(&json!({"service_tier":"priority"}));
    assert_eq!(unproven_tier.service_tier(), None);

    let empty = classify_anthropic_messages(&json!({
        "tools": [], "messages": [], "tool_choice": {"type": "auto",
        "disable_parallel_tool_use": true}, "thinking": {"type": "disabled"}
    }));
    assert_eq!(empty.tools_declared_count(), Some(0));
    assert_eq!(empty.tool_classes(), Some(0));
    assert_eq!(empty.tool_choice_mode(), Some(ToolChoiceMode::Auto.into()));
    assert_eq!(empty.parallel_tools_requested(), Some(false));
    assert_eq!(empty.reasoning_flag(), Some(false));

    for (choice, mode) in [
        (json!({"type": "any"}), ToolChoiceMode::Required),
        (json!({"type": "none"}), ToolChoiceMode::None),
        (json!({"type": "future"}), ToolChoiceMode::Unknown),
    ] {
        let classified = classify_anthropic_messages(&json!({"tool_choice": choice}));
        assert_eq!(classified.tool_choice_mode(), Some(mode.into()));
    }
    let mixed = classify_anthropic_messages(&json!({
        "tools": [
            {"name": NAME_SENTINEL, "input_schema": {}},
            {"type": "future_provider_tool", "name": MCP_SENTINEL}
        ]
    }));
    assert_eq!(mixed.tools_declared_count(), Some(2));
    assert_eq!(mixed.tool_classes(), None);
    let only_unknown = classify_anthropic_messages(&json!({
        "tools": [{"type": "mcp_toolset", "name": MCP_SENTINEL}]
    }));
    assert_eq!(only_unknown.tools_declared_count(), Some(1));
    assert_eq!(only_unknown.tool_classes(), None);

    let mixed_result = classify_anthropic_messages(&json!({
        "messages":[{"role":"user", "content":[
            {"type":"text", "text":CONTENT_SENTINEL},
            {"type":"future_result_like", "private":RESULT_SENTINEL}
        ]}]
    }));
    assert_eq!(mixed_result.tool_results_in_input(), None);
    let unknown_result = classify_anthropic_messages(&json!({
        "messages":[{"role":"user", "content":[
            {"type":"future_result_like", "private":RESULT_SENTINEL}
        ]}]
    }));
    assert_eq!(unknown_result.tool_results_in_input(), None);

    let malformed = classify_anthropic_messages(&json!({
        "tools": {"not": "an array"}, "messages": "unknown", "tool_choice": "auto"
    }));
    assert_eq!(malformed.tools_declared_count(), None);
    assert_eq!(malformed.tool_results_in_input(), None);
    assert_eq!(
        malformed.tool_choice_mode(),
        Some(ToolChoiceMode::Unknown.into())
    );
}

#[test]
fn openai_responses_classifies_the_selected_additional_tools_once() {
    let classified = classify_openai_responses(&json!({
        "tools": [],
        "input": [
            {"type":"additional_tools", "role":"developer", "tools":[
                {"type":"function", "name":NAME_SENTINEL, "parameters":{}},
                {"type":"tool_search", "execution":"client", "parameters":{}}
            ]},
            {"type":"message", "role":"user", "content":CONTENT_SENTINEL}
        ]
    }));
    assert_eq!(classified.tools_declared_count(), Some(2));
    assert_class(&classified, ToolClass::CustomFunction);
    assert_class(&classified, ToolClass::OtherReviewed);
    assert_eq!(classified.input_modalities(), Some(MODALITY_TEXT));

    let only_tool_search = classify_openai_responses(&json!({
        "input":[{"type":"additional_tools", "role":"developer", "tools":[
            {"type":"tool_search", "execution":"client", "parameters":{}}
        ]}]
    }));
    assert_eq!(only_tool_search.tools_declared_count(), Some(1));
    assert_eq!(
        only_tool_search.tool_classes(),
        Some(TOOL_CLASS_OTHER_REVIEWED)
    );
    assert_eq!(only_tool_search.input_modalities(), Some(0));
}

#[test]
fn openai_responses_maps_all_reviewed_classes_without_unknown_promotion() {
    let request = json!({
        "tools": [
            {"type":"function", "name":NAME_SENTINEL, "parameters":{"x":SCHEMA_SENTINEL}},
            {"type":"custom", "name":NAME_SENTINEL, "format":{"definition":SCHEMA_SENTINEL}},
            {"type":"web_search", "private":DESCRIPTION_SENTINEL},
            {"type":"tool_search", "execution":"client", "parameters":{},
             "private":SCHEMA_SENTINEL},
            {"type":"namespace", "name":MCP_SENTINEL, "tools":[
                {"type":"function", "name":"child_function", "parameters":{}},
                {"type":"custom", "name":"child_custom",
                 "format":{"type":"grammar", "syntax":"lark", "definition":"start: WORD"}}
            ]}
        ],
        "tool_choice": {"type":"function", "name":NAME_SENTINEL},
        "parallel_tool_calls": false,
        "input": [{"type":"function_call_output", "call_id":"c", "output":RESULT_SENTINEL},
                  {"type":"message", "role":"user", "content":[
                      {"type":"input_image", "image_url":"data:image/png;base64,AA"},
                      {"type":"input_text", "text":CONTENT_SENTINEL}]}],
        "text": {"format":{"type":"json_schema", "schema":{"x":SCHEMA_SENTINEL}}},
        "reasoning": {"effort":"high"}, "service_tier":"priority"
    });
    let classified = classify_openai_responses(&request);
    assert_eq!(classified.tools_declared_count(), Some(5));
    assert_class(&classified, ToolClass::CustomFunction);
    assert_class(&classified, ToolClass::CustomTool);
    assert_class(&classified, ToolClass::WebSearch);
    assert_class(&classified, ToolClass::OtherReviewed);
    for class in [
        ToolClass::Computer,
        ToolClass::CodeExecution,
        ToolClass::Mcp,
    ] {
        assert_eq!(has_class(&classified, class), Some(false), "{class:?}");
    }
    assert_eq!(
        classified.tool_choice_mode(),
        Some(ToolChoiceMode::Named.into())
    );
    assert_eq!(classified.parallel_tools_requested(), Some(false));
    assert_eq!(classified.tool_results_in_input(), Some(true));
    assert_eq!(classified.structured_output_flag(), Some(true));
    assert_eq!(classified.reasoning_flag(), Some(true));
    assert_eq!(classified.service_tier(), Some("priority"));
    assert_eq!(has_input_modality(&classified, Modality::Text), Some(true));
    assert_eq!(has_input_modality(&classified, Modality::Image), Some(true));
    assert_eq!(classified.output_modalities(), None);
    assert_redacted(&classified);
}

#[test]
fn openai_chat_covers_legacy_tools_choices_modalities_and_known_absence() {
    let request = json!({
        "functions": [{"name":NAME_SENTINEL, "description":DESCRIPTION_SENTINEL,
                       "parameters":{"x":SCHEMA_SENTINEL}}],
        "function_call": "none", "parallel_tool_calls": true,
        "messages": [
            {"role":"user", "content":[{"type":"text", "text":CONTENT_SENTINEL},
              {"type":"image_url", "image_url":{"url":"data:image/png;base64,AA"}}]},
            {"role":"function", "name":NAME_SENTINEL, "content":RESULT_SENTINEL}
        ],
        "response_format":{"type":"json_object"}, "reasoning_effort":"low",
        "service_tier":"default", "modalities":["text","audio"]
    });
    let classified = classify_openai_chat(&request);
    assert_eq!(classified.tools_declared_count(), Some(1));
    assert_class(&classified, ToolClass::CustomFunction);
    assert_eq!(
        classified.tool_choice_mode(),
        Some(ToolChoiceMode::None.into())
    );
    assert_eq!(classified.parallel_tools_requested(), Some(true));
    assert_eq!(classified.tool_results_in_input(), Some(true));
    assert_eq!(classified.structured_output_flag(), Some(true));
    assert_eq!(classified.reasoning_flag(), Some(true));
    assert_eq!(classified.service_tier(), Some("standard"));
    assert_eq!(has_input_modality(&classified, Modality::Text), Some(true));
    assert_eq!(has_input_modality(&classified, Modality::Image), Some(true));
    assert_eq!(has_output_modality(&classified, Modality::Text), Some(true));
    assert_eq!(
        has_output_modality(&classified, Modality::Audio),
        Some(true)
    );
    assert_redacted(&classified);

    let empty = classify_openai_chat(&json!({"tools": [], "messages": []}));
    assert_eq!(empty.tools_declared_count(), Some(0));
    assert_eq!(empty.tool_classes(), Some(0));
    assert_eq!(empty.tool_results_in_input(), Some(false));
    assert_eq!(empty.input_modalities(), Some(0));
    assert_eq!(empty.structured_output_flag(), None);
    assert_eq!(empty.reasoning_flag(), None);

    let result_only = classify_openai_chat(&json!({
        "messages":[{"role":"tool", "tool_call_id":"c", "content":RESULT_SENTINEL}]
    }));
    assert_eq!(result_only.tool_results_in_input(), Some(true));
    assert_eq!(result_only.input_modalities(), Some(0));

    let mixed_unknown = classify_openai_chat(&json!({
        "messages":[{"role":"user", "content":[
            {"type":"text", "text":CONTENT_SENTINEL},
            {"type":"future_media", "private":MCP_SENTINEL}
        ]}]
    }));
    assert_eq!(mixed_unknown.input_modalities(), None);
    assert_eq!(mixed_unknown.tool_results_in_input(), None);
}

#[test]
fn reviewed_tool_result_is_existential_despite_other_unknown_items() {
    assert_eq!(
        classify_anthropic_messages(&json!({"messages":[{"role":"user", "content":[
            {"type":"future_block"}, {"type":"tool_result", "tool_use_id":"t", "content":RESULT_SENTINEL}
        ]}]}))
        .tool_results_in_input(),
        Some(true)
    );
    assert_eq!(
        classify_anthropic_messages(&json!({"messages":[{"role":"user", "content":[
            {"type":"web_search_tool_result", "tool_use_id":"t", "content":RESULT_SENTINEL}
        ]}]}))
        .tool_results_in_input(),
        Some(true)
    );
    assert_eq!(
        classify_openai_responses(&json!({"input":[
            {"type":"future_item"},
            {"type":"function_call_output", "call_id":"c", "output":RESULT_SENTINEL}
        ]}))
        .tool_results_in_input(),
        Some(true)
    );
    assert_eq!(
        classify_gemini_generate_content(&json!({"contents":[{"role":"user", "parts":[
            {"futurePart":{}}, {"functionResponse":{"name":NAME_SENTINEL, "response":{}}}
        ]}]}))
        .tool_results_in_input(),
        Some(true)
    );
    assert_eq!(
        classify_gemini_generate_content(&json!({"contents":[{"role":"user", "parts":[
            {"codeExecutionResult":{"outcome":"OUTCOME_OK", "output":RESULT_SENTINEL}}
        ]}]}))
        .tool_results_in_input(),
        Some(true)
    );
}

#[test]
fn structured_output_requires_a_reviewed_exact_format() {
    for (request, expected) in [
        (json!({"output_config":{}}), Some(false)),
        (
            json!({"output_config":{"format":{"type":"json_schema"}}}),
            Some(true),
        ),
        (json!({"output_config":{"format":{"type":"future"}}}), None),
        (json!({"output_config":{"format":"bad"}}), None),
    ] {
        assert_eq!(
            classify_anthropic_messages(&request).structured_output_flag(),
            expected
        );
    }
    for (request, expected) in [
        (json!({"response_format":{"type":"text"}}), Some(false)),
        (
            json!({"response_format":{"type":"json_object"}}),
            Some(true),
        ),
        (
            json!({"response_format":{"type":"json_schema"}}),
            Some(true),
        ),
        (json!({"response_format":{"type":"future"}}), None),
        (json!({"response_format":"bad"}), None),
    ] {
        assert_eq!(
            classify_openai_chat(&request).structured_output_flag(),
            expected
        );
    }
    for (request, expected) in [
        (json!({"text":{"format":{"type":"text"}}}), Some(false)),
        (
            json!({"text":{"format":{"type":"json_object"}}}),
            Some(true),
        ),
        (
            json!({"text":{"format":{"type":"json_schema"}}}),
            Some(true),
        ),
        (json!({"text":{"format":{"type":"future"}}}), None),
        (json!({"text":{"format":"bad"}}), None),
    ] {
        assert_eq!(
            classify_openai_responses(&request).structured_output_flag(),
            expected
        );
    }
}

#[test]
fn reasoning_requires_bounded_enabled_or_disabled_evidence() {
    assert_eq!(
        classify_anthropic_messages(&json!({"thinking":{"type":"disabled"}})).reasoning_flag(),
        Some(false)
    );
    assert_eq!(
        classify_anthropic_messages(&json!({"thinking":{"type":"adaptive"}})).reasoning_flag(),
        Some(true)
    );
    assert_eq!(
        classify_anthropic_messages(&json!({"thinking":{"type":"future"}})).reasoning_flag(),
        None
    );
    assert_eq!(
        classify_openai_chat(&json!({"reasoning_effort":"none"})).reasoning_flag(),
        Some(false)
    );
    assert_eq!(
        classify_openai_responses(&json!({"reasoning":{"effort":"high"}})).reasoning_flag(),
        Some(true)
    );
    assert_eq!(
        classify_openai_responses(&json!({"reasoning":{"effort":"future"}})).reasoning_flag(),
        None
    );
    assert_eq!(
        classify_gemini_generate_content(&json!({
            "generationConfig":{"thinkingConfig":{"thinkingBudget":0}}
        }))
        .reasoning_flag(),
        Some(false)
    );
    assert_eq!(
        classify_gemini_generate_content(&json!({
            "generationConfig":{"thinkingConfig":{"thinkingLevel":"low"}}
        }))
        .reasoning_flag(),
        Some(true)
    );
    assert_eq!(
        classify_gemini_generate_content(&json!({
            "generationConfig":{"thinkingConfig":{}}
        }))
        .reasoning_flag(),
        None
    );
}

#[test]
fn input_modalities_follow_validated_parts_without_text_defaults() {
    let anthropic_image = classify_anthropic_messages(&json!({
        "messages":[{"role":"user", "content":[
            {"type":"image", "source":{"type":"base64", "media_type":"image/png", "data":"AA"}}
        ]}]
    }));
    assert_eq!(anthropic_image.input_modalities(), Some(MODALITY_IMAGE));

    let anthropic_result = classify_anthropic_messages(&json!({
        "system":[{"type":"text", "text":CONTENT_SENTINEL}],
        "messages":[{"role":"user", "content":[
            {"type":"tool_result", "tool_use_id":"t", "content":[
                {"type":"image", "source":{"type":"base64", "media_type":"image/png", "data":"AA"}}
            ]}
        ]}]
    }));
    assert_eq!(anthropic_result.input_modalities(), Some(MODALITY_TEXT));

    let anthropic_result_only = classify_anthropic_messages(&json!({
        "messages":[{"role":"user", "content":[
            {"type":"tool_result", "tool_use_id":"t", "content":RESULT_SENTINEL}
        ]}]
    }));
    assert_eq!(anthropic_result_only.input_modalities(), Some(0));

    let chat_image = classify_openai_chat(&json!({
        "messages":[{"role":"user", "content":[
            {"type":"image_url", "image_url":{"url":"data:image/png;base64,AA"}}
        ]}]
    }));
    assert_eq!(chat_image.input_modalities(), Some(MODALITY_IMAGE));

    let responses_image = classify_openai_responses(&json!({
        "input":[{"type":"message", "role":"user", "content":[
            {"type":"input_image", "image_url":"data:image/png;base64,AA"}
        ]}]
    }));
    assert_eq!(responses_image.input_modalities(), Some(MODALITY_IMAGE));

    let gemini_result = classify_gemini_generate_content(&json!({
        "contents":[{"role":"user", "parts":[{
            "functionResponse":{"name":NAME_SENTINEL, "response":{"private":RESULT_SENTINEL}}
        }]}]
    }));
    assert_eq!(gemini_result.input_modalities(), Some(0));
    assert_eq!(gemini_result.tool_results_in_input(), Some(true));

    let gemini_image = classify_gemini_generate_content(&json!({
        "contents":[{"role":"user", "parts":[{
            "inlineData":{"mimeType":"image/png", "data":"AA"}
        }, {
            "fileData":{"mimeType":"application/pdf", "fileUri":"files/private"}
        }]}]
    }));
    assert_eq!(
        gemini_image.input_modalities(),
        Some(MODALITY_IMAGE | MODALITY_PDF)
    );
}

#[test]
fn openai_choice_forms_and_malformed_or_unknown_fields_are_honest() {
    for (choice, mode) in [
        (json!("auto"), ToolChoiceMode::Auto),
        (json!("required"), ToolChoiceMode::Required),
        (json!("none"), ToolChoiceMode::None),
        (
            json!({"type":"function", "name":NAME_SENTINEL}),
            ToolChoiceMode::Named,
        ),
        (json!("future"), ToolChoiceMode::Unknown),
    ] {
        assert_eq!(
            classify_openai_responses(&json!({"tool_choice":choice})).tool_choice_mode(),
            Some(mode.into())
        );
    }
    let mixed = classify_openai_responses(&json!({
        "tools":[
            {"type":"function", "name":NAME_SENTINEL},
            {"type":"future_hosted_tool", "private":MCP_SENTINEL}
        ]
    }));
    assert_eq!(mixed.tools_declared_count(), Some(2));
    assert_eq!(mixed.tool_classes(), None);
    let only_unknown = classify_openai_responses(&json!({
        "tools":[{"type":"future_hosted_tool", "private":MCP_SENTINEL}]
    }));
    assert_eq!(only_unknown.tools_declared_count(), Some(1));
    assert_eq!(only_unknown.tool_classes(), None);

    let mixed_modalities = classify_openai_responses(&json!({
        "input":[
            {"type":"message", "role":"user", "content":[{"type":"input_text", "text":CONTENT_SENTINEL}]},
            {"type":"future_history_with_media", "private":MCP_SENTINEL}
        ]
    }));
    assert_eq!(mixed_modalities.input_modalities(), None);
    assert_eq!(mixed_modalities.tool_results_in_input(), None);
    let unknown_content = classify_openai_responses(&json!({
        "input":[{"type":"message", "role":"user", "content":[
            {"type":"input_text", "text":CONTENT_SENTINEL},
            {"type":"future_media", "private":MCP_SENTINEL}
        ]}]
    }));
    assert_eq!(unknown_content.input_modalities(), None);
    assert_eq!(unknown_content.tool_results_in_input(), None);

    let only_unknown_modality = classify_openai_responses(&json!({
        "input":[{"type":"future_history_with_media", "private":MCP_SENTINEL}]
    }));
    assert_eq!(only_unknown_modality.input_modalities(), None);
    assert_eq!(only_unknown_modality.tool_results_in_input(), None);
    let instructions = classify_openai_responses(&json!({
        "instructions":CONTENT_SENTINEL, "input":[]
    }));
    assert_eq!(instructions.input_modalities(), Some(MODALITY_TEXT));
    let result_only = classify_openai_responses(&json!({
        "input":[{"type":"function_call_output", "call_id":"c", "output":RESULT_SENTINEL}]
    }));
    assert_eq!(result_only.input_modalities(), Some(0));
    let agent_message = classify_openai_responses(&json!({
        "input":[{"type":"agent_message", "content":[{"type":"input_text", "text":CONTENT_SENTINEL}]}]
    }));
    assert_eq!(agent_message.input_modalities(), Some(MODALITY_TEXT));

    let malformed = classify_openai_responses(&json!({
        "tools":"bad", "input":{"bad":true}, "text":{"format":{"type":"future"}},
        "reasoning":"bad", "service_tier":"flex"
    }));
    assert_eq!(malformed.tools_declared_count(), None);
    assert_eq!(malformed.tool_results_in_input(), None);
    assert_eq!(malformed.structured_output_flag(), None);
    assert_eq!(malformed.reasoning_flag(), None);
    assert_eq!(malformed.service_tier(), None);
    assert_eq!(malformed.input_modalities(), None);
}

#[test]
fn gemini_structured_output_requires_schema_or_reviewed_json_mime() {
    for (config, expected) in [
        (json!({"responseMimeType":"application/json"}), Some(true)),
        (json!({"responseMimeType":"text/plain"}), Some(false)),
        (json!({"responseSchema":{"type":"object"}}), Some(true)),
        (json!({"responseJsonSchema":{"type":"object"}}), Some(true)),
        (json!({"responseMimeType":"future/type"}), None),
        (json!({"responseMimeType":null}), None),
        (
            json!({"responseSchema":null, "responseMimeType":"text/plain"}),
            None,
        ),
    ] {
        assert_eq!(
            classify_gemini_generate_content(&json!({"generationConfig":config}))
                .structured_output_flag(),
            expected
        );
    }
}

#[test]
fn gemini_native_maps_reviewed_tools_results_structured_reasoning_and_all_modalities() {
    let request = json!({
        "tools": [
            {"functionDeclarations":[{"name":NAME_SENTINEL,"description":DESCRIPTION_SENTINEL,
                                      "parameters":{"x":SCHEMA_SENTINEL}}]},
            {"googleSearch":{}}, {"computerUse":{}}, {"codeExecution":{}}, {"urlContext":{}}
        ],
        "toolConfig":{"functionCallingConfig":{"mode":"ANY",
                                                  "allowedFunctionNames":[NAME_SENTINEL]}},
        "contents":[{"role":"user","parts":[
            {"text":CONTENT_SENTINEL},
            {"inlineData":{"mimeType":"image/png","data":"AA"}},
            {"inlineData":{"mimeType":"audio/wav","data":"AA"}},
            {"inlineData":{"mimeType":"video/mp4","data":"AA"}},
            {"inlineData":{"mimeType":"application/pdf","data":"AA"}},
            {"functionResponse":{"name":NAME_SENTINEL,"response":{"private":RESULT_SENTINEL}}}
        ]}],
        "generationConfig":{
            "responseMimeType":"application/json", "responseSchema":{"x":SCHEMA_SENTINEL},
            "thinkingConfig":{"thinkingLevel":"high"}, "responseModalities":["TEXT","IMAGE"]
        }
    });
    let classified = classify_gemini_generate_content(&request);
    assert_eq!(classified.tools_declared_count(), Some(5));
    for class in [
        ToolClass::CustomFunction,
        ToolClass::WebSearch,
        ToolClass::Computer,
        ToolClass::CodeExecution,
        ToolClass::OtherReviewed,
    ] {
        assert_class(&classified, class);
    }
    assert_eq!(has_class(&classified, ToolClass::Mcp), Some(false));
    assert_eq!(
        classified.tool_choice_mode(),
        Some(ToolChoiceMode::Named.into())
    );
    assert_eq!(classified.parallel_tools_requested(), None);
    assert_eq!(classified.tool_results_in_input(), Some(true));
    assert_eq!(classified.structured_output_flag(), Some(true));
    assert_eq!(classified.reasoning_flag(), Some(true));
    assert_eq!(classified.service_tier(), None);
    for modality in [
        Modality::Text,
        Modality::Image,
        Modality::Audio,
        Modality::Video,
        Modality::Pdf,
    ] {
        assert_eq!(has_input_modality(&classified, modality), Some(true));
    }
    assert_eq!(has_output_modality(&classified, Modality::Text), Some(true));
    assert_eq!(
        has_output_modality(&classified, Modality::Image),
        Some(true)
    );
    assert_redacted(&classified);
}

#[test]
fn gemini_absent_empty_choice_forms_and_unknown_modalities_are_honest() {
    let absent = classify_gemini_generate_content(&json!({}));
    assert_eq!(absent.tools_declared_count(), None);
    assert_eq!(absent.tool_results_in_input(), None);
    assert_eq!(absent.input_modalities(), None);

    let empty = classify_gemini_generate_content(&json!({"tools":[], "contents":[]}));
    assert_eq!(empty.tools_declared_count(), Some(0));
    assert_eq!(empty.tool_classes(), Some(0));
    assert_eq!(empty.tool_results_in_input(), Some(false));
    assert_eq!(empty.input_modalities(), Some(0));

    for (mode, allowed, expected) in [
        ("AUTO", None, ToolChoiceMode::Auto),
        ("ANY", None, ToolChoiceMode::Required),
        ("NONE", None, ToolChoiceMode::None),
        ("FUTURE", None, ToolChoiceMode::Unknown),
        (
            "ANY",
            Some(json!([NAME_SENTINEL, "other"])),
            ToolChoiceMode::Required,
        ),
    ] {
        let mut config = json!({"mode":mode});
        if let Some(allowed) = allowed {
            config["allowedFunctionNames"] = allowed;
        }
        assert_eq!(
            classify_gemini_generate_content(
                &json!({"toolConfig":{"functionCallingConfig":config}})
            )
            .tool_choice_mode(),
            Some(expected.into())
        );
    }

    let mixed = classify_gemini_generate_content(&json!({
        "tools":[{"functionDeclarations":[]}, {"futureTool":{"private":MCP_SENTINEL}}]
    }));
    assert_eq!(mixed.tools_declared_count(), Some(2));
    assert_eq!(mixed.tool_classes(), None);
    let only_unknown = classify_gemini_generate_content(&json!({
        "tools":[{"futureTool":{"private":MCP_SENTINEL}}]
    }));
    assert_eq!(only_unknown.tools_declared_count(), Some(1));
    assert_eq!(only_unknown.tool_classes(), None);

    let mixed_result = classify_gemini_generate_content(&json!({
        "contents":[{"role":"user", "parts":[
            {"text":CONTENT_SENTINEL, "futureMedia":{"private":RESULT_SENTINEL}}
        ]}]
    }));
    assert_eq!(mixed_result.tool_results_in_input(), None);
    assert_eq!(mixed_result.input_modalities(), None);
    let unknown_result = classify_gemini_generate_content(&json!({
        "contents":[{"role":"user", "parts":[{"futurePart":{"private":RESULT_SENTINEL}}]}]
    }));
    assert_eq!(unknown_result.tool_results_in_input(), None);
    assert_eq!(unknown_result.input_modalities(), None);

    let malformed = classify_gemini_generate_content(&json!({
        "tools":"bad", "contents":"bad",
        "generationConfig":{"responseModalities":["TEXT","AUDIO"]}
    }));
    assert_eq!(malformed.tools_declared_count(), None);
    assert_eq!(malformed.tool_results_in_input(), None);
    assert_eq!(malformed.input_modalities(), None);
    assert_eq!(malformed.output_modalities(), None);
}
