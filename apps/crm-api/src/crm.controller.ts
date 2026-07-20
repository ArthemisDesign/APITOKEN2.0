import {
  BadRequestException,
  Body,
  Controller,
  Get,
  Inject,
  NotFoundException,
  Param,
  Post,
  Query,
  UseGuards,
} from "@nestjs/common";
import { desc, eq, inArray, ne } from "drizzle-orm";
import { z } from "zod";
import {
  aiAudit,
  attributeRegistry,
  contactAttributes,
  contactChannels,
  contacts,
  ingestRuns,
  parseFilter,
  smartViews,
} from "@claude-api/crm-db";
import { CRM_DB, dbOf } from "./db.provider.js";
import { ingestBodySchema } from "./envelope.js";
import { IngestKeyGuard } from "./ingest.guard.js";
import { IngestService } from "./ingest.service.js";
import { SegmentationService } from "./segmentation.service.js";

const askSchema = z.object({ description: z.string().min(3).max(4_000) });

@Controller()
export class CrmController {
  constructor(
    @Inject(CRM_DB) private readonly holder: unknown,
    private readonly ingest: IngestService,
    private readonly segmentation: SegmentationService,
  ) {}

  // ── Ингест (парсеры, x-crm-ingest-key; людской basic_auth сюда не доходит) ────────

  @Post("ingest/contacts")
  @UseGuards(IngestKeyGuard)
  async ingestContacts(@Body() body: unknown) {
    const parsed = ingestBodySchema.safeParse(body);
    if (!parsed.success) {
      throw new BadRequestException(parsed.error.issues.map((i) => `${i.path.join(".")}: ${i.message}`));
    }
    return this.ingest.ingestBatch(parsed.data.run.parser, parsed.data.run.run_id, parsed.data.contacts);
  }

  // ── Чтение (за basic_auth Caddy) ──────────────────────────────────────────────────

  @Get("overview")
  async overview() {
    const { db } = dbOf(this.holder);
    const [contactRows, viewRows, registryRows, runRows] = await Promise.all([
      db.select({ id: contacts.id }).from(contacts),
      db.select({ id: smartViews.id }).from(smartViews),
      db.select({ key: attributeRegistry.key }).from(attributeRegistry).where(ne(attributeRegistry.status, "merged")),
      db.select().from(ingestRuns).orderBy(desc(ingestRuns.id)).limit(10),
    ]);
    return {
      contacts: contactRows.length,
      views: viewRows.length,
      attributes: registryRows.length,
      recent_runs: runRows,
    };
  }

  @Get("contacts")
  async listContacts(@Query("view") viewId?: string, @Query("filter") filterJson?: string, @Query("limit") limit?: string) {
    const { db } = dbOf(this.holder);
    const max = Math.min(Number(limit) || 100, 500);

    let ids: string[] | null = null;
    if (viewId) {
      const view = await db.select().from(smartViews).where(eq(smartViews.id, viewId)).limit(1);
      if (view.length === 0) throw new NotFoundException("view not found");
      ids = this.segmentation.evaluate(await this.maps(), parseFilter(view[0]!.filter));
    } else if (filterJson) {
      let raw: unknown;
      try {
        raw = JSON.parse(filterJson);
      } catch {
        throw new BadRequestException("filter must be JSON");
      }
      ids = this.segmentation.evaluate(await this.maps(), parseFilter(raw));
    }

    const rows =
      ids === null
        ? await db.select().from(contacts).orderBy(desc(contacts.updatedAt)).limit(max)
        : ids.length === 0
          ? []
          : await db.select().from(contacts).where(inArray(contacts.id, ids.slice(0, max)));
    return { total: ids === null ? rows.length : ids.length, contacts: rows };
  }

  @Get("contacts/:id")
  async getContact(@Param("id") id: string) {
    const { db } = dbOf(this.holder);
    const contact = await db.select().from(contacts).where(eq(contacts.id, id)).limit(1);
    if (contact.length === 0) throw new NotFoundException("contact not found");
    const [channels, attributes] = await Promise.all([
      db.select().from(contactChannels).where(eq(contactChannels.contactId, id)),
      db.select().from(contactAttributes).where(eq(contactAttributes.contactId, id)),
    ]);
    return { ...contact[0], channels, attributes };
  }

  @Get("views")
  async listViews() {
    const { db } = dbOf(this.holder);
    return { views: await db.select().from(smartViews).orderBy(desc(smartViews.contactCount)) };
  }

  @Get("registry")
  async listRegistry() {
    const { db } = dbOf(this.holder);
    return {
      attributes: await db.select().from(attributeRegistry).orderBy(desc(attributeRegistry.seenCount)).limit(500),
    };
  }

  @Get("audit")
  async listAudit() {
    const { db } = dbOf(this.holder);
    return { audit: await db.select().from(aiAudit).orderBy(desc(aiAudit.id)).limit(100) };
  }

  // ── AI-действия (за basic_auth Caddy) ─────────────────────────────────────────────

  @Post("views/refresh")
  async refreshViews() {
    return this.segmentation.refreshViews();
  }

  @Post("registry/curate")
  async curateRegistry() {
    return this.segmentation.curateRegistry();
  }

  @Post("ask")
  async ask(@Body() body: unknown) {
    const parsed = askSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("description (3..4000 chars) required");
    const answer = await this.segmentation.ask(parsed.data.description);
    const { db } = dbOf(this.holder);
    const rows =
      answer.contactIds.length === 0
        ? []
        : await db.select().from(contacts).where(inArray(contacts.id, answer.contactIds.slice(0, 200)));
    return { filter: answer.filter, explanation: answer.explanation, total: answer.contactIds.length, contacts: rows };
  }

  private async maps() {
    return this.segmentation.attributeMaps();
  }
}
