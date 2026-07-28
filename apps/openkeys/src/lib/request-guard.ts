import "server-only";
import { NextResponse } from "next/server";
import { loadConfig } from "./config";

const MAX_JSON_BYTES = 16 * 1024;
const buckets = new Map<string, { count: number; resetAt: number }>();

function clientAddress(request: Request): string {
  const forwarded = request.headers.get("x-forwarded-for")?.split(",").map((part) => part.trim()).filter(Boolean);
  return forwarded?.at(-1) ?? request.headers.get("x-real-ip") ?? "unknown";
}

function sameOrigin(request: Request): boolean {
  const origin = request.headers.get("origin");
  if (!origin) return false;
  try {
    const actual = new URL(origin).origin;
    return actual === new URL(request.url).origin || actual === new URL(loadConfig().publicBaseUrl).origin;
  } catch {
    return false;
  }
}

function forbidden(): NextResponse {
  return NextResponse.json({ error: "forbidden" }, { status: 403 });
}

export function guardOrigin(request: Request): NextResponse | null {
  return sameOrigin(request) ? null : forbidden();
}

/** Single-instance protection; the edge proxy remains the outer volumetric limit. */
export function guardRequest(
  request: Request,
  scope: string,
  limit: number,
  windowMs: number,
): NextResponse | null {
  if (!sameOrigin(request)) {
    return forbidden();
  }

  const now = Date.now();
  const key = `${scope}:${clientAddress(request)}`;
  const current = buckets.get(key);
  if (!current || current.resetAt <= now) {
    buckets.set(key, { count: 1, resetAt: now + windowMs });
    return null;
  }
  if (current.count >= limit) {
    const retryAfter = Math.max(1, Math.ceil((current.resetAt - now) / 1000));
    return NextResponse.json(
      { error: "rate_limited" },
      { status: 429, headers: { "retry-after": String(retryAfter) } },
    );
  }
  current.count += 1;

  // Bound memory even if an attacker continually rotates addresses.
  if (buckets.size > 10_000) {
    for (const [bucketKey, value] of buckets) {
      if (value.resetAt <= now) buckets.delete(bucketKey);
    }
    if (buckets.size > 10_000) buckets.delete(buckets.keys().next().value as string);
  }
  return null;
}

/** Failed-login limiter. A valid credential clears stale failures instead of locking its owner out. */
export function guardLoginAttempt(request: Request, authenticated: boolean): NextResponse | null {
  const now = Date.now();
  const key = `admin-login:${clientAddress(request)}`;
  if (authenticated) {
    buckets.delete(key);
    return null;
  }

  const current = buckets.get(key);
  if (!current || current.resetAt <= now) {
    buckets.set(key, { count: 1, resetAt: now + 15 * 60_000 });
    return null;
  }
  current.count += 1;
  if (current.count <= 10) return null;

  const retryAfter = Math.max(1, Math.ceil((current.resetAt - now) / 1000));
  return NextResponse.json(
    { error: "rate_limited" },
    { status: 429, headers: { "retry-after": String(retryAfter) } },
  );
}

export async function readJsonLimited<T>(request: Request): Promise<T> {
  const declared = request.headers.get("content-length");
  if (declared && (!/^\d+$/.test(declared) || Number(declared) > MAX_JSON_BYTES)) {
    throw new Error("payload_too_large");
  }

  const reader = request.body?.getReader();
  if (!reader) throw new Error("invalid_body");
  const chunks: Uint8Array[] = [];
  let size = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > MAX_JSON_BYTES) {
      await reader.cancel();
      throw new Error("payload_too_large");
    }
    chunks.push(value);
  }
  const body = Buffer.concat(chunks).toString("utf8");
  return JSON.parse(body) as T;
}
