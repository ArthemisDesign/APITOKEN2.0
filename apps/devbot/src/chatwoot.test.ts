import { createHmac } from "node:crypto";
import { describe, expect, it } from "vitest";
import {
  CHATWOOT_SIGNATURE_MAX_AGE_SEC,
  chatwootConversationUrl,
  mapChatwootPayload,
  truncateChatwootContent,
  verifyChatwootSignature,
} from "./chatwoot.js";

function incomingPayload(overrides: Record<string, unknown> = {}) {
  return {
    event: "message_created",
    id: 42,
    content: "Hello, I cannot top up",
    message_type: "incoming",
    private: false,
    created_at: "2026-08-22 12:00:00 UTC",
    sender: { id: 7, name: "Jane Doe", email: "jane@example.com", type: "contact" },
    inbox: { id: 1, name: "Website" },
    conversation: { display_id: 15, id: 900, account_id: 1, channel: "Channel::WebWidget" },
    account: { id: 1, name: "APIToken" },
    attachments: [],
    ...overrides,
  };
}

describe("mapChatwootPayload", () => {
  it("maps an incoming client message", () => {
    expect(mapChatwootPayload(incomingPayload())).toEqual({
      id: "42",
      content: "Hello, I cannot top up",
      createdAt: "2026-08-22 12:00:00 UTC",
      conversationId: "15",
      accountId: "1",
      inboxName: "Website",
      channel: "Channel::WebWidget",
      name: "Jane Doe",
      email: "jane@example.com",
      attachments: [],
    });
  });

  it("accepts numeric message_type 0 as incoming", () => {
    const mapped = mapChatwootPayload(incomingPayload({ message_type: 0 }));
    expect(mapped?.id).toBe("42");
  });

  it("ignores outgoing, private notes, activity, and other events", () => {
    expect(mapChatwootPayload(incomingPayload({ message_type: "outgoing" }))).toBeNull();
    expect(mapChatwootPayload(incomingPayload({ message_type: 1 }))).toBeNull();
    expect(mapChatwootPayload(incomingPayload({ private: true }))).toBeNull();
    expect(mapChatwootPayload(incomingPayload({ message_type: "activity" }))).toBeNull();
    expect(mapChatwootPayload(incomingPayload({ event: "conversation_created" }))).toBeNull();
    expect(mapChatwootPayload(incomingPayload({ event: "message_updated" }))).toBeNull();
  });

  it("keeps attachment-only incoming messages", () => {
    const mapped = mapChatwootPayload(incomingPayload({
      content: null,
      attachments: [{ file_name: "invoice.pdf", file_type: "file" }],
    }));
    expect(mapped?.content).toBe("");
    expect(mapped?.attachments).toEqual([{ fileName: "invoice.pdf", fileType: "file" }]);
  });

  it("prefers conversation.display_id for the dashboard URL", () => {
    const mapped = mapChatwootPayload(incomingPayload());
    expect(mapped).not.toBeNull();
    expect(chatwootConversationUrl("https://support.apitoken.sale/", mapped!)).toBe(
      "https://support.apitoken.sale/app/accounts/1/conversations/15",
    );
  });

  it("throws on a non-object body and on incoming messages without ids", () => {
    expect(() => mapChatwootPayload("nope")).toThrow(/object/);
    expect(() => mapChatwootPayload(incomingPayload({ id: "", conversation: {}, account: {} }))).toThrow(/missing/);
  });
});

describe("verifyChatwootSignature", () => {
  const secret = "webhook-hmac-secret";
  const body = '{"event":"message_created"}';
  const timestamp = "1700000000";

  function sign(ts: string, raw: string): string {
    return `sha256=${createHmac("sha256", secret).update(`${ts}.${raw}`).digest("hex")}`;
  }

  it("accepts a fresh matching signature", () => {
    expect(verifyChatwootSignature(body, timestamp, sign(timestamp, body), secret, Number(timestamp))).toBe(true);
  });

  it("rejects a wrong secret, wrong body, missing header, and expired timestamp", () => {
    const signature = sign(timestamp, body);
    expect(verifyChatwootSignature(body, timestamp, signature, "other", Number(timestamp))).toBe(false);
    expect(verifyChatwootSignature('{"event":"x"}', timestamp, signature, secret, Number(timestamp))).toBe(false);
    expect(verifyChatwootSignature(body, timestamp, "", secret, Number(timestamp))).toBe(false);
    expect(verifyChatwootSignature(
      body,
      timestamp,
      signature,
      secret,
      Number(timestamp) + CHATWOOT_SIGNATURE_MAX_AGE_SEC + 1,
    )).toBe(false);
  });
});

describe("truncateChatwootContent", () => {
  it("leaves short text intact and ellipsizes long text", () => {
    expect(truncateChatwootContent("hi")).toBe("hi");
    expect(truncateChatwootContent("x".repeat(20), 8)).toBe(`${"x".repeat(7)}…`);
  });
});
