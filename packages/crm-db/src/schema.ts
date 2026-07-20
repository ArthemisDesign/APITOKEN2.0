import {
  bigserial,
  index,
  integer,
  jsonb,
  pgEnum,
  pgTable,
  real,
  text,
  timestamp,
  uniqueIndex,
  uuid,
} from "drizzle-orm/pg-core";
import { sql } from "drizzle-orm";

const createdAt = timestamp("created_at", { withTimezone: true }).notNull().defaultNow();
const updatedAt = timestamp("updated_at", { withTimezone: true }).notNull().defaultNow();

export const contactStatus = pgEnum("contact_status", ["new", "enriched", "qualified", "archived"]);
export const attributeStatus = pgEnum("attribute_status", ["proposed", "active", "merged"]);
export const viewCreator = pgEnum("view_creator", ["ai", "human"]);

/** Ядро: человек. Портрет (summary) пишет AI; признаки — в contact_attributes. */
export const contacts = pgTable("contacts", {
  id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
  name: text("name"),
  summary: text("summary"),
  status: contactStatus("status").notNull().default("new"),
  // Последний источник, обновивший контакт (parser/run_id) — полная история в ingest_runs.
  lastParser: text("last_parser"),
  lastRunId: text("last_run_id"),
  hypotheses: jsonb("hypotheses").$type<string[]>().notNull().default(sql`'[]'::jsonb`),
  raw: jsonb("raw").$type<Record<string, unknown>>().notNull().default(sql`'{}'::jsonb`),
  createdAt,
  updatedAt,
});

/** Каналы связи. (type,value) уникальны глобально — это ключ дедупликации при ингесте. */
export const contactChannels = pgTable(
  "contact_channels",
  {
    id: bigserial("id", { mode: "number" }).primaryKey(),
    contactId: uuid("contact_id").notNull().references(() => contacts.id, { onDelete: "cascade" }),
    type: text("type").notNull(),
    value: text("value").notNull(),
    createdAt,
  },
  (t) => [
    uniqueIndex("contact_channels_type_value_uq").on(t.type, t.value),
    index("contact_channels_contact_idx").on(t.contactId),
  ],
);

/** Открытое пространство признаков: ключи придумывает AI парсера, реестр курирует AI CRM. */
export const contactAttributes = pgTable(
  "contact_attributes",
  {
    id: bigserial("id", { mode: "number" }).primaryKey(),
    contactId: uuid("contact_id").notNull().references(() => contacts.id, { onDelete: "cascade" }),
    key: text("key").notNull(),
    value: jsonb("value").$type<unknown>().notNull(),
    confidence: real("confidence"),
    evidence: text("evidence"),
    source: text("source"),
    updatedAt,
  },
  (t) => [
    uniqueIndex("contact_attributes_contact_key_uq").on(t.contactId, t.key),
    index("contact_attributes_key_idx").on(t.key),
  ],
);

/** Живой реестр ключей признаков; курируется AI (описания, мерж синонимов). */
export const attributeRegistry = pgTable("attribute_registry", {
  key: text("key").primaryKey(),
  description: text("description"),
  valueKind: text("value_kind"),
  examples: jsonb("examples").$type<unknown[]>().notNull().default(sql`'[]'::jsonb`),
  status: attributeStatus("status").notNull().default("proposed"),
  mergedInto: text("merged_into"),
  aiNotes: text("ai_notes"),
  seenCount: integer("seen_count").notNull().default(0),
  createdAt,
  updatedAt,
});

/** AI-сегменты: фильтр-DSL (см. CRM_AI.md) + обоснование, зачем сегмент существует. */
export const smartViews = pgTable("smart_views", {
  id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
  title: text("title").notNull(),
  description: text("description"),
  filter: jsonb("filter").$type<Record<string, unknown>>().notNull(),
  rationale: text("rationale"),
  createdBy: viewCreator("created_by").notNull().default("ai"),
  contactCount: integer("contact_count").notNull().default(0),
  refreshedAt: timestamp("refreshed_at", { withTimezone: true }),
  createdAt,
  updatedAt,
});

/** Журнал ранов парсеров: сколько принято/починено AI/отклонено + drift формата. */
export const ingestRuns = pgTable(
  "ingest_runs",
  {
    id: bigserial("id", { mode: "number" }).primaryKey(),
    runId: text("run_id").notNull(),
    parser: text("parser").notNull(),
    received: integer("received").notNull().default(0),
    accepted: integer("accepted").notNull().default(0),
    repaired: integer("repaired").notNull().default(0),
    rejected: integer("rejected").notNull().default(0),
    drift: jsonb("drift").$type<unknown[]>().notNull().default(sql`'[]'::jsonb`),
    createdAt,
  },
  (t) => [index("ingest_runs_run_idx").on(t.runId)],
);

/** Каждое решение нейронки — на виду: что спросили, что решила. */
export const aiAudit = pgTable(
  "ai_audit",
  {
    id: bigserial("id", { mode: "number" }).primaryKey(),
    kind: text("kind").notNull(),
    model: text("model").notNull(),
    inputSummary: text("input_summary"),
    output: jsonb("output").$type<unknown>(),
    createdAt,
  },
  (t) => [index("ai_audit_kind_idx").on(t.kind)],
);
