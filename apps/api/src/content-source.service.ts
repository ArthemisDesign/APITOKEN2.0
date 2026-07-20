import { lookup } from "node:dns/promises";
import { isIP } from "node:net";
import { BadRequestException, Injectable, ServiceUnavailableException } from "@nestjs/common";
import type { ContentLocale } from "@claude-api/contracts";
import type { ExtractedContentSource } from "@claude-api/db";

const MAX_SOURCE_BYTES = 2_000_000;

@Injectable()
export class ContentSourceService {
  async extract(input: { sourceUrl: string; locale: ContentLocale; sourceContent?: string }): Promise<ExtractedContentSource> {
    const url = normalizeSourceUrl(input.sourceUrl);
    const platform = detectSourcePlatform(url);
    if (input.sourceContent) {
      return {
        sourceUrl: url.toString(), sourcePlatform: platform, sourceTitle: url.hostname,
        sourceAuthor: null, sourceContent: input.sourceContent, sourcePublishedAt: null,
        sourceSnapshot: { method: "manual", capturedAt: new Date().toISOString() }, primaryLocale: input.locale,
      };
    }

    const extracted = platform === "x"
      ? await this.extractOembed("https://publish.x.com/oembed", url)
      : platform === "reddit"
        ? await this.extractOembed("https://www.reddit.com/oembed", url)
        : await this.extractWebPage(url);

    if (!extracted.content.trim()) {
      throw new BadRequestException("The source did not expose readable text. Paste the post text manually.");
    }
    return {
      sourceUrl: url.toString(), sourcePlatform: platform,
      sourceTitle: extracted.title || url.hostname, sourceAuthor: extracted.author,
      sourceContent: extracted.content.slice(0, 100_000), sourcePublishedAt: extracted.publishedAt,
      sourceSnapshot: { method: extracted.method, capturedAt: new Date().toISOString() }, primaryLocale: input.locale,
    };
  }

  private async extractOembed(endpoint: string, source: URL): Promise<ExtractedPage> {
    const request = new URL(endpoint);
    request.searchParams.set("url", source.toString());
    request.searchParams.set("omit_script", "true");
    const response = await fetch(request, { signal: AbortSignal.timeout(12_000), headers: { accept: "application/json" } });
    if (!response.ok) throw new ServiceUnavailableException(`Source platform returned ${response.status}`);
    const payload = await response.json() as { title?: string; author_name?: string; html?: string };
    return {
      title: payload.title ?? "", author: payload.author_name ?? null,
      content: stripHtml(payload.html ?? ""), publishedAt: null, method: "oembed",
    };
  }

  private async extractWebPage(url: URL): Promise<ExtractedPage> {
    let current = url;
    for (let redirect = 0; redirect <= 3; redirect += 1) {
      await assertPublicHttpUrl(current);
      const response = await fetch(current, {
        redirect: "manual",
        signal: AbortSignal.timeout(15_000),
        headers: { accept: "text/html,application/xhtml+xml", "user-agent": "apiToken.sale Content Studio/1.0" },
      });
      if ([301, 302, 303, 307, 308].includes(response.status)) {
        const location = response.headers.get("location");
        if (!location) throw new BadRequestException("Source redirect has no destination");
        current = new URL(location, current);
        continue;
      }
      if (!response.ok) throw new ServiceUnavailableException(`Source returned ${response.status}`);
      const contentLength = Number(response.headers.get("content-length") ?? "0");
      if (contentLength > MAX_SOURCE_BYTES) throw new BadRequestException("Source page is too large");
      const html = await response.text();
      if (Buffer.byteLength(html) > MAX_SOURCE_BYTES) throw new BadRequestException("Source page is too large");
      return extractHtmlMetadata(html);
    }
    throw new BadRequestException("Source redirected too many times");
  }
}

export interface ExtractedPage {
  title: string;
  author: string | null;
  content: string;
  publishedAt: Date | null;
  method: "oembed" | "html";
}

export function normalizeSourceUrl(value: string): URL {
  let url: URL;
  try { url = new URL(value); } catch { throw new BadRequestException("Source URL is invalid"); }
  if (!["http:", "https:"].includes(url.protocol) || url.username || url.password) {
    throw new BadRequestException("Source URL must be a public HTTP or HTTPS URL without credentials");
  }
  url.hash = "";
  return url;
}

export function detectSourcePlatform(url: URL): string {
  const host = url.hostname.toLowerCase().replace(/^www\./, "");
  if (host === "x.com" || host === "twitter.com") return "x";
  if (host === "reddit.com" || host.endsWith(".reddit.com") || host === "redd.it") return "reddit";
  if (host === "vc.ru" || host.endsWith(".vc.ru")) return "vc-ru";
  if (host === "dzen.ru" || host.endsWith(".dzen.ru")) return "dzen";
  if (host === "habr.com" || host.endsWith(".habr.com")) return "habr";
  if (host === "medium.com" || host.endsWith(".medium.com")) return "medium";
  if (host === "t.me" || host === "telegram.me") return "telegram";
  if (host === "linkedin.com" || host.endsWith(".linkedin.com")) return "linkedin";
  return "web";
}

export function extractHtmlMetadata(html: string): ExtractedPage {
  const meta = (names: string[]): string => {
    for (const name of names) {
      const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      const patterns = [
        new RegExp(`<meta[^>]+(?:name|property)=["']${escaped}["'][^>]+content=["']([^"']*)["'][^>]*>`, "i"),
        new RegExp(`<meta[^>]+content=["']([^"']*)["'][^>]+(?:name|property)=["']${escaped}["'][^>]*>`, "i"),
      ];
      for (const pattern of patterns) {
        const match = html.match(pattern);
        if (match?.[1]) return decodeHtml(match[1]).trim();
      }
    }
    return "";
  };
  const titleTag = html.match(/<title[^>]*>([\s\S]*?)<\/title>/i)?.[1] ?? "";
  const article = html.match(/<article[^>]*>([\s\S]*?)<\/article>/i)?.[1]
    ?? html.match(/<main[^>]*>([\s\S]*?)<\/main>/i)?.[1]
    ?? html.match(/<body[^>]*>([\s\S]*?)<\/body>/i)?.[1]
    ?? html;
  const dateText = meta(["article:published_time", "datePublished"]);
  const parsedDate = dateText ? new Date(dateText) : null;
  return {
    title: meta(["og:title", "twitter:title"]) || stripHtml(titleTag),
    author: meta(["author", "article:author"]) || null,
    content: stripHtml(article),
    publishedAt: parsedDate && !Number.isNaN(parsedDate.getTime()) ? parsedDate : null,
    method: "html",
  };
}

export function stripHtml(value: string): string {
  return decodeHtml(value
    .replace(/<(script|style|noscript|svg)[^>]*>[\s\S]*?<\/\1>/gi, " ")
    .replace(/<br\s*\/?>/gi, "\n")
    .replace(/<\/(p|div|li|h[1-6]|blockquote)>/gi, "\n")
    .replace(/<[^>]+>/g, " "))
    .replace(/[ \t]+/g, " ")
    .replace(/[ \t]*\n[ \t]*/g, "\n")
    .replace(/\n\s*\n+/g, "\n\n")
    .trim();
}

function decodeHtml(value: string): string {
  return value.replace(/&nbsp;/gi, " ").replace(/&amp;/gi, "&").replace(/&lt;/gi, "<")
    .replace(/&gt;/gi, ">").replace(/&quot;/gi, "\"").replace(/&#39;|&apos;/gi, "'")
    .replace(/&#(\d+);/g, (_, code: string) => String.fromCodePoint(Number(code)));
}

async function assertPublicHttpUrl(url: URL): Promise<void> {
  normalizeSourceUrl(url.toString());
  const host = url.hostname.toLowerCase().replace(/^\[|\]$/g, "");
  if (host === "localhost" || host.endsWith(".local") || host.endsWith(".internal")) {
    throw new BadRequestException("Private network sources are not allowed");
  }
  const addresses = isIP(host) ? [{ address: host }] : await lookup(host, { all: true, verbatim: true });
  if (addresses.length === 0 || addresses.some(({ address }) => isPrivateAddress(address))) {
    throw new BadRequestException("Private network sources are not allowed");
  }
}

function isPrivateAddress(address: string): boolean {
  const value = address.toLowerCase();
  if (value === "::1" || value === "::" || value.startsWith("fe80:") || value.startsWith("fc") || value.startsWith("fd")) return true;
  const mapped = value.startsWith("::ffff:") ? value.slice(7) : value;
  const octets = mapped.split(".").map(Number);
  if (octets.length !== 4 || octets.some(Number.isNaN)) return false;
  const [a, b] = octets;
  return a === 0 || a === 10 || a === 127 || (a === 169 && b === 254)
    || (a === 172 && b! >= 16 && b! <= 31) || (a === 192 && b === 168) || a! >= 224;
}
