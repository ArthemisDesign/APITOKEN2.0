export const FEEDS = [
  "/admin/events",
  "/partner-admin/events",
  "/openkeys-admin/events",
  "/proxy-admin/events",
  "/events/engine",
  "/events/openai",
  "/events/gemini",
  "/events/kimi",
] as const;

export type FeedPath = (typeof FEEDS)[number];

const COMMERCE: FeedPath = "/admin/events";
const PARTNER: FeedPath = "/partner-admin/events";
const OPENKEYS: FeedPath = "/openkeys-admin/events";
const PROXY: FeedPath = "/proxy-admin/events";
const ENGINE: FeedPath = "/events/engine";
const OPENAI: FeedPath = "/events/openai";
const GEMINI: FeedPath = "/events/gemini";
const KIMI: FeedPath = "/events/kimi";

const FAILSAFE: readonly FeedPath[] = [COMMERCE, ENGINE];

function uniqueFeeds(feeds: readonly FeedPath[]): FeedPath[] {
  return [...new Set(feeds)];
}

/** Open only the SSE sources the current screen's URLs can invalidate. */
export function feedsForPath(pathname: string): readonly FeedPath[] {
  const path = (pathname.split("?")[0] ?? pathname).replace(/\/+$/, "") || "/";
  if (path === "/partners" || path.startsWith("/partners/")) {
    // Commerce owns the email-linked admin projection; Sales still emits request and payout events.
    return uniqueFeeds([COMMERCE, PARTNER]);
  }
  switch (path) {
    case "/":
      return uniqueFeeds([COMMERCE, PARTNER, ENGINE]);
    case "/subscriptions":
      return uniqueFeeds([ENGINE, OPENAI, GEMINI, KIMI]);
    case "/proxies":
      return uniqueFeeds([PROXY]);
    case "/system":
      return uniqueFeeds([ENGINE, OPENKEYS]);
    case "/trends":
      return uniqueFeeds([ENGINE]);
    case "/users":
      return uniqueFeeds([COMMERCE, ENGINE]);
    case "/paying-users":
      return uniqueFeeds([COMMERCE, OPENKEYS]);
    case "/accounts":
      return uniqueFeeds([ENGINE, COMMERCE, PARTNER, OPENKEYS]);
    case "/openkeys":
      return uniqueFeeds([OPENKEYS]);
    case "/business":
      return uniqueFeeds([COMMERCE]);
    case "/sales/calculator":
      return uniqueFeeds([ENGINE, OPENAI, GEMINI]);
    case "/topups":
      return uniqueFeeds([COMMERCE]);
    case "/engine-spend":
      return uniqueFeeds([COMMERCE, OPENKEYS]);
    case "/request-analytics":
      return uniqueFeeds([COMMERCE]);
    case "/finance":
      return uniqueFeeds([COMMERCE, ENGINE]);
    case "/admins":
    case "/audit":
      return uniqueFeeds([COMMERCE]);
    default:
      return uniqueFeeds(FAILSAFE);
  }
}
