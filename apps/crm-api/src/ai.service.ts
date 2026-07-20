import { Inject, Injectable, Logger, ServiceUnavailableException } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { aiAudit } from "@claude-api/crm-db";
import type { Environment } from "./config.js";
import { CRM_DB, dbOf } from "./db.provider.js";

/**
 * Единственная точка вызова нейронки во всей CRM. Каждый вызов оставляет след в ai_audit —
 * прозрачность решений AI важнее экономии строк в БД. Модель ходит через НАШ движок
 * (CRM_ENGINE_URL/v1/messages) по ключу «CRM & Parsing»; ключ живёт только в env.
 */
@Injectable()
export class AiService {
  private readonly logger = new Logger(AiService.name);

  constructor(
    private readonly config: ConfigService<Environment, true>,
    @Inject(CRM_DB) private readonly holder: unknown,
  ) {}

  get enabled(): boolean {
    return Boolean(this.config.get("CRM_ENGINE_KEY", { infer: true }));
  }

  /** Свободный текстовый ответ модели. */
  async complete(kind: string, system: string, user: string): Promise<string> {
    const key = this.config.get("CRM_ENGINE_KEY", { infer: true });
    if (!key) throw new ServiceUnavailableException("AI is not configured (CRM_ENGINE_KEY)");
    const model = this.config.get("CRM_AI_MODEL", { infer: true });

    const response = await fetch(`${this.config.get("CRM_ENGINE_URL", { infer: true })}/v1/messages`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-api-key": key,
        "anthropic-version": "2023-06-01",
      },
      body: JSON.stringify({
        model,
        max_tokens: this.config.get("CRM_AI_MAX_TOKENS", { infer: true }),
        system,
        messages: [{ role: "user", content: user }],
      }),
    });
    if (!response.ok) {
      const body = await response.text().catch(() => "");
      this.logger.error(`engine /v1/messages ${response.status}: ${body.slice(0, 300)}`);
      throw new ServiceUnavailableException(`AI backend error (${response.status})`);
    }
    const payload = (await response.json()) as { content?: Array<{ type: string; text?: string }> };
    const text = (payload.content ?? [])
      .filter((b) => b.type === "text" && typeof b.text === "string")
      .map((b) => b.text)
      .join("\n");

    await dbOf(this.holder).db.insert(aiAudit).values({
      kind,
      model,
      inputSummary: user.slice(0, 2_000),
      output: { text: text.slice(0, 20_000) },
    });
    return text;
  }

  /** Ответ строго-JSON: вытаскиваем первый JSON-объект из текста модели. */
  async json<T>(kind: string, system: string, user: string): Promise<T> {
    const text = await this.complete(kind, system, user);
    const start = text.indexOf("{");
    const end = text.lastIndexOf("}");
    if (start < 0 || end <= start) throw new ServiceUnavailableException("AI returned no JSON object");
    try {
      return JSON.parse(text.slice(start, end + 1)) as T;
    } catch {
      throw new ServiceUnavailableException("AI returned malformed JSON");
    }
  }
}
