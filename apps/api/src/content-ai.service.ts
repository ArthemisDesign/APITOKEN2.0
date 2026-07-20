import { Injectable, Logger, ServiceUnavailableException } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import type { ContentLocale, PlatformProfileRules } from "@claude-api/contracts";
import type { DraftDocument } from "@claude-api/db";
import { z } from "zod";
import type { Environment } from "./config.js";

const FACTUAL_SYSTEM = `You are the editorial engine for apiToken.sale Content Studio.
Use only facts present in the supplied source material and verified brief. Never invent a quote,
number, feature, test result, date, source, or personal experience. Clearly label analysis and
uncertainty. Preserve attribution. The original publisher is the source; apiToken.sale is the
analysis unless the supplied material says otherwise. Return only the requested JSON object.`;

const BRIEF_RESPONSE_SCHEMA = z.preprocess((value) => {
  if (!isRecord(value) || typeof value.briefMarkdown === "string") return value;
  const alias = [value.brief_markdown, value.brief].find((candidate) => typeof candidate === "string");
  return typeof alias === "string" ? { ...value, briefMarkdown: alias } : value;
}, z.object({ briefMarkdown: z.string().trim().min(1) }));

const DRAFT_RESPONSE_SCHEMA = z.object({
  title: z.string().trim().min(1),
  excerpt: z.string().optional().default(""),
  bodyMarkdown: z.string().trim().min(1),
});

const BRIEF_JSON_SCHEMA = {
  type: "object", additionalProperties: false, required: ["briefMarkdown"],
  properties: {
    briefMarkdown: {
      type: "string", minLength: 1,
      description: "The complete verification-first research brief formatted as Markdown.",
    },
  },
} as const;

const DRAFT_JSON_SCHEMA = {
  type: "object", additionalProperties: false, required: ["title", "excerpt", "bodyMarkdown"],
  properties: {
    title: { type: "string", minLength: 1 },
    excerpt: { type: "string" },
    bodyMarkdown: { type: "string", minLength: 1 },
  },
} as const;

type MessageBlock = {
  type: string;
  text?: string;
  name?: string;
  input?: unknown;
};

type MessagePayload = {
  content?: MessageBlock[];
  stop_reason?: string | null;
};

class StructuredResponseError extends Error {
  constructor(readonly reason: string, readonly blockTypes: string[]) {
    super(reason);
    this.name = "StructuredResponseError";
  }
}

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
    const result = await this.structured<{ briefMarkdown: string }>(`${FACTUAL_SYSTEM}
Create a verification-first research brief in ${languageName(input.locale)}. The Markdown brief must contain:
Summary, Confirmed facts, Claims needing verification, Reader impact, Original value we can add,
Recommended article angle, and Sources. Do not write the final article.
Return exactly one object with this shape: {"briefMarkdown":"the complete Markdown brief"}.`,
    JSON.stringify(input), BRIEF_JSON_SCHEMA, BRIEF_RESPONSE_SCHEMA, "brief");
    return result.briefMarkdown;
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
    if (input.profileKey === "blog") {
      document.bodyMarkdown = withoutCanonicalPlaceholder(document.bodyMarkdown);
      if (!document.bodyMarkdown) throw new ServiceUnavailableException("AI returned an invalid blog draft");
    } else if (!document.bodyMarkdown.includes("{{CANONICAL_URL}}")) {
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
    const result = await this.structured<DraftDocument>(system, JSON.stringify(user), DRAFT_JSON_SCHEMA,
      DRAFT_RESPONSE_SCHEMA, "draft");
    return {
      title: result.title.trim().slice(0, 300),
      excerpt: result.excerpt.trim().slice(0, 500),
      bodyMarkdown: result.bodyMarkdown.trim().slice(0, 150_000),
    };
  }

  private async structured<T>(
    system: string,
    user: string,
    inputSchema: Record<string, unknown>,
    schema: { parse(value: unknown): T },
    responseKind: "brief" | "draft",
  ): Promise<T> {
    for (let attempt = 1; attempt <= 2; attempt += 1) {
      const payload = await this.request(system, user, inputSchema, attempt > 1);
      try {
        return schema.parse(extractStructuredValue(payload));
      } catch (error) {
        if (!(error instanceof StructuredResponseError) && !(error instanceof z.ZodError)) throw error;
        const blockTypes = error instanceof StructuredResponseError
          ? error.blockTypes
          : (payload.content ?? []).map((block) => block.type);
        const reason = error instanceof StructuredResponseError ? error.reason : "schema validation failed";
        this.logger.warn(JSON.stringify({
          event: "content_ai_invalid_structure", responseKind, attempt, reason,
          blockTypes, stopReason: payload.stop_reason ?? null,
        }));
        if (attempt === 2) {
          throw new ServiceUnavailableException("AI response was not valid after an automatic retry");
        }
      }
    }
    throw new ServiceUnavailableException("AI response was not valid after an automatic retry");
  }

  private async request(
    system: string,
    user: string,
    inputSchema: Record<string, unknown>,
    correctiveRetry: boolean,
  ): Promise<MessagePayload> {
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
        system: correctiveRetry
          ? `${system}\nYour previous response did not match the required schema. Call return_content exactly once with valid structured fields.`
          : system,
        messages: [{ role: "user", content: user.slice(0, 180_000) }],
        tools: [{
          name: "return_content",
          description: "Return the completed Content Studio result in the required structure.",
          input_schema: inputSchema,
        }],
        tool_choice: { type: "tool", name: "return_content", disable_parallel_tool_use: true },
      }),
    });
    if (!response.ok) {
      const body = await response.text().catch(() => "");
      this.logger.error(`content AI returned ${response.status}: ${body.slice(0, 300)}`);
      throw new ServiceUnavailableException(`AI backend error (${response.status})`);
    }
    return await response.json() as MessagePayload;
  }
}

function extractStructuredValue(payload: MessagePayload): unknown {
  const blocks = payload.content ?? [];
  const toolBlock = blocks.find((block) => block.type === "tool_use"
    && block.name === "return_content" && isRecord(block.input));
  if (toolBlock) return toolBlock.input;

  const text = blocks.filter((block) => block.type === "text")
    .map((block) => block.text ?? "").join("\n").trim();
  if (!text) throw new StructuredResponseError("no structured content", blocks.map((block) => block.type));
  const parsed = parseJsonObject(text);
  if (parsed === undefined) {
    throw new StructuredResponseError("malformed JSON fallback", blocks.map((block) => block.type));
  }
  return parsed;
}

function parseJsonObject(text: string): unknown | undefined {
  const trimmed = text.trim().replace(/^```(?:json)?\s*/i, "").replace(/\s*```$/i, "");
  try {
    const parsed = JSON.parse(trimmed) as unknown;
    if (isRecord(parsed)) return parsed;
  } catch {
    // Scan below for the first balanced object when the model wrapped JSON in prose.
  }

  for (let start = trimmed.indexOf("{"); start >= 0; start = trimmed.indexOf("{", start + 1)) {
    let depth = 0;
    let inString = false;
    let escaped = false;
    for (let index = start; index < trimmed.length; index += 1) {
      const char = trimmed[index];
      if (inString) {
        if (escaped) escaped = false;
        else if (char === "\\") escaped = true;
        else if (char === "\"") inString = false;
        continue;
      }
      if (char === "\"") inString = true;
      else if (char === "{") depth += 1;
      else if (char === "}" && --depth === 0) {
        try {
          const parsed = JSON.parse(trimmed.slice(start, index + 1)) as unknown;
          if (isRecord(parsed)) return parsed;
        } catch {
          break;
        }
      }
    }
  }
  return undefined;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function withoutCanonicalPlaceholder(markdown: string): string {
  return markdown.split(/\r?\n/)
    .filter((line) => !line.includes("{{CANONICAL_URL}}"))
    .join("\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

function languageName(locale: ContentLocale): string {
  return locale === "ru" ? "Russian" : "English";
}
