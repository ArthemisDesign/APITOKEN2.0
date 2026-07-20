import { Injectable, Logger, ServiceUnavailableException } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import type { ContentLocale, PlatformProfileRules } from "@claude-api/contracts";
import type { DraftDocument } from "@claude-api/db";
import type { Environment } from "./config.js";

const FACTUAL_SYSTEM = `You are the editorial engine for apiToken.sale Content Studio.
Use only facts present in the supplied source material and verified brief. Never invent a quote,
number, feature, test result, date, source, or personal experience. Clearly label analysis and
uncertainty. Preserve attribution. The original publisher is the source; apiToken.sale is the
analysis unless the supplied material says otherwise. Return only the requested JSON object.`;

@Injectable()
export class ContentAiService {
  private readonly logger = new Logger(ContentAiService.name);

  constructor(private readonly config: ConfigService<Environment, true>) {}

  get enabled(): boolean {
    return Boolean(this.config.get("CONTENT_STUDIO_ENGINE_KEY", { infer: true }));
  }

  async generateBrief(input: {
    sourceUrl: string;
    title: string;
    author: string | null;
    content: string;
    references: Array<{ url: string; title: string; notes: string }>;
    locale: ContentLocale;
  }): Promise<string> {
    const result = await this.json<{ briefMarkdown: string }>(`${FACTUAL_SYSTEM}
Create a verification-first research brief in ${languageName(input.locale)}. The Markdown brief must contain:
Summary, Confirmed facts, Claims needing verification, Reader impact, Original value we can add,
Recommended article angle, and Sources. Do not write the final article.`, JSON.stringify(input));
    if (!result.briefMarkdown?.trim()) throw new ServiceUnavailableException("AI returned an empty brief");
    return result.briefMarkdown.trim();
  }

  async generateDraft(input: {
    brief: string;
    sourceUrl: string;
    profileKey: string;
    profileName: string;
    rules: PlatformProfileRules;
    locale: ContentLocale;
  }): Promise<DraftDocument> {
    const document = await this.document(`${FACTUAL_SYSTEM}
Write one original ${input.profileName} draft in ${languageName(input.locale)}.
Platform rules: ${JSON.stringify(input.rules)}
The response must be {"title":"...","excerpt":"...","bodyMarkdown":"..."}.
The body must be useful without clicking a link. For the blog version, include a visible Sources
section. For every external version, include {{CANONICAL_URL}} exactly once as the link to the full
apiToken.sale analysis; it is not the original source.`, {
      sourceUrl: input.sourceUrl, verifiedBrief: input.brief, platform: input.profileKey,
    });
    if (input.profileKey !== "blog" && !document.bodyMarkdown.includes("{{CANONICAL_URL}}")) {
      document.bodyMarkdown = `${document.bodyMarkdown}\n\nFull analysis: {{CANONICAL_URL}}`;
    }
    return document;
  }

  async reviseDraft(input: {
    document: DraftDocument;
    instruction: string;
    brief: string;
    profileName: string;
    rules: PlatformProfileRules;
    locale: ContentLocale;
  }): Promise<DraftDocument> {
    return this.document(`${FACTUAL_SYSTEM}
Revise the supplied ${input.profileName} draft in ${languageName(input.locale)} according to the
editor instruction. Do not change factual meaning or numeric values unless the verified brief
supports the change. Platform rules: ${JSON.stringify(input.rules)}
Return {"title":"...","excerpt":"...","bodyMarkdown":"..."}.`, {
      verifiedBrief: input.brief, editorInstruction: input.instruction, currentDraft: input.document,
    });
  }

  private async document(system: string, user: unknown): Promise<DraftDocument> {
    const result = await this.json<Partial<DraftDocument>>(system, JSON.stringify(user));
    if (!result.title?.trim() || !result.bodyMarkdown?.trim()) {
      throw new ServiceUnavailableException("AI returned an incomplete draft");
    }
    return {
      title: result.title.trim().slice(0, 300),
      excerpt: (result.excerpt ?? "").trim().slice(0, 500),
      bodyMarkdown: result.bodyMarkdown.trim().slice(0, 150_000),
    };
  }

  private async json<T>(system: string, user: string): Promise<T> {
    const key = this.config.get("CONTENT_STUDIO_ENGINE_KEY", { infer: true });
    if (!key) throw new ServiceUnavailableException("AI is not configured for Content Studio");
    const endpoint = new URL("/v1/messages", this.config.get("CONTENT_STUDIO_ENGINE_URL", { infer: true }));
    const response = await fetch(endpoint, {
      method: "POST",
      signal: AbortSignal.timeout(90_000),
      headers: { "content-type": "application/json", "x-api-key": key, "anthropic-version": "2023-06-01" },
      body: JSON.stringify({
        model: this.config.get("CONTENT_STUDIO_AI_MODEL", { infer: true }),
        max_tokens: this.config.get("CONTENT_STUDIO_AI_MAX_TOKENS", { infer: true }),
        system,
        messages: [{ role: "user", content: user.slice(0, 180_000) }],
      }),
    });
    if (!response.ok) {
      const body = await response.text().catch(() => "");
      this.logger.error(`content AI returned ${response.status}: ${body.slice(0, 300)}`);
      throw new ServiceUnavailableException(`AI backend error (${response.status})`);
    }
    const payload = await response.json() as { content?: Array<{ type: string; text?: string }> };
    const text = (payload.content ?? []).filter((block) => block.type === "text")
      .map((block) => block.text ?? "").join("\n");
    const start = text.indexOf("{");
    const end = text.lastIndexOf("}");
    if (start < 0 || end <= start) throw new ServiceUnavailableException("AI returned no JSON object");
    try { return JSON.parse(text.slice(start, end + 1)) as T; }
    catch { throw new ServiceUnavailableException("AI returned malformed JSON"); }
  }
}

function languageName(locale: ContentLocale): string {
  return locale === "ru" ? "Russian" : "English";
}
