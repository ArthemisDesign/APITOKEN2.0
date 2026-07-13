import { z } from "zod";

const environmentSchema = z.object({
  NODE_ENV: z.enum(["development", "test", "production"]).default("development"),
  DATABASE_URL: z.string().url(),
  ENGINE_BASE_URL: z.string().url(),
  ENGINE_CONTROL_KEY: z.string().min(32),
  ENGINE_TIMEOUT_MS: z.coerce.number().int().positive().default(10_000),
  CREDIT_POLL_MS: z.coerce.number().int().min(100).default(1000),
});

export type Environment = z.infer<typeof environmentSchema>;
export function validateEnvironment(value: Record<string, unknown>): Environment {
  return environmentSchema.parse(value);
}
