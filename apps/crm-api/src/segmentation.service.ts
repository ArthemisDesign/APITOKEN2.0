import { Inject, Injectable, Logger } from "@nestjs/common";
import { desc, eq, ne, sql } from "drizzle-orm";
import {
  attributeRegistry,
  contactAttributes,
  matchesFilter,
  parseFilter,
  smartViews,
  type AttributeMap,
  type FilterDsl,
} from "@claude-api/crm-db";
import { AiService } from "./ai.service.js";
import { CRM_DB, dbOf } from "./db.provider.js";

const DSL_HELP = `Фильтр-DSL (единственный исполняемый формат):
{"all":[{"key":"...","op":"eq|neq|in|contains|exists|gte|lte|regex","value":...}],
 "any":[...], "none":[...]}
"all" — все условия, "any" — хотя бы одно, "none" — ни одного. Признаки-массивы матчатся поэлементно.`;

const SEGMENT_SYSTEM = `Ты — AI-сегментатор CRM apitoken.sale (продажа доступа к Claude API со скидкой).
Тебе дают: реестр признаков контактов (ключ, описание, сколько раз встречался, примеры значений)
и текущие сегменты. САМ придумай полезные для продаж сегменты (smart views): кого стоит выделить
в отдельную группу и почему. Обновляй устаревшие: если сегмент уже есть и хорош — верни его без
изменений (по title). ${DSL_HELP}
Верни СТРОГО JSON: {"views":[{"title":"...","description":"...","rationale":"почему сегмент полезен",
"filter":{...}}]} — от 3 до 12 сегментов, только по реально существующим ключам признаков.`;

const ASK_SYSTEM = `Ты — поисковик по AI-CRM apitoken.sale. Человек описывает, КТО ему нужен.
Тебе дают реестр признаков (ключи, описания, примеры). Переведи описание в фильтр. ${DSL_HELP}
Верни СТРОГО JSON: {"filter":{...},"explanation":"как понял запрос и почему такой фильтр"}.`;

const CURATE_SYSTEM = `Ты — куратор реестра признаков AI-CRM. Тебе дают список ключей признаков
с примерами значений и счётчиками. Твоя работа: (1) короткое описание каждому ключу без описания,
(2) указать тип значения (string|number|boolean|string[]|score01), (3) найти синонимы —
пары ключей об одном и том же (например tg_handle и telegram_username) и предложить мерж в
более частотный ключ. Верни СТРОГО JSON:
{"described":[{"key":"...","description":"...","value_kind":"..."}],
 "merges":[{"from":"...","into":"...","reason":"..."}]}.`;

@Injectable()
export class SegmentationService {
  private readonly logger = new Logger(SegmentationService.name);

  constructor(
    @Inject(CRM_DB) private readonly holder: unknown,
    private readonly ai: AiService,
  ) {}

  /** Снимок реестра для промптов: AI видит реальное пространство признаков, не выдумывает. */
  private async registrySnapshot(): Promise<string> {
    const { db } = dbOf(this.holder);
    const rows = await db
      .select()
      .from(attributeRegistry)
      .where(ne(attributeRegistry.status, "merged"))
      .orderBy(desc(attributeRegistry.seenCount))
      .limit(300);
    return JSON.stringify(
      rows.map((r) => ({
        key: r.key,
        description: r.description,
        seen: r.seenCount,
        examples: (r.examples ?? []).slice(0, 3),
      })),
    );
  }

  /** Полная карта признаков контактов (для исполнения фильтров). Внутренний масштаб — ок. */
  async attributeMaps(): Promise<Map<string, AttributeMap>> {
    const { db } = dbOf(this.holder);
    const rows = await db
      .select({
        contactId: contactAttributes.contactId,
        key: contactAttributes.key,
        value: contactAttributes.value,
      })
      .from(contactAttributes);
    const maps = new Map<string, AttributeMap>();
    for (const row of rows) {
      const attrs = maps.get(row.contactId) ?? {};
      attrs[row.key] = row.value;
      maps.set(row.contactId, attrs);
    }
    return maps;
  }

  evaluate(maps: Map<string, AttributeMap>, filter: FilterDsl): string[] {
    const ids: string[] = [];
    for (const [contactId, attrs] of maps) {
      if (matchesFilter(attrs, filter)) ids.push(contactId);
    }
    return ids;
  }

  /** AI-сегментатор: нейронка сама придумывает/обновляет smart views по текущему корпусу. */
  async refreshViews(): Promise<{ views: Array<{ title: string; count: number }> }> {
    const { db } = dbOf(this.holder);
    const existing = await db.select().from(smartViews);
    const registry = await this.registrySnapshot();

    const proposal = await this.ai.json<{
      views: Array<{ title: string; description?: string; rationale?: string; filter: unknown }>;
    }>(
      "views_refresh",
      SEGMENT_SYSTEM,
      JSON.stringify({
        registry: JSON.parse(registry),
        current_views: existing.map((v) => ({ title: v.title, filter: v.filter })),
      }),
    );

    const maps = await this.attributeMaps();
    const out: Array<{ title: string; count: number }> = [];
    for (const view of proposal.views ?? []) {
      let filter: FilterDsl;
      try {
        filter = parseFilter(view.filter);
      } catch (error) {
        this.logger.warn(`AI proposed invalid filter for "${view.title}": ${String(error)}`);
        continue;
      }
      const count = this.evaluate(maps, filter).length;
      const prior = existing.find((v) => v.title === view.title);
      if (prior) {
        await db
          .update(smartViews)
          .set({
            description: view.description ?? prior.description,
            rationale: view.rationale ?? prior.rationale,
            filter: filter as Record<string, unknown>,
            contactCount: count,
            refreshedAt: new Date(),
            updatedAt: new Date(),
          })
          .where(eq(smartViews.id, prior.id));
      } else {
        await db.insert(smartViews).values({
          title: view.title,
          description: view.description,
          rationale: view.rationale,
          filter: filter as Record<string, unknown>,
          createdBy: "ai",
          contactCount: count,
          refreshedAt: new Date(),
        });
      }
      out.push({ title: view.title, count });
    }
    return { views: out };
  }

  /** «Кто мне нужен» → AI-фильтр → выборка. */
  async ask(description: string): Promise<{ filter: FilterDsl; explanation: string; contactIds: string[] }> {
    const registry = await this.registrySnapshot();
    const answer = await this.ai.json<{ filter: unknown; explanation?: string }>(
      "ask",
      ASK_SYSTEM,
      JSON.stringify({ registry: JSON.parse(registry), description }),
    );
    const filter = parseFilter(answer.filter);
    const maps = await this.attributeMaps();
    return {
      filter,
      explanation: answer.explanation ?? "",
      contactIds: this.evaluate(maps, filter),
    };
  }

  /** AI-куратор реестра: описания новым ключам + мерж синонимов. */
  async curateRegistry(): Promise<{ described: number; merged: number }> {
    const { db } = dbOf(this.holder);
    const rows = await db.select().from(attributeRegistry).orderBy(desc(attributeRegistry.seenCount)).limit(300);
    const answer = await this.ai.json<{
      described?: Array<{ key: string; description: string; value_kind?: string }>;
      merges?: Array<{ from: string; into: string; reason?: string }>;
    }>(
      "registry_curate",
      CURATE_SYSTEM,
      JSON.stringify(
        rows.map((r) => ({
          key: r.key,
          description: r.description,
          seen: r.seenCount,
          examples: (r.examples ?? []).slice(0, 3),
        })),
      ),
    );

    let described = 0;
    for (const item of answer.described ?? []) {
      const updated = await db
        .update(attributeRegistry)
        .set({
          description: item.description,
          valueKind: item.value_kind,
          status: "active",
          updatedAt: new Date(),
        })
        .where(eq(attributeRegistry.key, item.key))
        .returning({ key: attributeRegistry.key });
      described += updated.length;
    }

    let merged = 0;
    for (const merge of answer.merges ?? []) {
      if (merge.from === merge.into) continue;
      const [from, into] = [merge.from, merge.into];
      const exists = await db.select({ key: attributeRegistry.key }).from(attributeRegistry)
        .where(eq(attributeRegistry.key, into)).limit(1);
      if (exists.length === 0) continue;
      // Переносим значения признака на канонический ключ (существующие не перетираем).
      await db.execute(sql`
        INSERT INTO contact_attributes (contact_id, key, value, confidence, evidence, source, updated_at)
        SELECT contact_id, ${into}, value, confidence, evidence, source, now()
        FROM contact_attributes WHERE key = ${from}
        ON CONFLICT (contact_id, key) DO NOTHING`);
      await db.execute(sql`DELETE FROM contact_attributes WHERE key = ${from}`);
      await db
        .update(attributeRegistry)
        .set({ status: "merged", mergedInto: into, aiNotes: merge.reason, updatedAt: new Date() })
        .where(eq(attributeRegistry.key, from));
      merged += 1;
    }
    return { described, merged };
  }
}
