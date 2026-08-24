import type { PartnerApplicationEvent } from "./events.js";

/** Loopback path prefix; the path secret is the final segment. */
export const PARTNER_WEBHOOK_PREFIX = "/hooks/partners/";

const MESSAGE_LIMIT = 1500;
const EVENTS = new Set(["submitted", "updated", "decided"]);
const STATUSES = new Set(["pending", "approved", "rejected"]);

function asRecord(value: unknown): Record<string, unknown> | undefined {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return undefined;
  return value as Record<string, unknown>;
}

function asString(value: unknown): string | undefined {
  if (typeof value !== "string") return undefined;
  const trimmed = value.trim();
  return trimmed === "" ? undefined : trimmed;
}

function asNullableString(value: unknown): string | null {
  if (value === null || value === undefined) return null;
  if (typeof value !== "string") throw new Error("partner application payload has an invalid reviewer field");
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

/**
 * Maps a Commerce partner-application webhook body.
 * Throws when the body is not a usable JSON object — that is a 400, not a silent skip.
 */
export function mapPartnerApplicationPayload(body: unknown): PartnerApplicationEvent {
  const payload = asRecord(body);
  if (!payload) throw new Error("payload must be an object");
  const event = asString(payload.event);
  const id = asString(payload.id);
  const email = asString(payload.email);
  const status = asString(payload.status);
  if (!event || !EVENTS.has(event)) throw new Error("partner application event is missing or unknown");
  if (!id) throw new Error("partner application id is missing");
  if (!email) throw new Error("partner application email is missing");
  if (!status || !STATUSES.has(status)) throw new Error("partner application status is missing or unknown");
  const message = typeof payload.message === "string" ? payload.message : "";
  const createdAt = asString(payload.createdAt) ?? "";
  return {
    event: event as PartnerApplicationEvent["event"],
    id,
    email,
    status: status as PartnerApplicationEvent["status"],
    message,
    createdAt,
    reviewerActor: asNullableString(payload.reviewerActor),
    reviewerNote: asNullableString(payload.reviewerNote),
  };
}

export function truncatePartnerMessage(text: string, limit: number = MESSAGE_LIMIT): string {
  if (text.length <= limit) return text;
  return `${text.slice(0, limit - 1)}…`;
}

export const PARTNER_REVIEW_QUEUE_URL = "https://admin.apitoken.sale/partners/applications";
