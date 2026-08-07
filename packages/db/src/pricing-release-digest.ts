/**
 * Leaf module with the shared pricing digest helpers.
 *
 * Two content-addressing domains live here side by side so the hot pricing writers never
 * import the legacy release-cycle libraries:
 *
 * - `stage5V2CanonicalJson` / `stage5V2Digest` — the release-v2 domain (`pricing-stage5-v2:`,
 *   `sha256:v2:`), thin re-wrappers of the canonical primitives in
 *   `packages/engine-client/src/release-provisioning.ts`.
 * - `stage5Digest` — the legacy v1 domain (`multi-discount-stage5:`, `sha256:v1:`) that the
 *   managed catalog/switch/policy writers in `pricing-policy-write.ts` still pin, plus the
 *   reviewed catalog entry list `STAGE5_CATALOG_MODELS`.
 *
 * Digest algorithms are frozen contracts: any change here silently invalidates every stored
 * content digest, so this file must stay byte-compatible — extend, never modify.
 */
import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import { CURRENT_PRODUCT_CATALOG_ENTRIES } from "@claude-api/contracts";
import {
  canonicalPricingReleaseV2Json,
  pricingReleaseV2Digest,
} from "@claude-api/engine-client";

export function stage5V2CanonicalJson(value: unknown): string {
  return canonicalPricingReleaseV2Json(value);
}

export function stage5V2Digest(label: string, value: unknown): string {
  return pricingReleaseV2Digest(label, value);
}

/** The reviewed product catalog entries every managed catalog generation builds from. */
export const STAGE5_CATALOG_MODELS = CURRENT_PRODUCT_CATALOG_ENTRIES;

function compareUtf8(left: string, right: string): number {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

function canonicalValue(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonicalValue);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>)
        .filter(([, child]) => child !== undefined)
        .sort(([left], [right]) => compareUtf8(left, right))
        .map(([key, child]) => [key, canonicalValue(child)]),
    );
  }
  return value;
}

function canonicalJson(value: unknown): string {
  return JSON.stringify(canonicalValue(value));
}

export function stage5Digest(label: string, value: unknown): string {
  const hex = createHash("sha256")
    .update(`multi-discount-stage5:${label}\n`, "utf8")
    .update(canonicalJson(value), "utf8")
    .digest("hex");
  return `sha256:v1:${hex}`;
}
