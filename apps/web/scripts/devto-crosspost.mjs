#!/usr/bin/env node
// Cross-posts learn-cluster articles to dev.to with canonical_url pointing at
// the original, so search weight flows to apitoken.sale (no duplicate-content
// penalty). Content is taken from the live markdown gateway /md/docs/learn/*,
// i.e. exactly what production serves.
//
// Usage:
//   DEVTO_API_KEY=... node scripts/devto-crosspost.mjs <slug> [<slug>...] [--dry-run]
//   (key also read from ~/.config/apitoken/devto.env)
//
// dev.to throttles article creation, so consecutive posts are spaced ~35s
// apart. Keep the cadence low overall: 2-3 articles per week, not the whole
// cluster at once.

import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const ORIGIN = "https://apitoken.sale";
const API = "https://dev.to/api/articles";
const POST_GAP_MS = 35_000;

const TAGS = {
  "cheapest-claude-api": ["ai", "claude", "api", "llm"],
  "apitoken-vs-openrouter": ["ai", "claude", "api", "openrouter"],
  "claude-code-without-subscription": ["ai", "claude", "api", "cli"],
};
const DEFAULT_TAGS = ["ai", "claude", "api", "llm"];

function apiKey() {
  if (process.env.DEVTO_API_KEY) return process.env.DEVTO_API_KEY;
  try {
    const env = readFileSync(join(homedir(), ".config/apitoken/devto.env"), "utf8");
    const match = env.match(/^DEVTO_API_KEY=(.+)$/m);
    if (match) return match[1].trim();
  } catch {}
  console.error("DEVTO_API_KEY not set and ~/.config/apitoken/devto.env not readable");
  process.exit(1);
}

async function fetchArticle(slug) {
  const res = await fetch(`${ORIGIN}/md/docs/learn/${slug}`);
  if (!res.ok) throw new Error(`GET /md/docs/learn/${slug} -> ${res.status}`);
  return res.text();
}

function parseGatewayMarkdown(md, slug) {
  const fm = md.match(/^---\n([\s\S]*?)\n---\n/);
  if (!fm) throw new Error(`${slug}: no front matter in gateway markdown`);
  const meta = {};
  for (const line of fm[1].split("\n")) {
    const idx = line.indexOf(": ");
    if (idx > 0) meta[line.slice(0, idx)] = line.slice(idx + 2);
  }
  const title = meta.title;
  const description = meta.description ? JSON.parse(meta.description) : "";
  const url = meta.url ?? `${ORIGIN}/docs/learn/${slug}`;

  let body = md.slice(fm[0].length).trimStart();
  // dev.to renders the title itself; drop the duplicate H1.
  body = body.replace(/^# .+\n+/, "");
  // Absolutize site-relative links so they work off-site.
  body = body.replace(/\]\(\//g, `](${ORIGIN}/`);
  body += `\n*Originally published at [apitoken.sale](${url}).*\n`;
  return { title, description, url, body };
}

async function publish(key, slug, dryRun) {
  const { title, description, url, body } = parseGatewayMarkdown(await fetchArticle(slug), slug);
  const article = {
    title,
    body_markdown: body,
    published: true,
    canonical_url: url,
    description,
    tags: TAGS[slug] ?? DEFAULT_TAGS,
  };
  if (dryRun) {
    console.log(`--- ${slug} (dry run) ---`);
    console.log(JSON.stringify({ ...article, body_markdown: `${body.slice(0, 400)}…` }, null, 2));
    return null;
  }
  let res, payload;
  for (let attempt = 0; ; attempt++) {
    res = await fetch(API, {
      method: "POST",
      headers: {
        "api-key": key,
        "content-type": "application/json",
        accept: "application/vnd.forem.api-v1+json",
      },
      body: JSON.stringify({ article }),
    });
    payload = await res.json().catch(() => ({}));
    if (res.status === 429 && attempt < 2) {
      const wait = (Number(payload.error?.match(/(\d+) seconds/)?.[1]) || 300) + 15;
      console.log(`${slug}: rate limited, retrying in ${wait}s`);
      await new Promise((r) => setTimeout(r, wait * 1000));
      continue;
    }
    break;
  }
  if (!res.ok) throw new Error(`${slug}: dev.to ${res.status} ${JSON.stringify(payload)}`);
  console.log(`${slug} -> ${payload.url}`);
  return payload.url;
}

const args = process.argv.slice(2);
const dryRun = args.includes("--dry-run");
const slugs = args.filter((a) => !a.startsWith("--"));
if (slugs.length === 0) {
  console.error("usage: devto-crosspost.mjs <slug> [<slug>...] [--dry-run]");
  process.exit(1);
}

const key = apiKey();
for (let i = 0; i < slugs.length; i++) {
  if (i > 0 && !dryRun) await new Promise((r) => setTimeout(r, POST_GAP_MS));
  await publish(key, slugs[i], dryRun);
}
