import { Inject, Injectable, Logger } from "@nestjs/common";
import { eq, sql } from "drizzle-orm";
import {
  attributeRegistry,
  contactAttributes,
  contactChannels,
  contacts,
  ingestRuns,
} from "@claude-api/crm-db";
import { AiService } from "./ai.service.js";
import { CRM_DB, dbOf } from "./db.provider.js";
import { envelopeSchema, ENVELOPE_TAG, type ContactEnvelope } from "./envelope.js";

export interface IngestResult {
  received: number;
  accepted: number;
  repaired: number;
  rejected: number;
  drift: Array<{ index: number; error: string; repaired: boolean }>;
}

const REPAIR_SYSTEM = `Ты — адаптер форматов AI-CRM apitoken.sale. Тебе дают объект-контакт,
который НЕ прошёл валидацию конверта "${ENVELOPE_TAG}", и текст ошибки. Преобразуй данные в
валидный конверт, ничего не выдумывая: имя и каналы связи (telegram/gmail/email/phone/...) бери
только из входных данных; всё непонятное складывай в raw. Если признаки (attributes) угадываются
из данных — перенеси их в ai.attributes с confidence и evidence.
ТОЧНАЯ форма конверта (следуй ей буквально, особенно identity.channels — массив объектов):
{"envelope":"${ENVELOPE_TAG}",
 "identity":{"name":"...|null","channels":[{"type":"telegram","value":"@handle"}]},
 "raw":{...},
 "ai":{"summary":"...","attributes":{"key":{"value":...,"confidence":0.5,"evidence":"..."}},"hypotheses":[]}}
Верни СТРОГО один JSON-объект конверта без пояснений и без markdown-ограждений.
Если в данных нет ни одного контакта-канала — верни {"unrepairable": true}.`;

/**
 * Ингест: жёсткая проверка минимума → AI-адаптер для «не влезло» → слияние по каналам →
 * открытые признаки → автопополнение attribute_registry. Идемпотентно: повтор батча
 * не плодит дубликаты (upsert по (type,value) и (contact_id,key)).
 */
@Injectable()
export class IngestService {
  private readonly logger = new Logger(IngestService.name);

  constructor(
    @Inject(CRM_DB) private readonly holder: unknown,
    private readonly ai: AiService,
  ) {}

  async ingestBatch(parser: string, runId: string, items: unknown[]): Promise<IngestResult> {
    const result: IngestResult = { received: items.length, accepted: 0, repaired: 0, rejected: 0, drift: [] };

    for (const [index, item] of items.entries()) {
      const parsed = envelopeSchema.safeParse(item);
      if (parsed.success) {
        await this.storeContact(parsed.data, parser, runId);
        result.accepted += 1;
        continue;
      }

      // Формат отклонился от спеки → решение принимает нейронка, не хардкод: AI-адаптер
      // пытается смапить чужой формат в конверт; сам факт отклонения — в drift-лог.
      const error = parsed.error.issues.map((i) => `${i.path.join(".")}: ${i.message}`).join("; ");
      let repaired = false;
      if (this.ai.enabled) {
        try {
          const candidate = await this.ai.json<Record<string, unknown>>(
            "ingest_repair",
            REPAIR_SYSTEM,
            JSON.stringify({ error, item }).slice(0, 30_000),
          );
          const reparsed = envelopeSchema.safeParse(candidate);
          if (reparsed.success && candidate.unrepairable !== true) {
            await this.storeContact(reparsed.data, parser, runId);
            result.repaired += 1;
            repaired = true;
          }
        } catch (repairError) {
          this.logger.warn(`ingest repair failed at #${index}: ${String(repairError)}`);
        }
      }
      if (!repaired) result.rejected += 1;
      result.drift.push({ index, error: error.slice(0, 500), repaired });
    }

    await dbOf(this.holder).db.insert(ingestRuns).values({
      runId,
      parser,
      received: result.received,
      accepted: result.accepted,
      repaired: result.repaired,
      rejected: result.rejected,
      drift: result.drift,
    });
    return result;
  }

  /** Слияние: контакт един для всех своих каналов; признаки — upsert, свежее побеждает. */
  private async storeContact(envelope: ContactEnvelope, parser: string, runId: string): Promise<void> {
    const { db } = dbOf(this.holder);
    const channels = envelope.identity.channels.map((c) => ({
      type: c.type.trim().toLowerCase(),
      value: c.value.trim(),
    }));

    await db.transaction(async (tx) => {
      // Существующий контакт ищем по любому из каналов.
      let contactId: string | null = null;
      for (const channel of channels) {
        const found = await tx
          .select({ contactId: contactChannels.contactId })
          .from(contactChannels)
          .where(sql`${contactChannels.type} = ${channel.type} AND ${contactChannels.value} = ${channel.value}`)
          .limit(1);
        if (found.length > 0) {
          contactId = found[0]!.contactId;
          break;
        }
      }

      const patch = {
        name: envelope.identity.name ?? undefined,
        summary: envelope.ai?.summary,
        hypotheses: envelope.ai?.hypotheses ?? [],
        raw: envelope.raw ?? {},
        lastParser: parser,
        lastRunId: runId,
        updatedAt: new Date(),
      };
      if (contactId === null) {
        const inserted = await tx
          .insert(contacts)
          .values({ ...patch, status: envelope.ai?.attributes ? "enriched" : "new" })
          .returning({ id: contacts.id });
        contactId = inserted[0]!.id;
      } else {
        await tx.update(contacts).set(patch).where(eq(contacts.id, contactId));
      }

      for (const channel of channels) {
        await tx
          .insert(contactChannels)
          .values({ contactId, ...channel })
          .onConflictDoNothing();
      }

      const attributes = envelope.ai?.attributes ?? {};
      const source = `${parser}/${runId}`;
      for (const [rawKey, attribute] of Object.entries(attributes)) {
        const key = rawKey.trim().toLowerCase();
        if (!/^[a-z][a-z0-9_]{0,79}$/.test(key)) continue;
        await tx
          .insert(contactAttributes)
          .values({
            contactId,
            key,
            value: attribute.value ?? null,
            confidence: attribute.confidence,
            evidence: attribute.evidence,
            source,
            updatedAt: new Date(),
          })
          .onConflictDoUpdate({
            target: [contactAttributes.contactId, contactAttributes.key],
            set: {
              value: attribute.value ?? null,
              confidence: attribute.confidence,
              evidence: attribute.evidence,
              source,
              updatedAt: new Date(),
            },
          });

        // Новые ключи сами встают в реестр (proposed) — описания/мерж добавит AI-куратор.
        await tx
          .insert(attributeRegistry)
          .values({ key, seenCount: 1, examples: [attribute.value ?? null] })
          .onConflictDoUpdate({
            target: attributeRegistry.key,
            set: { seenCount: sql`${attributeRegistry.seenCount} + 1`, updatedAt: new Date() },
          });
      }
    });
  }
}
