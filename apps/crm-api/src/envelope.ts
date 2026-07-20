import { z } from "zod";

/**
 * Конверт apitoken.crm/contact@v1 — контракт из CRM_PARSING_SPEC.md.
 * Жёсткий минимум: тег конверта + хотя бы один канал связи. Всё остальное — открытое
 * пространство (признаки придумывает AI парсера, реестр курирует AI CRM).
 */

export const ENVELOPE_TAG = "apitoken.crm/contact@v1";

export const channelSchema = z.object({
  type: z.string().min(1).max(40),
  value: z.string().min(1).max(500),
});

export const attributeSchema = z.object({
  value: z.unknown(),
  confidence: z.number().min(0).max(1).optional(),
  evidence: z.string().max(2_000).optional(),
});

export const envelopeSchema = z.object({
  envelope: z.literal(ENVELOPE_TAG),
  source: z
    .object({
      parser: z.string().max(200).optional(),
      run_id: z.string().max(200).optional(),
      parsed_at: z.string().max(64).optional(),
      origin: z.record(z.unknown()).optional(),
    })
    .optional(),
  identity: z.object({
    name: z.string().max(300).nullish(),
    channels: z.array(channelSchema).min(1).max(20),
  }),
  raw: z.record(z.unknown()).optional(),
  ai: z
    .object({
      model: z.string().max(100).optional(),
      classified_at: z.string().max(64).optional(),
      summary: z.string().max(4_000).optional(),
      attributes: z.record(attributeSchema).optional(),
      hypotheses: z.array(z.string().max(1_000)).max(50).optional(),
    })
    .optional(),
});

export type ContactEnvelope = z.infer<typeof envelopeSchema>;

export const ingestBodySchema = z.object({
  run: z.object({
    parser: z.string().min(1).max(200),
    run_id: z.string().min(1).max(200),
  }),
  contacts: z.array(z.unknown()).min(1).max(500),
});

export type IngestBody = z.infer<typeof ingestBodySchema>;
