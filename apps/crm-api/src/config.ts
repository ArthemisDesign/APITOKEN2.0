import { z } from "zod";

const environmentSchema = z.object({
  NODE_ENV: z.enum(["development", "test", "production"]).default("development"),
  PORT: z.coerce.number().int().min(1).max(65_535).default(3400),
  HOST: z.string().default("127.0.0.1"),
  API_DRAIN_DEADLINE_MS: z.coerce.number().int().min(1_000).max(29_000).default(25_000),
  CRM_DATABASE_URL: z.string().url(),
  // Ключ для парсеров (заголовок x-crm-ingest-key). Отдельный от учёток людей.
  CRM_INGEST_KEY: z.string().min(32),
  // AI через НАШ движок: обычный Anthropic-совместимый /v1/messages по ключу «CRM & Parsing».
  CRM_ENGINE_URL: z.string().url().default("https://api.apitoken.sale"),
  CRM_ENGINE_KEY: z.preprocess((v) => (v === "" ? undefined : v), z.string().min(20).optional()),
  CRM_AI_MODEL: z.string().default("claude-sonnet-5"),
  CRM_AI_MAX_TOKENS: z.coerce.number().int().min(256).max(64_000).default(8_192),
}).superRefine((value, context) => {
  if (value.NODE_ENV === "production" && !value.CRM_ENGINE_KEY) {
    context.addIssue({ code: "custom", message: "CRM_ENGINE_KEY is required in production (AI-first CRM)" });
  }
});

export type Environment = z.infer<typeof environmentSchema>;

export function validateEnvironment(value: Record<string, unknown>): Environment {
  return environmentSchema.parse(value);
}
