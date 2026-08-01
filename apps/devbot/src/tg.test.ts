import { describe, expect, it, vi } from "vitest";
import { TelegramBot } from "./tg.js";

const TOKEN = "123456:SECRET_TOKEN";

const instantSleep = vi.fn(async (_ms: number) => undefined);

function okResponse(result: unknown): Response {
  return new Response(JSON.stringify({ ok: true, result }), { status: 200 });
}

function makeBot(overrides: Partial<ConstructorParameters<typeof TelegramBot>[0]> = {}) {
  const fetchFn = overrides.fetchFn ?? (async () => okResponse({ message_id: 1 }));
  const bot = new TelegramBot({
    token: TOKEN,
    sleep: instantSleep,
    ...overrides,
    fetchFn,
  });
  return { bot, fetchFn };
}

describe("TelegramBot", () => {
  it("coalesces queued messages to the same thread into one send", async () => {
    const bodies: string[] = [];
    let releaseFirst: (value: Response) => void = () => undefined;
    let call = 0;
    const fetchFn: typeof fetch = async (_url, init) => {
      call += 1;
      bodies.push(String(init?.body));
      if (call === 1) {
        return await new Promise<Response>((resolve) => {
          releaseFirst = resolve;
        });
      }
      return okResponse({ message_id: call });
    };
    const { bot } = makeBot({ fetchFn });

    const first = bot.sendMessage(100, "t1", { threadId: 7 });
    // Первое сообщение уже в полёте, эти два встают в очередь и склеиваются.
    const second = bot.sendMessage(100, "t2", { threadId: 7 });
    const third = bot.sendMessage(100, "t3", { threadId: 7 });
    releaseFirst(okResponse({ message_id: 1 }));

    const ids = await Promise.all([first, second, third]);
    expect(bodies).toHaveLength(2);
    expect(JSON.parse(bodies[0] as string).text).toBe("t1");
    const merged = JSON.parse(bodies[1] as string);
    expect(merged.text).toBe("t2\nt3");
    expect(merged.message_thread_id).toBe(7);
    expect(merged.parse_mode).toBe("HTML");
    expect(merged.disable_web_page_preview).toBe(true);
    expect(ids).toEqual([1, 2, 2]);
  });

  it("throttles consecutive sends to a chat with the per-chat interval", async () => {
    instantSleep.mockClear();
    const { bot } = makeBot();
    await Promise.all([
      bot.sendMessage(100, "a", { threadId: 1 }),
      bot.sendMessage(100, "b", { threadId: 2 }),
    ]);
    const intervals = instantSleep.mock.calls.map(([ms]) => ms);
    expect(intervals).toContain(1000);
  });

  it("honors 429 retry_after before retrying", async () => {
    instantSleep.mockClear();
    let call = 0;
    const fetchFn: typeof fetch = async () => {
      call += 1;
      if (call === 1) {
        return new Response(
          JSON.stringify({ ok: false, description: "Too Many Requests", parameters: { retry_after: 2 } }),
          { status: 429 },
        );
      }
      return okResponse({ message_id: 42 });
    };
    const { bot } = makeBot({ fetchFn });
    const id = await bot.sendMessage(100, "hi");
    expect(id).toBe(42);
    expect(call).toBe(2);
    expect(instantSleep.mock.calls.map(([ms]) => ms)).toContain(2000);
  });

  it("drops the message after maxAttempts network errors and reports the failure", async () => {
    let calls = 0;
    const onSendFailure = vi.fn();
    const fetchFn: typeof fetch = async () => {
      calls += 1;
      throw new Error("connect ECONNREFUSED");
    };
    const { bot } = makeBot({ fetchFn, maxAttempts: 3, onSendFailure });
    const id = await bot.sendMessage(100, "lost");
    expect(id).toBeNull();
    expect(calls).toBe(3);
    expect(onSendFailure).toHaveBeenCalledWith("sendMessage");
  });

  it("does not retry permanent API errors", async () => {
    let calls = 0;
    const fetchFn: typeof fetch = async () => {
      calls += 1;
      return new Response(JSON.stringify({ ok: false, description: "Bad Request: text is empty" }), { status: 400 });
    };
    const { bot } = makeBot({ fetchFn });
    const id = await bot.sendMessage(100, "bad");
    expect(id).toBeNull();
    expect(calls).toBe(1);
  });

  it("redacts the token from network error strings", async () => {
    const fetchFn: typeof fetch = async (url) => {
      throw new Error(`getaddrinfo failed for ${String(url)}`);
    };
    const { bot } = makeBot({ fetchFn, maxAttempts: 2 });
    await expect(bot.getUpdates(undefined, 1)).rejects.toSatisfy(
      (error: Error) => !error.message.includes(TOKEN) && error.message.includes("***"),
    );
  });

  it("redacts the token from API error descriptions", async () => {
    const fetchFn: typeof fetch = async () => new Response(
      JSON.stringify({ ok: false, description: `Not Found: bot${TOKEN}/sendMessage` }),
      { status: 404 },
    );
    const { bot } = makeBot({ fetchFn });
    const ok = await bot.editMessageText(100, 5, "x");
    expect(ok).toBe(false);
  });

  it("editMessageText returns true on success and passes message_id", async () => {
    const bodies: unknown[] = [];
    const fetchFn: typeof fetch = async (_url, init) => {
      bodies.push(JSON.parse(String(init?.body)));
      return okResponse(true);
    };
    const { bot } = makeBot({ fetchFn });
    expect(await bot.editMessageText(100, 55, "edited")).toBe(true);
    expect(bodies[0]).toMatchObject({ chat_id: 100, message_id: 55, text: "edited", parse_mode: "HTML" });
  });

  it("deleteWebhook swallows errors", async () => {
    const fetchFn: typeof fetch = async () => {
      throw new Error("down");
    };
    const { bot } = makeBot({ fetchFn, maxAttempts: 1 });
    await expect(bot.deleteWebhook()).resolves.toBeUndefined();
  });
});
