import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { loadUsageByViewToken } from "@/lib/keys";
import { GET as stream } from "./route";

vi.mock("@/lib/keys", () => ({ loadUsageByViewToken: vi.fn() }));

const VIEW_TOKEN = "abcdefghijklmnopqrstuv";

function view(marker: string): Record<string, unknown> {
  return { viewToken: VIEW_TOKEN, keyMasked: "sk-pool-test…test", status: "active", marker };
}

function request(token: string): Request {
  return new Request(`https://openkeys.apitoken.sale/api/usage/stream?token=${encodeURIComponent(token)}`, {
    headers: { origin: "https://openkeys.apitoken.sale" },
  });
}

async function readFrame(reader: ReadableStreamDefaultReader<Uint8Array>): Promise<string> {
  const { value, done } = await reader.read();
  expect(done).toBe(false);
  return new TextDecoder().decode(value);
}

describe("OpenKeys usage SSE stream", () => {
  beforeEach(() => {
    vi.stubEnv("OPENKEYS_SESSION_SECRET", "0123456789abcdef0123456789abcdef");
    vi.mocked(loadUsageByViewToken).mockReset();
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    vi.useRealTimers();
  });

  it("rejects an invalid view token before opening any stream", async () => {
    const response = await stream(request("not-a-token"));
    expect(response.status).toBe(404);
    expect(loadUsageByViewToken).not.toHaveBeenCalled();
  });

  it("opens an SSE stream and pushes the current snapshot as the first frame", async () => {
    vi.mocked(loadUsageByViewToken).mockResolvedValue(view("initial") as never);

    const response = await stream(request(VIEW_TOKEN));
    expect(response.status).toBe(200);
    expect(response.headers.get("content-type")).toContain("text/event-stream");
    expect(response.headers.get("cache-control")).toContain("no-transform");

    const reader = response.body!.getReader();
    const first = await readFrame(reader);
    expect(first).toContain("data: ");
    expect(first).toContain('"marker":"initial"');
    await reader.cancel();
  });

  it("stays silent while the snapshot is unchanged and pushes the next real change", async () => {
    vi.useFakeTimers();
    vi.mocked(loadUsageByViewToken)
      .mockResolvedValueOnce(view("initial") as never)
      .mockResolvedValueOnce(view("initial") as never) // тик без изменений — кадра быть не должно
      .mockResolvedValue(view("changed") as never);

    const response = await stream(request(VIEW_TOKEN));
    const reader = response.body!.getReader();
    expect(await readFrame(reader)).toContain('"marker":"initial"');

    await vi.advanceTimersByTimeAsync(5_000); // идентичный снапшот — молчим
    await vi.advanceTimersByTimeAsync(5_000); // изменение — следующим же кадром

    const second = await readFrame(reader);
    expect(second).toContain('"marker":"changed"');
    expect(second).not.toContain('"marker":"initial"');
    await reader.cancel();
  });
});
