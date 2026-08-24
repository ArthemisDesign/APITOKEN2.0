import { afterEach, describe, expect, it, vi } from "vitest";
import { ConfigService } from "@nestjs/config";
import type { Environment } from "./config.js";
import { DevbotPartnerNotifier, type PartnerApplicationWebhookPayload } from "./devbot-partner.notifier.js";

const PAYLOAD: PartnerApplicationWebhookPayload = {
  event: "submitted",
  id: "6a3a0f4b-2f24-4a2e-a0a5-2f1ad2d7f4b1",
  email: "partner@example.com",
  status: "pending",
  message: "I run an agency.",
  createdAt: "2026-08-23T09:00:00.000Z",
  reviewerActor: null,
  reviewerNote: null,
};

function notifier(url?: string): DevbotPartnerNotifier {
  return new DevbotPartnerNotifier(new ConfigService<Environment, true>({
    ...(url === undefined ? {} : { DEVBOT_PARTNER_WEBHOOK_URL: url }),
  } as Environment));
}

describe("DevbotPartnerNotifier", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("does nothing when the webhook URL is unset", async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    notifier().notify(PAYLOAD);
    await Promise.resolve();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("POSTs the application payload to the configured loopback URL", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(null, { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    notifier("http://127.0.0.1:3800/hooks/partners/secret").notify(PAYLOAD);
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("http://127.0.0.1:3800/hooks/partners/secret");
    expect(init.method).toBe("POST");
    expect(JSON.parse(String(init.body))).toEqual(PAYLOAD);
    expect(String(init.body)).not.toContain("userId");
  });

  it("swallows delivery failures", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("http://127.0.0.1:3800/hooks/partners/secret down")));
    expect(() => notifier("http://127.0.0.1:3800/hooks/partners/secret").notify(PAYLOAD)).not.toThrow();
    await Promise.resolve();
    await Promise.resolve();
  });
});
