import type { PartnerRequestView } from "@claude-api/sales-db";

export interface PartnerRequestCursor {
  createdAt: Date;
  id: string;
}

export function encodePartnerRequestCursor(cursor: PartnerRequestCursor | null): string | null {
  if (cursor === null) return null;
  return Buffer.from(JSON.stringify({ at: cursor.createdAt.toISOString(), id: cursor.id }), "utf8")
    .toString("base64url");
}

export function decodePartnerRequestCursor(value: string | undefined): PartnerRequestCursor | undefined {
  if (value === undefined) return undefined;
  try {
    const decoded: unknown = JSON.parse(Buffer.from(value, "base64url").toString("utf8"));
    if (typeof decoded !== "object" || decoded === null || Array.isArray(decoded)) return undefined;
    const { at, id } = decoded as Record<string, unknown>;
    if (typeof at !== "string" || typeof id !== "string"
      || !/^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(id)) {
      return undefined;
    }
    const createdAt = new Date(at);
    if (!Number.isFinite(createdAt.getTime()) || createdAt.toISOString() !== at) return undefined;
    return { createdAt, id };
  } catch {
    return undefined;
  }
}

export function partnerRequestView(
  request: PartnerRequestView,
  customerEmail: string | null,
  options: { includeCommerceIdentity?: boolean } = {},
): unknown {
  return {
    id: request.id,
    requestType: request.requestType,
    status: request.status,
    requesterPartnerId: request.requesterPartnerId,
    requesterEmail: request.requesterEmail,
    requesterDisplayName: request.requesterDisplayName,
    subjectPartnerId: request.subjectPartnerId,
    ...(options.includeCommerceIdentity
      ? { customerCommerceUserId: request.commerceUserId }
      : {}),
    customerEmail,
    reason: request.reason,
    stateSnapshot: request.stateSnapshot,
    requestedCommissionBps: request.requestedCommissionBps,
    requestedDiscountBps: request.requestedDiscountBps,
    approvedCommissionBps: request.approvedCommissionBps,
    approvedDiscountBps: request.approvedDiscountBps,
    reviewerActor: request.reviewerActor,
    reviewerNote: request.reviewerNote,
    reviewedAt: request.reviewedAt?.toISOString() ?? null,
    appliedAt: request.appliedAt?.toISOString() ?? null,
    applyAttempts: request.applyAttempts,
    lastApplyError: request.lastApplyError,
    version: request.version,
    providerTerms: request.providerTerms.map((term) => ({
      providerId: term.providerId,
      requestedDiscountBps: term.requestedDiscountBps,
      approvedDiscountBps: term.approvedDiscountBps ?? null,
      decided: term.approvedDiscountBps !== undefined,
    })),
    effect: request.effect === null ? null : {
      id: request.effect.id,
      status: request.effect.status,
      attempts: request.effect.attempts,
      nextAttemptAt: request.effect.nextAttemptAt?.toISOString() ?? null,
      terminal: request.effect.terminal,
      appliedAt: request.effect.appliedAt?.toISOString() ?? null,
      lastError: request.effect.lastError,
    },
    createdAt: request.createdAt.toISOString(),
    updatedAt: request.updatedAt.toISOString(),
  };
}
