//! Incremental top-level routing-field extraction for universal bodies.
//!
//! Namespaced single-model requests must not build a full `serde_json::Value` only to read
//! `model`. Unknown payload fields are skipped through `IgnoredAny` so original bytes stay the
//! authority for the plane upload.

use serde::de::{self, Deserializer, IgnoredAny, MapAccess, Visitor};
use serde::Deserialize;
use std::fmt;

#[derive(Debug, Eq, PartialEq)]
pub struct RoutingSelectors {
    pub model: String,
    pub has_models: bool,
    pub has_provider: bool,
    pub has_service_tier_alias: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectorError {
    InvalidJson,
    DuplicateField,
    MissingModel,
    InvalidModel,
}

pub fn extract_routing_selectors(bytes: &[u8]) -> Result<RoutingSelectors, SelectorError> {
    match serde_json::from_slice::<RoutingSelectors>(bytes) {
        Ok(selectors) => Ok(selectors),
        Err(error) => {
            let message = error.to_string();
            if message.contains("duplicate routing field") {
                Err(SelectorError::DuplicateField)
            } else if message.contains("missing model") {
                Err(SelectorError::MissingModel)
            } else if message.contains("invalid model") {
                Err(SelectorError::InvalidModel)
            } else {
                Err(SelectorError::InvalidJson)
            }
        }
    }
}

impl<'de> Deserialize<'de> for RoutingSelectors {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(SelectorsVisitor)
    }
}

struct SelectorsVisitor;

impl<'de> Visitor<'de> for SelectorsVisitor {
    type Value = RoutingSelectors;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut model = None;
        let mut seen_model = false;
        let mut has_models = false;
        let mut has_provider = false;
        let mut has_service_tier_alias = false;
        while let Some(key) = map.next_key::<&str>()? {
            match key {
                "model" => {
                    if seen_model {
                        return Err(de::Error::custom("duplicate routing field"));
                    }
                    seen_model = true;
                    match map.next_value::<serde_json::Value>()? {
                        serde_json::Value::String(value) if !value.is_empty() => {
                            model = Some(value);
                        }
                        _ => return Err(de::Error::custom("invalid model")),
                    }
                }
                "models" => {
                    if has_models {
                        return Err(de::Error::custom("duplicate routing field"));
                    }
                    has_models = true;
                    let _: IgnoredAny = map.next_value()?;
                }
                "provider" => {
                    if has_provider {
                        return Err(de::Error::custom("duplicate routing field"));
                    }
                    has_provider = true;
                    let _: IgnoredAny = map.next_value()?;
                }
                "serviceTier" => {
                    if has_service_tier_alias {
                        return Err(de::Error::custom("duplicate routing field"));
                    }
                    has_service_tier_alias = true;
                    let _: IgnoredAny = map.next_value()?;
                }
                _ => {
                    let _: IgnoredAny = map.next_value()?;
                }
            }
        }
        Ok(RoutingSelectors {
            model: model.ok_or_else(|| de::Error::custom("missing model"))?,
            has_models,
            has_provider,
            has_service_tier_alias,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_namespaced_model_without_advanced_fields() {
        let selectors = extract_routing_selectors(
            br#"{"model":"anthropic/claude-opus-4-8","messages":[{"role":"user","content":"hi"}]}"#,
        )
        .unwrap();
        assert_eq!(selectors.model, "anthropic/claude-opus-4-8");
        assert!(!selectors.has_models);
        assert!(!selectors.has_provider);
        assert!(!selectors.has_service_tier_alias);
    }

    #[test]
    fn detects_advanced_and_fast_selectors() {
        let selectors = extract_routing_selectors(
            br#"{"model":"openai/gpt-5.6","models":["openai/gpt-5.6"],"provider":{"order":["openai"]},"serviceTier":"fast"}"#,
        )
        .unwrap();
        assert!(selectors.has_models);
        assert!(selectors.has_provider);
        assert!(selectors.has_service_tier_alias);
    }

    #[test]
    fn duplicate_routing_fields_fail_closed() {
        assert_eq!(
            extract_routing_selectors(br#"{"model":"a","model":"b"}"#),
            Err(SelectorError::DuplicateField)
        );
    }

    #[test]
    fn missing_or_invalid_model_is_distinct_from_malformed_json() {
        assert_eq!(
            extract_routing_selectors(br#"{"messages":[]}"#),
            Err(SelectorError::MissingModel)
        );
        assert_eq!(
            extract_routing_selectors(br#"{"model":""}"#),
            Err(SelectorError::InvalidModel)
        );
        assert_eq!(
            extract_routing_selectors(br#"{"model":1}"#),
            Err(SelectorError::InvalidModel)
        );
        assert_eq!(
            extract_routing_selectors(br#"["not","object"]"#),
            Err(SelectorError::InvalidJson)
        );
        assert_eq!(
            extract_routing_selectors(b"{"),
            Err(SelectorError::InvalidJson)
        );
    }
}
