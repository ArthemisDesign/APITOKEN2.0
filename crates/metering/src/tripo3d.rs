//! Official Tripo3D API platform per-task credit rate card and nanoUSD conversion.
//!
//! Authority: `docs/engine/TRIPO3D_PROVIDER.md` §5.1 (official `billing.md` rate card) and §3
//! (per-task `model_version` admission sets), reviewed 2026-08-12. Tripo3D publishes a fixed
//! dollar link for its native credit unit — **$0.01 per credit** — so the API-dollar leg is
//! exact, not estimated. This module prices the **reserve** (conservative hold = worst case of
//! base + selected surcharges); the per-turn settlement authority is the provider-reported
//! `consumed_credit`, and a settled amount above this tariff's maximum for the admitted task
//! shape is a typed anomaly, never silent acceptance.
//!
//! Every lookup fails closed: an unknown task kind, an unlisted `model_version`, or an option
//! combination the rate card does not admit returns `None` — never a guessed price. Free tasks
//! (`animate_prerigcheck`, `import_model`) return `Some(0)`: an explicit, documented official
//! zero, distinct from a missing price. All arithmetic is checked integer math.

use crate::TariffScheduleId;

/// Reviewed identity of the official per-task credit rate card below
/// (`docs/engine/TRIPO3D_PROVIDER.md` §5.1, reviewed 2026-08-12). Change this identity whenever
/// any rate or option semantics change; history is never rewritten, a new epoch is added.
pub const TRIPO3D_TARIFF_SCHEDULE_ID: &str = "tripo3d/openapi-billing/2026-08-12";

/// Hot-override tariff family of the Tripo3D rate card: `TRIPO3D_TARIFF_SCHEDULE_ID` minus its
/// date suffix. One family covers every task/model price below because they share one reviewed
/// official rate card.
pub const TRIPO3D_TARIFF_FAMILY: &str = "tripo3d/openapi";

/// Official fixed dollar link of the native credit unit: **1 credit = $0.01 = 10 000 000
/// nanoUSD** (`platform.tripo3d.ai/docs/other/billing.md`, reviewed 2026-08-12;
/// `docs/engine/TRIPO3D_PROVIDER.md` §1.1/§5.1). Prepaid API credits never expire; no
/// volume/bundle discount is published, so the flat rate is exact.
pub const TRIPO3D_NANOUSD_PER_CREDIT: i128 = 10_000_000;

/// The hot-override tariff family a Tripo3D task price resolves against.
pub fn tripo3d_tariff_family() -> &'static str {
    TRIPO3D_TARIFF_FAMILY
}

/// The pinned schedule identity of this rate card, for durable admission snapshots.
pub fn tripo3d_tariff_schedule_id() -> TariffScheduleId {
    TariffScheduleId::from_static(TRIPO3D_TARIFF_SCHEDULE_ID)
}

/// Tripo3D task kinds with an official price (`docs/engine/TRIPO3D_PROVIDER.md` §3 catalog and
/// §5.1 rate card, reviewed 2026-08-12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tripo3dTaskKind {
    TextToModel,
    ImageToModel,
    MultiviewToModel,
    TextureModel,
    MeshSegmentation,
    MeshCompletion,
    HighpolyToLowpoly,
    AnimatePrerigcheck,
    AnimateRig,
    AnimateRetarget,
    ConvertModel,
    ImportModel,
    TextToImage,
    GenerateImage,
    GenerateMultiviewImage,
    EditMultiviewImage,
    RefineModel,
}

/// `texture_quality` option values and their surcharges (§5.1: standard +10 / detailed +20 /
/// extreme +30 over the no-texture base). For `texture_model` this is the primary price
/// selector (10 / 20 / 30 flat).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tripo3dTextureQuality {
    #[default]
    Standard,
    Detailed,
    Extreme,
}

/// `geometry_quality` option values (§5.1: `detailed` costs +20 over the default `standard`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tripo3dGeometryQuality {
    #[default]
    Standard,
    Detailed,
}

/// Request-side options of a generation task (§5.1 surcharge list).
///
/// Surcharges stack on the no-texture base and are admitted **only for the standard tier**
/// (Turbo / v3.1 / v3.0 / v2.5 / v2.0): `smart_low_poly` +10, `generate_parts` +20, `quad` +5,
/// `style` +5, `texture_quality` standard +10 / detailed +20 / extreme +30,
/// `geometry_quality=detailed` +20. The standard-texture base column of the rate card is
/// exactly no-texture base + the standard texture surcharge, and **includes PBR** — `pbr`
/// therefore never changes the price. P1 is all-in: it admits only the `texture` base selector
/// (standard quality) and `pbr`; any other non-default option fails closed. The legacy v1.4
/// tier admits no options at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tripo3dOptions {
    pub texture: bool,
    pub pbr: bool,
    pub smart_low_poly: bool,
    pub generate_parts: bool,
    pub quad: bool,
    pub style: bool,
    pub texture_quality: Tripo3dTextureQuality,
    pub geometry_quality: Tripo3dGeometryQuality,
}

/// `convert_model` modes (§5.1: basic 5 / advanced 10 credits). Version-independent task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tripo3dConvertMode {
    Basic,
    Advanced,
}

/// Why a credit → nanoUSD conversion cannot be performed. Every variant fails closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tripo3dMeteringError {
    /// Credits are a non-negative counter; a negative value means broken upstream accounting.
    NegativeCredits,
    /// Checked integer arithmetic overflowed.
    Overflow,
}

// ── model_version admission sets (manifest §3, reviewed 2026-08-12) ──────────

/// `text_to_model` / `image_to_model` accept the full generation ladder.
const VERSIONS_TO_MODEL: &[&str] = &[
    "v3.1-20260211",
    "v3.0-20250812",
    "P1-20260311",
    "v2.5-20250123",
    "v2.0-20240919",
    "Turbo-v1.0-20250506",
    "v1.4-20240625",
];

/// `multiview_to_model`: the same ladder minus Turbo and legacy v1.4.
const VERSIONS_MULTIVIEW: &[&str] = &[
    "v3.1-20260211",
    "v3.0-20250812",
    "P1-20260311",
    "v2.5-20250123",
    "v2.0-20240919",
];

const VERSIONS_TEXTURE_MODEL: &[&str] = &["v3.0-20250812", "v2.5-20250123"];

/// Mesh segmentation: the v2 API version only. The apiv3 `v2.0-20260430` surface is
/// undocumented and unused (manifest §2/§3) — it fails closed.
const VERSIONS_MESH_SEGMENTATION: &[&str] = &["v1.0-20250506"];

/// Mesh completion. The overview's additional `P-v2.0-20251225` mention is NOT admitted: the
/// same string is entangled in the documented `highpoly_to_lowpoly` docs-vs-SDK conflict
/// (manifest §3/§6.6), so it fails closed until a live probe pins it.
const VERSIONS_MESH_COMPLETION: &[&str] = &["v1.0-20250506"];

const VERSIONS_ANIMATE_PRERIGCHECK: &[&str] = &["v2.0-20250506", "v1.0-20240301"];
const VERSIONS_ANIMATE_RIG: &[&str] = &["v2.5-20260210", "v2.0-20250506", "v1.0-20240301"];

/// `text_to_image` / `generate_image` model ids (manifest §3).
const VERSIONS_IMAGE: &[&str] = &[
    "flux.1_kontext_pro",
    "flux.1_dev",
    "gpt_4o",
    "gpt_image_1.5",
    "gpt_image_2",
    "midjourney",
    "gemini_2.5_flash_image_preview",
    "gemini_3_pro_image_preview",
    "gemini_3.1_flash_image_preview",
];

/// Documented default when a generation task omits `model_version` (manifest §3).
const DEFAULT_GENERATION_VERSION: &str = "v2.5-20250123";
/// Documented default image model when `text_to_image`/`generate_image` omit it (manifest §3).
const DEFAULT_IMAGE_VERSION: &str = "flux.1_kontext_pro";

/// Price tier of a generation `model_version`: the §5.1 base table has three columns — P1,
/// standard (Turbo/v3.1/v3.0/v2.5/v2.0, all at one price; "v3.0 and v2.5 cost the same"), and
/// legacy v1.4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GenerationTier {
    P1,
    Standard,
    LegacyV14,
}

fn generation_tier(version: &str) -> Option<GenerationTier> {
    match version {
        "P1-20260311" => Some(GenerationTier::P1),
        "v1.4-20240625" => Some(GenerationTier::LegacyV14),
        "v3.1-20260211" | "v3.0-20250812" | "v2.5-20250123" | "v2.0-20240919"
        | "Turbo-v1.0-20250506" => Some(GenerationTier::Standard),
        _ => None,
    }
}

fn texture_quality_surcharge(quality: Tripo3dTextureQuality) -> i64 {
    match quality {
        Tripo3dTextureQuality::Standard => 10,
        Tripo3dTextureQuality::Detailed => 20,
        Tripo3dTextureQuality::Extreme => 30,
    }
}

/// §5.1 base table: (P1 no-texture, P1 std-texture, standard no-texture, standard std-texture,
/// legacy v1.4 flat). The standard std-texture column is exactly no-texture + the standard
/// texture surcharge (+10); a regression test pins that identity.
fn generation_base(kind: Tripo3dTaskKind) -> (i64, i64, i64, i64, Option<i64>) {
    match kind {
        Tripo3dTaskKind::TextToModel => (30, 40, 10, 20, Some(20)),
        Tripo3dTaskKind::ImageToModel => (40, 50, 20, 30, Some(30)),
        // The rate card has no legacy v1.4 column for multiview_to_model ("—"); the version
        // set already excludes it, so the None here is unreachable defense in depth.
        Tripo3dTaskKind::MultiviewToModel => (40, 50, 20, 30, None),
        _ => unreachable!("generation_base is only called for *_to_model tasks"),
    }
}

fn generation_credits(
    kind: Tripo3dTaskKind,
    tier: GenerationTier,
    options: &Tripo3dOptions,
) -> Option<i64> {
    let (p1_plain, p1_textured, std_plain, _std_textured, legacy) = generation_base(kind);
    match tier {
        // P1 is all-in: the `texture` base selector (standard quality only) and the free `pbr`
        // flag are admitted; every other option is a surcharge the P1 tier does not have, so it
        // fails closed rather than being silently ignored or charged.
        GenerationTier::P1 => {
            if options.smart_low_poly
                || options.generate_parts
                || options.quad
                || options.style
                || options.texture_quality != Tripo3dTextureQuality::Standard
                || options.geometry_quality != Tripo3dGeometryQuality::Standard
            {
                return None;
            }
            Some(if options.texture { p1_textured } else { p1_plain })
        }
        GenerationTier::Standard => {
            let mut total = if options.texture {
                std_plain + texture_quality_surcharge(options.texture_quality)
            } else {
                // A texture quality tier without the texture flag itself is a combination the
                // rate card does not define.
                if options.texture_quality != Tripo3dTextureQuality::Standard {
                    return None;
                }
                std_plain
            };
            if options.smart_low_poly {
                total += 10;
            }
            if options.generate_parts {
                total += 20;
            }
            if options.quad {
                total += 5;
            }
            if options.style {
                total += 5;
            }
            if options.geometry_quality == Tripo3dGeometryQuality::Detailed {
                total += 20;
            }
            Some(total)
        }
        // Legacy v1.4 has a single flat price per task — no texture split, no surcharges.
        GenerationTier::LegacyV14 => {
            if *options != Tripo3dOptions::default() {
                return None;
            }
            legacy
        }
    }
}

/// Version-independent tasks accept no `model_version`; a supplied one fails closed.
fn version_independent(model_version: Option<&str>) -> bool {
    model_version.is_none()
}

/// Flat-priced tasks whose price does not depend on the version: a supplied version must be in
/// the admitted set, an omitted one is accepted (there is no price ambiguity to resolve).
fn flat_version_ok(model_version: Option<&str>, admitted: &[&str]) -> bool {
    match model_version {
        Some(version) => admitted.contains(&version),
        None => true,
    }
}

/// Exact official credit cost of one task, or `None` when the task/model/option combination is
/// not on the reviewed rate card (`docs/engine/TRIPO3D_PROVIDER.md` §5.1, reviewed 2026-08-12).
///
/// Per-unit tasks are priced by their dedicated helpers instead and return `None` here:
/// `animate_retarget` → [`tripo3d_animate_retarget_credits`], `edit_multiview_image` →
/// [`tripo3d_edit_multiview_image_credits`], `convert_model` →
/// [`tripo3d_convert_model_credits`].
///
/// `highpoly_to_lowpoly` fails closed entirely: its `model_version` is a documented docs-vs-SDK
/// conflict (`P-v2.0-20251225` vs `P-v2.0-20251226`, manifest §3/§6.6). Neither spelling is
/// accepted until a live probe pins the wire value; the docs spelling is the designated
/// canonical candidate once proven.
///
/// `generate_image` is priced at the conservative upper bound 10 credits: the official card
/// says "5 or 10" without publishing the per-model split (§5.1), so the reserve holds the worst
/// case; a live probe will pin the per-model prices as a new epoch.
pub fn tripo3d_task_credits(
    kind: Tripo3dTaskKind,
    model_version: Option<&str>,
    options: &Tripo3dOptions,
) -> Option<i64> {
    match kind {
        Tripo3dTaskKind::TextToModel
        | Tripo3dTaskKind::ImageToModel
        | Tripo3dTaskKind::MultiviewToModel => {
            let admitted = match kind {
                Tripo3dTaskKind::MultiviewToModel => VERSIONS_MULTIVIEW,
                _ => VERSIONS_TO_MODEL,
            };
            let version = model_version.unwrap_or(DEFAULT_GENERATION_VERSION);
            if !admitted.contains(&version) {
                return None;
            }
            generation_credits(kind, generation_tier(version)?, options)
        }
        Tripo3dTaskKind::RefineModel => {
            // Legacy-only task: v1.4 flat 30, no options (§5.1 base table).
            let version = model_version.unwrap_or("v1.4-20240625");
            if version != "v1.4-20240625" || *options != Tripo3dOptions::default() {
                return None;
            }
            Some(30)
        }
        Tripo3dTaskKind::TextureModel => {
            // Texture quality is the price selector: standard 10 / detailed 20 / extreme 30,
            // +5 with a style reference (§5.1 "Other tasks"). No other option applies.
            let version = model_version.unwrap_or(DEFAULT_GENERATION_VERSION);
            if !VERSIONS_TEXTURE_MODEL.contains(&version) {
                return None;
            }
            if options.texture
                || options.pbr
                || options.smart_low_poly
                || options.generate_parts
                || options.quad
                || options.geometry_quality != Tripo3dGeometryQuality::Standard
            {
                return None;
            }
            let mut total = match options.texture_quality {
                Tripo3dTextureQuality::Standard => 10,
                Tripo3dTextureQuality::Detailed => 20,
                Tripo3dTextureQuality::Extreme => 30,
            };
            if options.style {
                total += 5;
            }
            Some(total)
        }
        Tripo3dTaskKind::MeshSegmentation => {
            if !flat_version_ok(model_version, VERSIONS_MESH_SEGMENTATION)
                || *options != Tripo3dOptions::default()
            {
                return None;
            }
            Some(40)
        }
        Tripo3dTaskKind::MeshCompletion => {
            if !flat_version_ok(model_version, VERSIONS_MESH_COMPLETION)
                || *options != Tripo3dOptions::default()
            {
                return None;
            }
            Some(50)
        }
        // Documented docs-vs-SDK version conflict (manifest §3/§6.6): neither
        // `P-v2.0-20251225` (docs, the designated canonical candidate) nor `P-v2.0-20251226`
        // (SDK) is accepted until live-probed. Fail closed.
        Tripo3dTaskKind::HighpolyToLowpoly => None,
        // Free by the official card (§5.1) — an explicit documented zero, not a missing price.
        Tripo3dTaskKind::AnimatePrerigcheck => {
            if !flat_version_ok(model_version, VERSIONS_ANIMATE_PRERIGCHECK)
                || *options != Tripo3dOptions::default()
            {
                return None;
            }
            Some(0)
        }
        Tripo3dTaskKind::AnimateRig => {
            if !flat_version_ok(model_version, VERSIONS_ANIMATE_RIG)
                || *options != Tripo3dOptions::default()
            {
                return None;
            }
            Some(25)
        }
        Tripo3dTaskKind::AnimateRetarget
        | Tripo3dTaskKind::ConvertModel
        | Tripo3dTaskKind::EditMultiviewImage => None,
        // Free by the official card (§5.1) — an explicit documented zero, not a missing price.
        Tripo3dTaskKind::ImportModel => {
            if !version_independent(model_version) || *options != Tripo3dOptions::default() {
                return None;
            }
            Some(0)
        }
        Tripo3dTaskKind::TextToImage => {
            let version = model_version.unwrap_or(DEFAULT_IMAGE_VERSION);
            if !VERSIONS_IMAGE.contains(&version) || *options != Tripo3dOptions::default() {
                return None;
            }
            Some(5)
        }
        Tripo3dTaskKind::GenerateImage => {
            let version = model_version.unwrap_or(DEFAULT_IMAGE_VERSION);
            if !VERSIONS_IMAGE.contains(&version) || *options != Tripo3dOptions::default() {
                return None;
            }
            // "5 or 10" without a published per-model split: conservative upper bound.
            Some(10)
        }
        Tripo3dTaskKind::GenerateMultiviewImage => {
            if !version_independent(model_version) || *options != Tripo3dOptions::default() {
                return None;
            }
            Some(10)
        }
    }
}

/// `animate_retarget`: **10 credits per animation** (§5.1). Version-independent. A zero count
/// is not a priceable request and fails closed.
pub fn tripo3d_animate_retarget_credits(animations: u64) -> Option<i64> {
    if animations == 0 {
        return None;
    }
    10_i64.checked_mul(i64::try_from(animations).ok()?)
}

/// `edit_multiview_image`: **5 credits per edited image** (§5.1). A zero count fails closed.
pub fn tripo3d_edit_multiview_image_credits(edited_images: u64) -> Option<i64> {
    if edited_images == 0 {
        return None;
    }
    5_i64.checked_mul(i64::try_from(edited_images).ok()?)
}

/// `convert_model`: basic 5 / advanced 10 credits (§5.1). Version-independent.
pub fn tripo3d_convert_model_credits(mode: Tripo3dConvertMode) -> i64 {
    match mode {
        Tripo3dConvertMode::Basic => 5,
        Tripo3dConvertMode::Advanced => 10,
    }
}

/// Convert exact credits to nanoUSD at the official fixed rate ($0.01/credit). Checked integer
/// math; negative credits and overflow are typed errors, never silently clamped. The input is
/// i128 so callers aggregating many settled tasks can pass a running total directly — overflow
/// is then reachable and reported, not wrapped.
pub fn tripo3d_cost_nanodollars(credits: i128) -> Result<i128, Tripo3dMeteringError> {
    if credits < 0 {
        return Err(Tripo3dMeteringError::NegativeCredits);
    }
    credits
        .checked_mul(TRIPO3D_NANOUSD_PER_CREDIT)
        .ok_or(Tripo3dMeteringError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPTS: Tripo3dOptions = Tripo3dOptions {
        texture: false,
        pbr: false,
        smart_low_poly: false,
        generate_parts: false,
        quad: false,
        style: false,
        texture_quality: Tripo3dTextureQuality::Standard,
        geometry_quality: Tripo3dGeometryQuality::Standard,
    };

    fn price(kind: Tripo3dTaskKind, version: Option<&str>, options: Tripo3dOptions) -> Option<i64> {
        tripo3d_task_credits(kind, version, &options)
    }

    #[test]
    fn base_table_every_cell_is_exact() {
        // §5.1 base table. Standard tier is one price column for Turbo/v3.1/v3.0/v2.5/v2.0
        // ("v3.0 and v2.5 cost the same"); the test walks every admitted standard spelling.
        let standard_versions = [
            "v3.1-20260211",
            "v3.0-20250812",
            "v2.5-20250123",
            "v2.0-20240919",
            "Turbo-v1.0-20250506",
        ];
        let textured = Tripo3dOptions {
            texture: true,
            ..OPTS
        };
        // (kind, P1 plain, P1 textured, standard plain, standard textured, v1.4, admits Turbo)
        let cases = [
            (Tripo3dTaskKind::TextToModel, 30, 40, 10, 20, Some(20), true),
            (Tripo3dTaskKind::ImageToModel, 40, 50, 20, 30, Some(30), true),
            (Tripo3dTaskKind::MultiviewToModel, 40, 50, 20, 30, None, false),
        ];
        for (kind, p1_plain, p1_tex, std_plain, std_tex, v14, admits_turbo) in cases {
            assert_eq!(price(kind, Some("P1-20260311"), OPTS), Some(p1_plain));
            assert_eq!(price(kind, Some("P1-20260311"), textured), Some(p1_tex));
            for version in standard_versions {
                let expected = if version.starts_with("Turbo") && !admits_turbo {
                    None // multiview_to_model admits neither Turbo nor v1.4 (§3)
                } else {
                    Some(std_plain)
                };
                assert_eq!(price(kind, Some(version), OPTS), expected, "{version}");
                assert_eq!(
                    price(kind, Some(version), textured),
                    expected.map(|_| std_tex),
                    "{version}"
                );
            }
            match v14 {
                Some(flat) => assert_eq!(price(kind, Some("v1.4-20240625"), OPTS), Some(flat)),
                None => assert_eq!(price(kind, Some("v1.4-20240625"), OPTS), None),
            }
        }
        // Legacy refine_model: v1.4 only, flat 30.
        assert_eq!(
            price(Tripo3dTaskKind::RefineModel, Some("v1.4-20240625"), OPTS),
            Some(30)
        );
        assert_eq!(
            price(Tripo3dTaskKind::RefineModel, Some("v2.5-20250123"), OPTS),
            None
        );
        // The std-texture column is exactly no-texture base + the standard texture surcharge.
        for kind in [
            Tripo3dTaskKind::TextToModel,
            Tripo3dTaskKind::ImageToModel,
            Tripo3dTaskKind::MultiviewToModel,
        ] {
            let plain = price(kind, Some("v3.0-20250812"), OPTS).unwrap();
            let std_tex = price(kind, Some("v3.0-20250812"), textured).unwrap();
            assert_eq!(std_tex - plain, 10);
        }
    }

    #[test]
    fn default_model_version_is_v2_5() {
        assert_eq!(
            price(Tripo3dTaskKind::TextToModel, None, OPTS),
            price(Tripo3dTaskKind::TextToModel, Some("v2.5-20250123"), OPTS)
        );
        assert_eq!(
            price(Tripo3dTaskKind::TextToImage, None, OPTS),
            price(Tripo3dTaskKind::TextToImage, Some("flux.1_kontext_pro"), OPTS)
        );
    }

    #[test]
    fn standard_tier_surcharges_stack_exactly() {
        let options = Tripo3dOptions {
            texture: true,
            texture_quality: Tripo3dTextureQuality::Detailed,
            smart_low_poly: true,
            generate_parts: true,
            quad: true,
            style: true,
            geometry_quality: Tripo3dGeometryQuality::Detailed,
            ..OPTS
        };
        // text_to_model: 10 base + 20 detailed texture + 10 + 20 + 5 + 5 + 20 = 90.
        assert_eq!(
            price(Tripo3dTaskKind::TextToModel, Some("v3.1-20260211"), options),
            Some(90)
        );

        // Each surcharge individually, on image_to_model (base 20).
        let one = |patch: Tripo3dOptions| price(Tripo3dTaskKind::ImageToModel, Some("v2.0-20240919"), patch);
        assert_eq!(
            one(Tripo3dOptions { smart_low_poly: true, ..OPTS }),
            Some(30)
        );
        assert_eq!(
            one(Tripo3dOptions { generate_parts: true, ..OPTS }),
            Some(40)
        );
        assert_eq!(one(Tripo3dOptions { quad: true, ..OPTS }), Some(25));
        assert_eq!(one(Tripo3dOptions { style: true, ..OPTS }), Some(25));
        assert_eq!(
            one(Tripo3dOptions {
                geometry_quality: Tripo3dGeometryQuality::Detailed,
                ..OPTS
            }),
            Some(40)
        );
        // Texture quality tiers over the no-texture base: standard +10, detailed +20, extreme +30.
        let tex = |quality| {
            one(Tripo3dOptions {
                texture: true,
                texture_quality: quality,
                ..OPTS
            })
        };
        assert_eq!(tex(Tripo3dTextureQuality::Standard), Some(30));
        assert_eq!(tex(Tripo3dTextureQuality::Detailed), Some(40));
        assert_eq!(tex(Tripo3dTextureQuality::Extreme), Some(50));
        // A texture quality tier without the texture flag is not a priced combination.
        assert_eq!(
            one(Tripo3dOptions {
                texture_quality: Tripo3dTextureQuality::Detailed,
                ..OPTS
            }),
            None
        );
    }

    #[test]
    fn std_texture_base_includes_pbr() {
        let pbr = Tripo3dOptions {
            texture: true,
            pbr: true,
            ..OPTS
        };
        let no_pbr = Tripo3dOptions {
            texture: true,
            ..OPTS
        };
        for version in ["v3.0-20250812", "P1-20260311"] {
            assert_eq!(
                price(Tripo3dTaskKind::TextToModel, Some(version), pbr),
                price(Tripo3dTaskKind::TextToModel, Some(version), no_pbr),
                "{version}"
            );
        }
    }

    #[test]
    fn p1_is_all_in_and_rejects_surcharges() {
        let p1 = Some("P1-20260311");
        let rejected = [
            Tripo3dOptions { smart_low_poly: true, ..OPTS },
            Tripo3dOptions { generate_parts: true, ..OPTS },
            Tripo3dOptions { quad: true, ..OPTS },
            Tripo3dOptions { style: true, ..OPTS },
            Tripo3dOptions {
                geometry_quality: Tripo3dGeometryQuality::Detailed,
                ..OPTS
            },
            Tripo3dOptions {
                texture: true,
                texture_quality: Tripo3dTextureQuality::Detailed,
                ..OPTS
            },
            Tripo3dOptions {
                texture: true,
                texture_quality: Tripo3dTextureQuality::Extreme,
                ..OPTS
            },
        ];
        for options in rejected {
            assert_eq!(price(Tripo3dTaskKind::TextToModel, p1, options), None);
        }
        // Admitted on P1: the texture base selector at standard quality and the free pbr flag.
        assert_eq!(price(Tripo3dTaskKind::TextToModel, p1, OPTS), Some(30));
        assert_eq!(
            price(
                Tripo3dTaskKind::TextToModel,
                p1,
                Tripo3dOptions { texture: true, ..OPTS }
            ),
            Some(40)
        );
        assert_eq!(
            price(
                Tripo3dTaskKind::ImageToModel,
                p1,
                Tripo3dOptions {
                    texture: true,
                    pbr: true,
                    ..OPTS
                }
            ),
            Some(50)
        );
    }

    #[test]
    fn legacy_v14_admits_no_options() {
        let textured = Tripo3dOptions {
            texture: true,
            ..OPTS
        };
        assert_eq!(
            price(Tripo3dTaskKind::TextToModel, Some("v1.4-20240625"), textured),
            None
        );
        assert_eq!(
            price(
                Tripo3dTaskKind::RefineModel,
                Some("v1.4-20240625"),
                Tripo3dOptions { style: true, ..OPTS }
            ),
            None
        );
    }

    #[test]
    fn texture_model_is_priced_by_texture_quality() {
        let v = Some("v3.0-20250812");
        let quality = |texture_quality| Tripo3dOptions {
            texture_quality,
            ..OPTS
        };
        assert_eq!(
            price(Tripo3dTaskKind::TextureModel, v, quality(Tripo3dTextureQuality::Standard)),
            Some(10)
        );
        assert_eq!(
            price(Tripo3dTaskKind::TextureModel, v, quality(Tripo3dTextureQuality::Detailed)),
            Some(20)
        );
        assert_eq!(
            price(Tripo3dTaskKind::TextureModel, v, quality(Tripo3dTextureQuality::Extreme)),
            Some(30)
        );
        // +5 style reference.
        assert_eq!(
            price(
                Tripo3dTaskKind::TextureModel,
                Some("v2.5-20250123"),
                Tripo3dOptions { style: true, ..OPTS }
            ),
            Some(15)
        );
        // Generation-only options do not apply to texture_model.
        assert_eq!(
            price(
                Tripo3dTaskKind::TextureModel,
                v,
                Tripo3dOptions { quad: true, ..OPTS }
            ),
            None
        );
        // Version set is exactly {v3.0-20250812, v2.5-20250123}.
        assert_eq!(price(Tripo3dTaskKind::TextureModel, Some("P1-20260311"), OPTS), None);
    }

    #[test]
    fn flat_priced_tasks_match_the_card() {
        assert_eq!(
            price(Tripo3dTaskKind::MeshSegmentation, Some("v1.0-20250506"), OPTS),
            Some(40)
        );
        assert_eq!(price(Tripo3dTaskKind::MeshSegmentation, None, OPTS), Some(40));
        // The undocumented apiv3 version fails closed.
        assert_eq!(
            price(Tripo3dTaskKind::MeshSegmentation, Some("v2.0-20260430"), OPTS),
            None
        );
        assert_eq!(
            price(Tripo3dTaskKind::MeshCompletion, Some("v1.0-20250506"), OPTS),
            Some(50)
        );
        // The overview's P-v2.0 mention is entangled in the highpoly conflict: fail closed.
        assert_eq!(
            price(Tripo3dTaskKind::MeshCompletion, Some("P-v2.0-20251225"), OPTS),
            None
        );
        for version in ["v2.5-20260210", "v2.0-20250506", "v1.0-20240301"] {
            assert_eq!(
                price(Tripo3dTaskKind::AnimateRig, Some(version), OPTS),
                Some(25),
                "{version}"
            );
        }
        assert_eq!(
            price(Tripo3dTaskKind::AnimateRig, Some("v9.9-20990101"), OPTS),
            None
        );
    }

    #[test]
    fn free_tasks_are_an_explicit_documented_zero() {
        for version in [Some("v2.0-20250506"), Some("v1.0-20240301"), None] {
            assert_eq!(
                price(Tripo3dTaskKind::AnimatePrerigcheck, version, OPTS),
                Some(0)
            );
        }
        assert_eq!(price(Tripo3dTaskKind::ImportModel, None, OPTS), Some(0));
        // Version-independent tasks reject a supplied version.
        assert_eq!(
            price(Tripo3dTaskKind::ImportModel, Some("v1.0-20240301"), OPTS),
            None
        );
        // Zero cost still converts to exactly zero nanoUSD.
        assert_eq!(tripo3d_cost_nanodollars(0), Ok(0));
    }

    #[test]
    fn highpoly_to_lowpoly_conflict_fails_closed() {
        // Neither the docs spelling nor the SDK spelling is accepted until live-probed.
        assert_eq!(
            price(Tripo3dTaskKind::HighpolyToLowpoly, Some("P-v2.0-20251225"), OPTS),
            None
        );
        assert_eq!(
            price(Tripo3dTaskKind::HighpolyToLowpoly, Some("P-v2.0-20251226"), OPTS),
            None
        );
        assert_eq!(price(Tripo3dTaskKind::HighpolyToLowpoly, None, OPTS), None);
    }

    #[test]
    fn per_unit_tasks_use_counted_helpers() {
        // animate_retarget: 10 per animation.
        assert_eq!(tripo3d_animate_retarget_credits(1), Some(10));
        assert_eq!(tripo3d_animate_retarget_credits(16), Some(160));
        assert_eq!(tripo3d_animate_retarget_credits(0), None);
        assert_eq!(tripo3d_animate_retarget_credits(u64::MAX), None);
        // edit_multiview_image: 5 per edited image.
        assert_eq!(tripo3d_edit_multiview_image_credits(1), Some(5));
        assert_eq!(tripo3d_edit_multiview_image_credits(4), Some(20));
        assert_eq!(tripo3d_edit_multiview_image_credits(0), None);
        assert_eq!(tripo3d_edit_multiview_image_credits(u64::MAX), None);
        // convert_model: basic 5 / advanced 10.
        assert_eq!(tripo3d_convert_model_credits(Tripo3dConvertMode::Basic), 5);
        assert_eq!(tripo3d_convert_model_credits(Tripo3dConvertMode::Advanced), 10);
        // The flat entry point refuses the per-unit kinds rather than guessing a count.
        assert_eq!(price(Tripo3dTaskKind::AnimateRetarget, None, OPTS), None);
        assert_eq!(price(Tripo3dTaskKind::ConvertModel, None, OPTS), None);
        assert_eq!(price(Tripo3dTaskKind::EditMultiviewImage, None, OPTS), None);
    }

    #[test]
    fn image_tasks_match_the_card() {
        for version in VERSIONS_IMAGE {
            assert_eq!(
                price(Tripo3dTaskKind::TextToImage, Some(version), OPTS),
                Some(5),
                "{version}"
            );
            // generate_image: conservative upper bound 10 ("5 or 10", split unpublished).
            assert_eq!(
                price(Tripo3dTaskKind::GenerateImage, Some(version), OPTS),
                Some(10),
                "{version}"
            );
        }
        assert_eq!(
            price(Tripo3dTaskKind::TextToImage, Some("dall-e-3"), OPTS),
            None
        );
        assert_eq!(
            price(Tripo3dTaskKind::GenerateMultiviewImage, None, OPTS),
            Some(10)
        );
        assert_eq!(
            price(Tripo3dTaskKind::GenerateMultiviewImage, Some("flux.1_dev"), OPTS),
            None
        );
    }

    #[test]
    fn unknown_task_model_or_option_combination_fails_closed() {
        assert_eq!(
            price(Tripo3dTaskKind::TextToModel, Some("v4.0-20270101"), OPTS),
            None
        );
        assert_eq!(
            price(Tripo3dTaskKind::TextToModel, Some("p1-20260311"), OPTS), // case-sensitive
            None
        );
        assert_eq!(
            price(
                Tripo3dTaskKind::MeshSegmentation,
                None,
                Tripo3dOptions { style: true, ..OPTS }
            ),
            None
        );
    }

    #[test]
    fn nanodollar_conversion_is_exact() {
        assert_eq!(tripo3d_cost_nanodollars(1), Ok(10_000_000));
        assert_eq!(tripo3d_cost_nanodollars(20), Ok(200_000_000));
        // A P1 textured image_to_model (50 credits) is exactly $0.50.
        assert_eq!(tripo3d_cost_nanodollars(50), Ok(500_000_000));
        assert_eq!(tripo3d_cost_nanodollars(100), Ok(crate::NANO_PER_USD));
    }

    #[test]
    fn conversion_errors_are_typed() {
        assert_eq!(
            tripo3d_cost_nanodollars(-1),
            Err(Tripo3dMeteringError::NegativeCredits)
        );
        assert_eq!(
            tripo3d_cost_nanodollars(i128::MAX),
            Err(Tripo3dMeteringError::Overflow)
        );
    }

    #[test]
    fn tariff_family_is_the_schedule_identity_without_date() {
        assert_eq!(tripo3d_tariff_family(), "tripo3d/openapi");
        assert!(
            TRIPO3D_TARIFF_SCHEDULE_ID.starts_with(TRIPO3D_TARIFF_FAMILY),
            "the family must remain a prefix of the pinned schedule id"
        );
        assert_eq!(
            tripo3d_tariff_schedule_id().as_str(),
            TRIPO3D_TARIFF_SCHEDULE_ID
        );
    }
}
