//! Reviewed routing presets and deterministic price/latency ranks.
//!
//! The manifest is compiled into the router binary. It contains no runtime telemetry or host
//! configuration, so the same request and aggregate catalog snapshot always produce the same
//! ordering. Missing live preset members are skipped; a preset is visible in the public catalog
//! only while at least one member is present.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::Deserialize;

use crate::catalog::{self, CatalogEntry};

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

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "object": "model",
            "created": 0,
            "owned_by": "router",
            "aliases": [],
            "name": self.name,
        })
    }
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
            matches!(model.id.split_once('/'), Some(("anthropic" | "openai" | "google", native)) if !native.is_empty()),
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
        .map(Preset::to_json)
        .collect()
}

pub fn active_catalog_entry(
    id: &str,
    entries: &[(String, CatalogEntry)],
) -> Option<serde_json::Value> {
    find(id)
        .filter(|preset| is_active(preset, entries))
        .map(Preset::to_json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_manifest_is_strict_complete_and_hermes_safe() {
        let parsed = parse_manifest().unwrap();
        assert_eq!(parsed.schema_version, 1);
        assert_eq!(parsed.presets.len(), 4);
        assert!(parsed.models.len() >= 22);
    }

    #[test]
    fn presets_are_active_only_with_a_live_member() {
        let entries = vec![(
            "openai".to_string(),
            CatalogEntry {
                id: "openai/gpt-5.6-terra".to_string(),
                alias: "gpt-5.6-terra".to_string(),
                display_name: None,
                reasoning_efforts: None,
            },
        )];
        let ids: Vec<_> = active_catalog_entries(&entries)
            .into_iter()
            .map(|entry| entry["id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(ids, ["preset/auto", "preset/hermes"]);
        assert!(active_catalog_entry("preset/quality", &entries).is_none());
    }
}
