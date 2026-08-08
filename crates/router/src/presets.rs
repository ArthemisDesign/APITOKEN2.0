//! Reviewed routing presets and deterministic price/latency ranks.
//!
//! The manifest is compiled into the router binary. It contains no runtime telemetry or host
//! configuration, so the same request and aggregate catalog snapshot always produce the same
//! ordering. Missing live preset members are skipped; a preset is visible in the public catalog
//! only while at least one member is present.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::Deserialize;

use crate::catalog::{self, CatalogEntry, CatalogLimits};

pub const PRESET_PREFIX: &str = "preset/";
const SCHEMA_VERSION: u64 = 1;
const HERMES_MIN_CONTEXT_TOKENS: u64 = 64_000;
const REQUIRED_PRESETS: [&str; 4] = [
    "preset/auto",
    "preset/quality",
    "preset/fast",
    "preset/hermes",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoutingManifest {
    schema_version: u64,
    models: Vec<ModelRank>,
    presets: Vec<Preset>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelRank {
    id: String,
    price_rank: u32,
    latency_rank: u32,
    context_tokens: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    id: String,
    name: String,
    models: Vec<String>,
}

impl Preset {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn models(&self) -> &[String] {
        &self.models
    }

    fn active_members<'a>(&self, entries: &'a [(String, CatalogEntry)]) -> Vec<&'a CatalogEntry> {
        self.models
            .iter()
            .filter_map(|member| catalog::find(entries, member).map(|(_, entry)| entry))
            .collect()
    }

    fn to_json(&self, entries: &[(String, CatalogEntry)]) -> serde_json::Value {
        let members = self.active_members(entries);
        let limits = guaranteed_limits(&members);
        let entry = CatalogEntry {
            id: self.id.clone(),
            native_id: self.id.clone(),
            aliases: Vec::new(),
            display_name: Some(self.name.clone()),
            limits,
            reasoning_efforts: guaranteed_list(&members, |entry| &entry.reasoning_efforts),
            service_tiers: guaranteed_list(&members, |entry| &entry.service_tiers),
            input_modalities: guaranteed_list(&members, |entry| &entry.input_modalities),
            output_modalities: guaranteed_list(&members, |entry| &entry.output_modalities),
            tool_calling: guaranteed_bool(&members, |entry| entry.tool_calling),
            structured_outputs: guaranteed_bool(&members, |entry| entry.structured_outputs),
            reasoning: guaranteed_bool(&members, |entry| entry.reasoning),
            streaming: guaranteed_bool(&members, |entry| entry.streaming),
        };
        let mut json = entry.to_json("router");
        json["apitoken"]["routing"] = serde_json::json!({
            "members": members.iter().map(|entry| &entry.id).collect::<Vec<_>>(),
            "variable_model_pricing": true,
        });
        json
    }
}

fn guaranteed_limit(
    members: &[&CatalogEntry],
    field: impl Fn(&CatalogLimits) -> Option<u64>,
) -> Option<u64> {
    members
        .iter()
        .map(|entry| entry.limits.as_ref().and_then(&field))
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .min()
}

fn guaranteed_limits(members: &[&CatalogEntry]) -> Option<CatalogLimits> {
    if members.is_empty() {
        return None;
    }
    let limits = CatalogLimits {
        context: guaranteed_limit(members, |limits| limits.context),
        input: guaranteed_limit(members, |limits| limits.input),
        output: guaranteed_limit(members, |limits| limits.output),
    };
    (limits.context.is_some() || limits.input.is_some() || limits.output.is_some())
        .then_some(limits)
}

fn guaranteed_list(
    members: &[&CatalogEntry],
    field: impl Fn(&CatalogEntry) -> &Option<Vec<String>>,
) -> Option<Vec<String>> {
    let (first, rest) = members.split_first()?;
    let mut intersection = field(first).clone()?;
    for member in rest {
        let values = field(member).as_ref()?;
        intersection.retain(|value| values.contains(value));
    }
    Some(intersection)
}

fn guaranteed_bool(
    members: &[&CatalogEntry],
    field: impl Fn(&CatalogEntry) -> Option<bool>,
) -> Option<bool> {
    if members.is_empty() {
        return None;
    }
    let mut unknown = false;
    for member in members {
        match field(member) {
            Some(false) => return Some(false),
            Some(true) => {}
            None => unknown = true,
        }
    }
    (!unknown).then_some(true)
}

static MANIFEST: OnceLock<RoutingManifest> = OnceLock::new();

fn parse_manifest() -> anyhow::Result<RoutingManifest> {
    let manifest: RoutingManifest = serde_json::from_str(include_str!("../routing-presets.json"))?;
    anyhow::ensure!(
        manifest.schema_version == SCHEMA_VERSION,
        "routing preset manifest schema_version must be {SCHEMA_VERSION}"
    );
    anyhow::ensure!(
        !manifest.models.is_empty(),
        "routing rank table must not be empty"
    );

    let mut ranked = HashMap::with_capacity(manifest.models.len());
    for model in &manifest.models {
        anyhow::ensure!(
            valid_id(&model.id),
            "invalid ranked model id {:?}",
            model.id
        );
        anyhow::ensure!(
            matches!(model.id.split_once('/'), Some(("anthropic" | "openai" | "google" | "kimi", native)) if !native.is_empty()),
            "ranked model must use a supported namespace: {:?}",
            model.id
        );
        anyhow::ensure!(
            model.price_rank > 0 && model.latency_rank > 0 && model.context_tokens > 0,
            "model ranks and context must be positive: {:?}",
            model.id
        );
        anyhow::ensure!(
            ranked.insert(model.id.as_str(), model).is_none(),
            "duplicate ranked model {:?}",
            model.id
        );
    }

    let mut preset_ids = HashSet::with_capacity(manifest.presets.len());
    for preset in &manifest.presets {
        anyhow::ensure!(
            valid_id(&preset.id) && preset.id.starts_with(PRESET_PREFIX),
            "invalid preset id {:?}",
            preset.id
        );
        anyhow::ensure!(
            !preset.name.trim().is_empty(),
            "preset name must not be empty"
        );
        anyhow::ensure!(
            !preset.models.is_empty(),
            "preset {:?} must not be empty",
            preset.id
        );
        anyhow::ensure!(
            preset_ids.insert(preset.id.as_str()),
            "duplicate preset {:?}",
            preset.id
        );
        let mut members = HashSet::with_capacity(preset.models.len());
        for member in &preset.models {
            let rank = ranked.get(member.as_str()).ok_or_else(|| {
                anyhow::anyhow!("preset {:?} member {:?} has no rank", preset.id, member)
            })?;
            anyhow::ensure!(
                members.insert(member.as_str()),
                "duplicate member {:?} in preset {:?}",
                member,
                preset.id
            );
            if preset.id == "preset/hermes" {
                anyhow::ensure!(
                    rank.context_tokens >= HERMES_MIN_CONTEXT_TOKENS,
                    "Hermes preset member {:?} has less than 64K context",
                    member
                );
            }
        }
    }

    let actual: HashSet<_> = manifest
        .presets
        .iter()
        .map(|preset| preset.id.as_str())
        .collect();
    anyhow::ensure!(
        REQUIRED_PRESETS.iter().copied().collect::<HashSet<_>>() == actual,
        "routing manifest must define exactly the four reserved presets"
    );
    Ok(manifest)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id.trim() == id
        && !id.bytes().any(|byte| byte.is_ascii_control())
}

fn manifest() -> &'static RoutingManifest {
    MANIFEST.get_or_init(|| {
        parse_manifest().unwrap_or_else(|error| panic!("invalid routing-presets.json: {error:#}"))
    })
}

/// Parse and validate the compiled manifest during process startup, before accepting traffic.
pub fn validate_at_startup() -> anyhow::Result<()> {
    if MANIFEST.get().is_none() {
        let parsed = parse_manifest()?;
        let _ = MANIFEST.set(parsed);
    }
    Ok(())
}

pub fn is_preset_syntax(id: &str) -> bool {
    id.starts_with(PRESET_PREFIX)
}

pub fn find(id: &str) -> Option<&'static Preset> {
    manifest().presets.iter().find(|preset| preset.id == id)
}

pub fn ranks(id: &str) -> Option<(u32, u32)> {
    manifest()
        .models
        .iter()
        .find(|model| model.id == id)
        .map(|model| (model.price_rank, model.latency_rank))
}

fn is_active(preset: &Preset, entries: &[(String, CatalogEntry)]) -> bool {
    preset
        .models
        .iter()
        .any(|member| catalog::find(entries, member).is_some())
}

pub fn active_catalog_entries(entries: &[(String, CatalogEntry)]) -> Vec<serde_json::Value> {
    manifest()
        .presets
        .iter()
        .filter(|preset| is_active(preset, entries))
        .map(|preset| preset.to_json(entries))
        .collect()
}

pub fn active_catalog_entry(
    id: &str,
    entries: &[(String, CatalogEntry)],
) -> Option<serde_json::Value> {
    find(id)
        .filter(|preset| is_active(preset, entries))
        .map(|preset| preset.to_json(entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_manifest_is_strict_complete_and_hermes_safe() {
        let parsed = parse_manifest().unwrap();
        assert_eq!(parsed.schema_version, 1);
        assert_eq!(parsed.presets.len(), 4);
        assert!(parsed.models.len() >= 23);
        let flash_preview = parsed
            .models
            .iter()
            .find(|model| model.id == "google/gemini-3-flash-preview")
            .expect("published Flash Preview must have reviewed router ranks");
        assert_eq!(flash_preview.price_rank, 30);
        assert_eq!(flash_preview.latency_rank, 15);
        assert_eq!(flash_preview.context_tokens, 1_000_000);
        assert!(parsed
            .presets
            .iter()
            .all(|preset| !preset.models.contains(&flash_preview.id)));
    }

    #[test]
    fn presets_are_active_only_with_a_live_member() {
        let entries = vec![(
            "openai".to_string(),
            CatalogEntry {
                id: "openai/gpt-5.6-terra".to_string(),
                native_id: "gpt-5.6-terra".to_string(),
                aliases: vec!["gpt-5.6-terra".to_string()],
                display_name: None,
                limits: None,
                reasoning_efforts: None,
                service_tiers: Some(vec!["standard".to_string(), "priority".to_string()]),
                input_modalities: Some(vec!["text".to_string(), "image".to_string()]),
                output_modalities: Some(vec!["text".to_string()]),
                tool_calling: Some(true),
                structured_outputs: Some(true),
                reasoning: Some(true),
                streaming: Some(true),
            },
        )];
        let ids: Vec<_> = active_catalog_entries(&entries)
            .into_iter()
            .map(|entry| entry["id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(ids, ["preset/auto", "preset/hermes"]);
        assert!(active_catalog_entry("preset/quality", &entries).is_none());
        let auto = active_catalog_entry("preset/auto", &entries).unwrap();
        assert_eq!(
            auto["apitoken"]["routing"]["members"],
            serde_json::json!(["openai/gpt-5.6-terra"])
        );
        assert_eq!(auto["apitoken"]["routing"]["variable_model_pricing"], true);
        assert_eq!(
            auto["service_tiers"],
            serde_json::json!(["standard", "priority"])
        );
    }

    #[test]
    fn preset_catalog_publishes_only_live_conservative_guarantees() {
        let make = |id: &str, limits: CatalogLimits, efforts: &[&str], tiers: &[&str]| {
            (
                id.split_once('/').unwrap().0.to_string(),
                CatalogEntry {
                    id: id.to_string(),
                    native_id: id.split_once('/').unwrap().1.to_string(),
                    aliases: Vec::new(),
                    display_name: None,
                    limits: Some(limits),
                    reasoning_efforts: Some(
                        efforts.iter().map(|value| (*value).to_string()).collect(),
                    ),
                    service_tiers: Some(tiers.iter().map(|value| (*value).to_string()).collect()),
                    input_modalities: Some(vec!["text".to_string(), "image".to_string()]),
                    output_modalities: Some(vec!["text".to_string()]),
                    tool_calling: Some(true),
                    structured_outputs: Some(true),
                    reasoning: Some(true),
                    streaming: Some(true),
                },
            )
        };
        let mut entries = vec![
            make(
                "anthropic/claude-sonnet-5",
                CatalogLimits {
                    context: Some(1_000_000),
                    input: Some(1_000_000),
                    output: Some(128_000),
                },
                &["low", "medium", "high", "xhigh", "max"],
                &["standard"],
            ),
            make(
                "openai/gpt-5.6-terra",
                CatalogLimits {
                    context: Some(400_000),
                    input: Some(272_000),
                    output: Some(128_000),
                },
                &["none", "low", "medium", "high", "xhigh", "max"],
                &["standard", "priority"],
            ),
            make(
                "google/gemini-3.6-flash",
                CatalogLimits {
                    context: Some(1_048_576),
                    input: Some(1_048_576),
                    output: Some(65_536),
                },
                &["minimal", "low", "medium", "high"],
                &["standard"],
            ),
        ];

        let auto = active_catalog_entry("preset/auto", &entries).unwrap();
        assert_eq!(
            auto["apitoken"]["routing"]["members"],
            serde_json::json!([
                "anthropic/claude-sonnet-5",
                "openai/gpt-5.6-terra",
                "google/gemini-3.6-flash"
            ])
        );
        assert_eq!(
            auto["apitoken"]["limits"],
            serde_json::json!({"context": 400_000, "input": 272_000, "output": 65_536})
        );
        assert_eq!(
            auto["reasoning_efforts"],
            serde_json::json!(["low", "medium", "high"])
        );
        assert_eq!(auto["service_tiers"], serde_json::json!(["standard"]));
        assert_eq!(auto["apitoken"]["capabilities"]["tool_calling"], true);

        entries[2].1.structured_outputs = Some(false);
        let auto = active_catalog_entry("preset/auto", &entries).unwrap();
        assert_eq!(
            auto["apitoken"]["capabilities"]["structured_outputs"],
            false
        );
        entries[2].1.structured_outputs = None;
        let auto = active_catalog_entry("preset/auto", &entries).unwrap();
        assert!(auto["apitoken"]["capabilities"]
            .get("structured_outputs")
            .is_none());
    }
}
