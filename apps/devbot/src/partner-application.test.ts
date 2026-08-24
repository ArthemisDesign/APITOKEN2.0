import { describe, expect, it } from "vitest";
import {
  mapPartnerApplicationPayload,
  PARTNER_REVIEW_QUEUE_URL,
  truncatePartnerMessage,
} from "./partner-application.js";

const SAMPLE = {
  event: "submitted" as const,
  id: "6a3a0f4b-2f24-4a2e-a0a5-2f1ad2d7f4b1",
  email: "partner@example.com",
  status: "pending" as const,
  message: "I run an agency with three AI products.",
  createdAt: "2026-08-23T09:00:00.000Z",
  reviewerActor: null,
  reviewerNote: null,
};

describe("mapPartnerApplicationPayload", () => {
  it("maps a submitted application", () => {
    expect(mapPartnerApplicationPayload(SAMPLE)).toEqual(SAMPLE);
  });

  it("maps a decision and treats blank reviewer fields as null", () => {
    expect(mapPartnerApplicationPayload({
      ...SAMPLE,
      event: "decided",
      status: "approved",
      reviewerActor: " ops ",
      reviewerNote: "",
    })).toEqual({
      ...SAMPLE,
      event: "decided",
      status: "approved",
      reviewerActor: "ops",
      reviewerNote: null,
    });
  });

  it("ignores unknown extra fields including userId", () => {
    expect(mapPartnerApplicationPayload({ ...SAMPLE, userId: "secret", extra: true })).toEqual(SAMPLE);
  });

  it("throws on a non-object body and on missing identity", () => {
    expect(() => mapPartnerApplicationPayload("nope")).toThrow(/object/);
    expect(() => mapPartnerApplicationPayload({ ...SAMPLE, event: "ping" })).toThrow(/event/);
    expect(() => mapPartnerApplicationPayload({ ...SAMPLE, id: "" })).toThrow(/id/);
    expect(() => mapPartnerApplicationPayload({ ...SAMPLE, email: "  " })).toThrow(/email/);
    expect(() => mapPartnerApplicationPayload({ ...SAMPLE, status: "queued" })).toThrow(/status/);
  });
});

describe("truncatePartnerMessage", () => {
  it("keeps short text and ellipsises a long body", () => {
    expect(truncatePartnerMessage("hello")).toBe("hello");
    expect(truncatePartnerMessage("x".repeat(1501)).endsWith("…")).toBe(true);
    expect(truncatePartnerMessage("x".repeat(1501)).length).toBe(1500);
  });
});

describe("review queue URL", () => {
  it("points at the Admin access-application queue", () => {
    expect(PARTNER_REVIEW_QUEUE_URL).toBe("https://admin.apitoken.sale/partners/applications");
  });
});
