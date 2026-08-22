// Compact / paged operator URLs for engine panel routes. Extra query params are
// ignored by older engines; defaults without the params keep the historical payload.

export const COMPACT_RECENT_TURNS = "recent_turns=0";
export const ENGINE_ACCOUNT_PAGE = 50;

export function compactCapacityUrl(): string {
  return `/capacity?${COMPACT_RECENT_TURNS}`;
}

export function compactCodexSubsUrl(): string {
  return `/codex-subs?${COMPACT_RECENT_TURNS}`;
}

export function compactGeminiSubsUrl(): string {
  return `/gemini-subs?${COMPACT_RECENT_TURNS}`;
}

export function compactKimiSubsUrl(): string {
  return `/kimi-subs?${COMPACT_RECENT_TURNS}`;
}

export function compactGlmSubsUrl(): string {
  return `/glm-subs?${COMPACT_RECENT_TURNS}`;
}

export function compactOverviewUrl(): string {
  return "/overview?accounts_limit=0";
}

export function pagedOverviewUrl(offset: number, limit = ENGINE_ACCOUNT_PAGE): string {
  return `/overview?accounts_limit=${limit}&accounts_offset=${offset}`;
}

export function clampPageOffset(offset: number, total: number, pageSize: number): number {
  if (total <= 0 || offset < total) return offset;
  return Math.max(0, Math.floor((total - 1) / pageSize) * pageSize);
}
