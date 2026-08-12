//! Official Tripo3D API platform per-task credit rate card and nanoUSD conversion.
//!
//! Authority: `docs/engine/TRIPO3D_PROVIDER.md` §5.1 (official `billing.md` rate card) and §3
//! (per-task `model_version` admission sets), reviewed 2026-08-12. Tripo3D publishes a fixed
//! dollar link for its native credit unit — **$0.01 per credit** — so the API-dollar leg is
//! exact, not estimated. This module prices the **reserve**; the per-turn settlement authority
//! is the provider-reported `consumed_credit`, and a settled amount above the reserved maximum
//! for the admitted task shape is a typed anomaly, never silent acceptance.
//!
//! Reserve rule (two tiers, both honest):
//!
//! 1. **Exact** — [`tripo3d_task_credits`]: the published price of the exact
//!    task/version/option combination. A documented free task (`animate_prerigcheck`,
//!    `import_model`) returns `Some(0)`: an explicit, documented official zero.
//! 2. **Conservative** — [`tripo3d_reserve_credits`]: when the task kind and the
//!    `model_version` are reviewed but the exact option combination has no published price, the
//!    reserve is the highest published price of the task's family
//!    ([`tripo3d_family_max_credits`]), flagged `conservative`. Settlement never follows the
//!    reserve: it is exactly the authoritative `consumed_credit`.
//!
//! Fail-closed stays fail-closed: an unknown task kind, an unlisted `model_version`, the
//! conflicted `highpoly_to_lowpoly` (docs-vs-SDK version conflict, manifest §3/§6.6) and an
//! unpriceable per-unit count return `None` — never a guessed price. All arithmetic is checked
//! integer math.

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

// ── full-catalog admission: wire names, conservative family reserves ────────

impl Tripo3dTaskKind {
    /// The provider's exact wire discriminator (manifest §3 catalog, reviewed 2026-08-12).
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::TextToModel => "text_to_model",
            Self::ImageToModel => "image_to_model",
            Self::MultiviewToModel => "multiview_to_model",
            Self::TextureModel => "texture_model",
            Self::MeshSegmentation => "mesh_segmentation",
            Self::MeshCompletion => "mesh_completion",
            Self::HighpolyToLowpoly => "highpoly_to_lowpoly",
            Self::AnimatePrerigcheck => "animate_prerigcheck",
            Self::AnimateRig => "animate_rig",
            Self::AnimateRetarget => "animate_retarget",
            Self::ConvertModel => "convert_model",
            Self::ImportModel => "import_model",
            Self::TextToImage => "text_to_image",
            Self::GenerateImage => "generate_image",
            Self::GenerateMultiviewImage => "generate_multiview_image",
            Self::EditMultiviewImage => "edit_multiview_image",
            Self::RefineModel => "refine_model",
        }
    }

    /// The task kind for a wire discriminator. An unknown `type` names nothing — the caller
    /// fails closed with the admitted set, never a guess.
    pub fn from_wire(wire: &str) -> Option<Self> {
        Some(match wire {
            "text_to_model" => Self::TextToModel,
            "image_to_model" => Self::ImageToModel,
            "multiview_to_model" => Self::MultiviewToModel,
            "texture_model" => Self::TextureModel,
            "mesh_segmentation" => Self::MeshSegmentation,
            "mesh_completion" => Self::MeshCompletion,
            "highpoly_to_lowpoly" => Self::HighpolyToLowpoly,
            "animate_prerigcheck" => Self::AnimatePrerigcheck,
            "animate_rig" => Self::AnimateRig,
            "animate_retarget" => Self::AnimateRetarget,
            "convert_model" => Self::ConvertModel,
            "import_model" => Self::ImportModel,
            "text_to_image" => Self::TextToImage,
            "generate_image" => Self::GenerateImage,
            "generate_multiview_image" => Self::GenerateMultiviewImage,
            "edit_multiview_image" => Self::EditMultiviewImage,
            "refine_model" => Self::RefineModel,
            _ => return None,
        })
    }

    /// Per-unit kinds price by count (credits per animation / per edited image / per mode), so
    /// the flat reserve helper cannot price them — the caller uses the counted helpers with the
    /// request's actual count.
    pub fn is_per_unit(self) -> bool {
        matches!(
            self,
            Self::AnimateRetarget | Self::ConvertModel | Self::EditMultiviewImage
        )
    }
}

/// Whether a supplied `model_version` sits in the kind's reviewed admission set (manifest §3).
/// Version-independent kinds admit no version at all. An omitted version is the provider's
/// documented default and is always admitted.
fn version_admitted(kind: Tripo3dTaskKind, model_version: Option<&str>) -> bool {
    let Some(version) = model_version else {
        return true;
    };
    match kind {
        Tripo3dTaskKind::TextToModel | Tripo3dTaskKind::ImageToModel => {
            VERSIONS_TO_MODEL.contains(&version)
        }
        Tripo3dTaskKind::MultiviewToModel => VERSIONS_MULTIVIEW.contains(&version),
        Tripo3dTaskKind::TextureModel => VERSIONS_TEXTURE_MODEL.contains(&version),
        Tripo3dTaskKind::MeshSegmentation => VERSIONS_MESH_SEGMENTATION.contains(&version),
        Tripo3dTaskKind::MeshCompletion => VERSIONS_MESH_COMPLETION.contains(&version),
        Tripo3dTaskKind::AnimatePrerigcheck => VERSIONS_ANIMATE_PRERIGCHECK.contains(&version),
        Tripo3dTaskKind::AnimateRig => VERSIONS_ANIMATE_RIG.contains(&version),
        Tripo3dTaskKind::RefineModel => version == "v1.4-20240625",
        Tripo3dTaskKind::TextToImage | Tripo3dTaskKind::GenerateImage => {
            VERSIONS_IMAGE.contains(&version)
        }
        // Version-independent kinds: a supplied version fails closed, exactly as the exact
        // pricer treats it.
        Tripo3dTaskKind::HighpolyToLowpoly
        | Tripo3dTaskKind::AnimateRetarget
        | Tripo3dTaskKind::ConvertModel
        | Tripo3dTaskKind::ImportModel
        | Tripo3dTaskKind::GenerateMultiviewImage
        | Tripo3dTaskKind::EditMultiviewImage => false,
    }
}

/// The highest published price in a task kind's family (§5.1), for the conservative reserve of
/// a reviewed-but-unpriced option combination. `None` only where no honest bound exists:
/// `highpoly_to_lowpoly` (conflicted, fail closed) and the per-unit kinds (their bound is
/// count-derived, so a flat maximum would be a fabrication).
///
/// The generation maxima are the standard tier with every published surcharge stacked:
/// base + extreme texture (30) + smart_low_poly (10) + generate_parts (20) + quad (5) +
/// style (5) + detailed geometry (20) — P1 (all-in, max 50) and legacy v1.4 (flat) never
/// exceed the stacked standard tier. A regression test recomputes these from the exact pricer.
pub fn tripo3d_family_max_credits(kind: Tripo3dTaskKind) -> Option<i64> {
    Some(match kind {
        Tripo3dTaskKind::TextToModel => 100,
        Tripo3dTaskKind::ImageToModel | Tripo3dTaskKind::MultiviewToModel => 110,
        Tripo3dTaskKind::TextureModel => 35,
        Tripo3dTaskKind::MeshSegmentation => 40,
        Tripo3dTaskKind::MeshCompletion => 50,
        Tripo3dTaskKind::AnimatePrerigcheck => 0,
        Tripo3dTaskKind::AnimateRig => 25,
        Tripo3dTaskKind::ImportModel => 0,
        Tripo3dTaskKind::TextToImage => 5,
        Tripo3dTaskKind::GenerateImage => 10,
        Tripo3dTaskKind::GenerateMultiviewImage => 10,
        Tripo3dTaskKind::RefineModel => 30,
        Tripo3dTaskKind::HighpolyToLowpoly
        | Tripo3dTaskKind::AnimateRetarget
        | Tripo3dTaskKind::ConvertModel
        | Tripo3dTaskKind::EditMultiviewImage => return None,
    })
}

/// The reserve for one admitted request.
///
/// `conservative: false` — the exact published price of the combination. `conservative: true` —
/// the combination is reviewed (kind + `model_version` admitted) but has no published price, so
/// the reserve is the family's highest published price; settlement is still exactly the
/// authoritative `consumed_credit`, and the reserved value is also the anomaly cross-check
/// bound (a settlement above it is quarantined, never silently accepted).
///
/// `None` — fail closed: per-unit kind (price by count via the dedicated helpers), conflicted
/// `highpoly_to_lowpoly`, or a supplied `model_version` outside the reviewed set.
pub fn tripo3d_reserve_credits(
    kind: Tripo3dTaskKind,
    model_version: Option<&str>,
    options: &Tripo3dOptions,
) -> Option<Tripo3dReserve> {
    if let Some(credits) = tripo3d_task_credits(kind, model_version, options) {
        return Some(Tripo3dReserve {
            credits,
            conservative: false,
        });
    }
    if kind.is_per_unit() || kind == Tripo3dTaskKind::HighpolyToLowpoly {
        return None;
    }
    if !version_admitted(kind, model_version) {
        return None;
    }
    let credits = tripo3d_family_max_credits(kind)?;
    Some(Tripo3dReserve {
        credits,
        conservative: true,
    })
}

/// The reserve decision for one admitted flat-priced request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tripo3dReserve {
    pub credits: i64,
    /// True when the price is the family's published maximum rather than the exact combination's
    /// published price. Settlement never follows the reserve either way.
    pub conservative: bool,
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

    // ── full-catalog admission (reserve rule) ───────────────────────────────

    const ALL_KINDS: [Tripo3dTaskKind; 17] = [
        Tripo3dTaskKind::TextToModel,
        Tripo3dTaskKind::ImageToModel,
        Tripo3dTaskKind::MultiviewToModel,
        Tripo3dTaskKind::TextureModel,
        Tripo3dTaskKind::MeshSegmentation,
        Tripo3dTaskKind::MeshCompletion,
        Tripo3dTaskKind::HighpolyToLowpoly,
        Tripo3dTaskKind::AnimatePrerigcheck,
        Tripo3dTaskKind::AnimateRig,
        Tripo3dTaskKind::AnimateRetarget,
        Tripo3dTaskKind::ConvertModel,
        Tripo3dTaskKind::ImportModel,
        Tripo3dTaskKind::TextToImage,
        Tripo3dTaskKind::GenerateImage,
        Tripo3dTaskKind::GenerateMultiviewImage,
        Tripo3dTaskKind::EditMultiviewImage,
        Tripo3dTaskKind::RefineModel,
    ];

    #[test]
    fn wire_names_roundtrip_the_whole_catalog() {
        for kind in ALL_KINDS {
            assert_eq!(Tripo3dTaskKind::from_wire(kind.as_wire()), Some(kind));
        }
        assert_eq!(Tripo3dTaskKind::from_wire("text_to_3d"), None);
        assert_eq!(Tripo3dTaskKind::from_wire(""), None);
        assert_eq!(Tripo3dTaskKind::from_wire("TEXT_TO_MODEL"), None);
    }

    #[test]
    fn family_max_is_the_grid_maximum_for_generation_kinds() {
        // Recompute the constant from the exact pricer: every admitted version x the full
        // surcharge grid (texture qualities incl. extreme x every boolean flag combination).
        let qualities = [
            Tripo3dTextureQuality::Standard,
            Tripo3dTextureQuality::Detailed,
            Tripo3dTextureQuality::Extreme,
        ];
        for (kind, versions, expected) in [
            (Tripo3dTaskKind::TextToModel, VERSIONS_TO_MODEL.to_vec(), 100),
            (Tripo3dTaskKind::ImageToModel, VERSIONS_TO_MODEL.to_vec(), 110),
            (Tripo3dTaskKind::MultiviewToModel, VERSIONS_MULTIVIEW.to_vec(), 110),
        ] {
            let mut grid_max = 0;
            for version in &versions {
                for texture in [false, true] {
                    for quality in qualities {
                        for bits in 0..32u8 {
                            let options = Tripo3dOptions {
                                texture,
                                pbr: bits & 1 != 0,
                                smart_low_poly: bits & 2 != 0,
                                generate_parts: bits & 4 != 0,
                                quad: bits & 8 != 0,
                                style: bits & 16 != 0,
                                texture_quality: quality,
                                geometry_quality: if bits & 1 != 0 {
                                    Tripo3dGeometryQuality::Detailed
                                } else {
                                    Tripo3dGeometryQuality::Standard
                                },
                            };
                            if let Some(credits) = price(kind, Some(version), options) {
                                grid_max = grid_max.max(credits);
                            }
                        }
                    }
                }
            }
            assert_eq!(tripo3d_family_max_credits(kind), Some(expected), "{kind:?}");
            assert_eq!(grid_max, expected, "{kind:?} grid maximum moved");
        }
    }

    #[test]
    fn family_max_covers_every_priced_combination() {
        // The conservative reserve must never under-hold: for every admitted kind/version the
        // family maximum is at least the exact price of any priced option grid cell.
        let versions: Vec<Option<&str>> = vec![
            None,
            Some("v3.1-20260211"),
            Some("P1-20260311"),
            Some("v2.5-20250123"),
            Some("v1.4-20240625"),
        ];
        let all_options = Tripo3dOptions {
            texture: true,
            pbr: true,
            smart_low_poly: true,
            generate_parts: true,
            quad: true,
            style: true,
            texture_quality: Tripo3dTextureQuality::Extreme,
            geometry_quality: Tripo3dGeometryQuality::Detailed,
        };
        for kind in ALL_KINDS {
            let Some(max) = tripo3d_family_max_credits(kind) else {
                continue;
            };
            for version in &versions {
                for options in [OPTS, all_options] {
                    if let Some(exact) = price(kind, *version, options) {
                        assert!(exact <= max, "{kind:?} {version:?} exact {exact} > max {max}");
                    }
                }
            }
        }
    }

    #[test]
    fn reserve_is_exact_when_the_card_prices_the_combination() {
        let reserve = tripo3d_reserve_credits(
            Tripo3dTaskKind::TextToModel,
            Some("v3.1-20260211"),
            &OPTS,
        )
        .unwrap();
        assert_eq!(
            reserve,
            Tripo3dReserve {
                credits: 10,
                conservative: false
            }
        );
        // Documented free tasks reserve exactly zero, not a conservative bound.
        let free = tripo3d_reserve_credits(Tripo3dTaskKind::AnimatePrerigcheck, None, &OPTS)
            .unwrap();
        assert_eq!(
            free,
            Tripo3dReserve {
                credits: 0,
                conservative: false
            }
        );
    }

    #[test]
    fn reserve_falls_back_to_the_family_max_for_unpriced_reviewed_combinations() {
        // P1 takes no surcharges on the card: reviewed kind + version, unpriced combination.
        let p1_surcharged = Tripo3dOptions {
            smart_low_poly: true,
            ..OPTS
        };
        let reserve = tripo3d_reserve_credits(
            Tripo3dTaskKind::TextToModel,
            Some("P1-20260311"),
            &p1_surcharged,
        )
        .unwrap();
        assert_eq!(
            reserve,
            Tripo3dReserve {
                credits: 100,
                conservative: true
            }
        );
        // Legacy v1.4 with any option: same conservative fallback at the family max.
        let reserve = tripo3d_reserve_credits(
            Tripo3dTaskKind::ImageToModel,
            Some("v1.4-20240625"),
            &Tripo3dOptions {
                texture: true,
                ..OPTS
            },
        )
        .unwrap();
        assert_eq!(reserve.credits, 110);
        assert!(reserve.conservative);
        // A texture quality tier without the texture flag: unpriced combination, family max.
        let reserve = tripo3d_reserve_credits(
            Tripo3dTaskKind::MultiviewToModel,
            Some("v3.0-20250812"),
            &Tripo3dOptions {
                texture_quality: Tripo3dTextureQuality::Detailed,
                ..OPTS
            },
        )
        .unwrap();
        assert_eq!(reserve.credits, 110);
        assert!(reserve.conservative);
    }

    #[test]
    fn reserve_fails_closed_where_no_honest_bound_exists() {
        // The conflicted highpoly task: neither spelling, no fallback.
        assert_eq!(
            tripo3d_reserve_credits(Tripo3dTaskKind::HighpolyToLowpoly, None, &OPTS),
            None
        );
        // Per-unit kinds price by count, never by a flat family bound.
        for kind in [
            Tripo3dTaskKind::AnimateRetarget,
            Tripo3dTaskKind::ConvertModel,
            Tripo3dTaskKind::EditMultiviewImage,
        ] {
            assert_eq!(tripo3d_reserve_credits(kind, None, &OPTS), None, "{kind:?}");
        }
        // An unlisted version stays fail-closed — the conservative fallback must not launder it.
        assert_eq!(
            tripo3d_reserve_credits(Tripo3dTaskKind::TextToModel, Some("v4.0-20270101"), &OPTS),
            None
        );
        // A version supplied to a version-independent kind stays fail-closed.
        assert_eq!(
            tripo3d_reserve_credits(
                Tripo3dTaskKind::ImportModel,
                Some("v1.0-20240301"),
                &OPTS
            ),
            None
        );
    }
}
