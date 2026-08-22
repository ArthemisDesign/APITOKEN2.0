import { createHmac, timingSafeEqual } from "node:crypto";
import type { ChatwootIncomingMessage } from "./events.js";

/** Public and loopback path prefix; the path secret is the final segment. */
export const CHATWOOT_WEBHOOK_PREFIX = "/hooks/devbot/";

/** Reject HMAC timestamps older than this (Chatwoot replay window). */
export const CHATWOOT_SIGNATURE_MAX_AGE_SEC = 5 * 60;

const CONTENT_LIMIT = 1500;

function asRecord(value: unknown): Record<string, unknown> | undefined {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return undefined;
  return value as Record<string, unknown>;
}

function asString(value: unknown): string | undefined {
  if (typeof value === "string") {
    const trimmed = value.trim();
    return trimmed === "" ? undefined : trimmed;
  }
  if (typeof value === "number" && Number.isFinite(value)) return String(value);
  return undefined;
}

function isIncomingMessageType(value: unknown): boolean {
  return value === "incoming" || value === 0 || value === "0";
}

/**
 * Constant-time HMAC check for Chatwoot signed deliveries:
 * `sha256=HMAC-SHA256(secret, "{timestamp}.{raw_body}")`.
 */
export function verifyChatwootSignature(
  rawBody: string,
  timestamp: string,
  receivedSignature: string,
  secret: string,
  nowSec: number = Math.floor(Date.now() / 1000),
): boolean {
  if (receivedSignature === "" || timestamp === "" || secret === "") return false;
  const ts = Number(timestamp);
  if (!Number.isFinite(ts) || Math.abs(nowSec - ts) > CHATWOOT_SIGNATURE_MAX_AGE_SEC) return false;
  const expected = `sha256=${createHmac("sha256", secret).update(`${timestamp}.${rawBody}`).digest("hex")}`;
  const expectedBuf = Buffer.from(expected);
  const receivedBuf = Buffer.from(receivedSignature);
  if (expectedBuf.length !== receivedBuf.length) return false;
  return timingSafeEqual(expectedBuf, receivedBuf);
}

function contactFrom(payload: Record<string, unknown>): { name?: string; email?: string } {
  const sender = asRecord(payload.sender);
  const contact = asRecord(payload.contact);
  const conversation = asRecord(payload.conversation);
  const meta = asRecord(conversation?.meta);
  const metaSender = asRecord(meta?.sender);
  const name = asString(sender?.name) ?? asString(contact?.name) ?? asString(metaSender?.name);
  const email = asString(sender?.email) ?? asString(contact?.email) ?? asString(metaSender?.email);
  return { ...(name ? { name } : {}), ...(email ? { email } : {}) };
}

function attachmentsFrom(payload: Record<string, unknown>): ChatwootIncomingMessage["attachments"] {
  const raw = payload.attachments;
  if (!Array.isArray(raw)) return [];
  const out: ChatwootIncomingMessage["attachments"] = [];
  for (const item of raw) {
    const rec = asRecord(item);
    if (!rec) continue;
    const fileName = asString(rec.file_name) ?? asString(rec.filename);
    const fileType = asString(rec.file_type) ?? asString(rec.content_type);
    if (!fileName && !fileType) continue;
    out.push({
      ...(fileName ? { fileName } : {}),
      ...(fileType ? { fileType } : {}),
    });
  }
  return out;
}

/**
 * Maps a Chatwoot webhook JSON body to an incoming client message.
 * Returns null for events the bot must ignore (outgoing, private notes, activity, other events).
 * Throws only when the body is not a JSON object — that is a 400, not a silent skip.
 */
export function mapChatwootPayload(body: unknown): ChatwootIncomingMessage | null {
  const payload = asRecord(body);
  if (!payload) throw new Error("payload must be an object");
  if (payload.event !== "message_created") return null;
  if (payload.private === true) return null;
  if (!isIncomingMessageType(payload.message_type)) return null;

  const conversation = asRecord(payload.conversation);
  const account = asRecord(payload.account);
  const inbox = asRecord(payload.inbox);
  const id = asString(payload.id);
  const conversationId = asString(conversation?.display_id) ?? asString(conversation?.id);
  const accountId = asString(account?.id) ?? asString(conversation?.account_id);
  if (!id || !conversationId || !accountId) {
    throw new Error("message_created is missing id, conversation id, or account id");
  }

  const contact = contactFrom(payload);
  const attachments = attachmentsFrom(payload);
  const content = typeof payload.content === "string" ? payload.content : "";
  const createdAt = asString(payload.created_at) ?? "";
  const inboxName = asString(inbox?.name);
  const channel = asString(conversation?.channel) ?? asString(inbox?.channel);

  return {
    id,
    content,
    createdAt,
    conversationId,
    accountId,
    attachments,
    ...(inboxName ? { inboxName } : {}),
    ...(channel ? { channel } : {}),
    ...contact,
  };
}

export function chatwootConversationUrl(baseUrl: string, message: ChatwootIncomingMessage): string {
  const root = baseUrl.replace(/\/+$/, "");
  return `${root}/app/accounts/${encodeURIComponent(message.accountId)}/conversations/${encodeURIComponent(message.conversationId)}`;
}

export function truncateChatwootContent(text: string, limit: number = CONTENT_LIMIT): string {
  if (text.length <= limit) return text;
  return `${text.slice(0, limit - 1)}…`;
}
