import {
  BadRequestException,
  Body,
  Controller,
  Get,
  Header,
  HttpCode,
  Inject,
  NotFoundException,
  Param,
  Patch,
  Post,
  Query,
  UnprocessableEntityException,
  UseGuards,
} from "@nestjs/common";
import { z } from "zod";
import {
  decidePayout,
  getSalesOverview,
  listPartnersWithAggregates,
  listPayouts,
  updatePartnerAdmin,
  InvalidPayoutTransitionError,
  type SalesDatabase,
} from "@claude-api/sales-db";
import { AdminKeyGuard } from "./admin.guard.js";
import { SALES_DATABASE } from "./infrastructure.module.js";
import { adminPatchPartnerSchema, adminPayoutDecisionSchema, adminPayoutsQuerySchema } from "./schemas.js";

const uuidSchema = z.string().uuid();

@Controller("admin")
@UseGuards(AdminKeyGuard)
export class AdminController {
  constructor(@Inject(SALES_DATABASE) private readonly database: SalesDatabase) {}

  @Get("overview")
  @Header("Cache-Control", "no-store")
  async overview(): Promise<unknown> {
    const overview = await getSalesOverview(this.database);
    return {
      partners: overview.partners,
      activePartners: overview.activePartners,
      referredUsers: overview.referredUsers,
      totalSpendNano: overview.totalSpendNano.toString(),
      totalCommissionsNano: overview.totalCommissionsNano.toString(),
      pendingPayoutsNano: overview.pendingPayoutsNano.toString(),
      paidPayoutsNano: overview.paidPayoutsNano.toString(),
    };
  }

  @Get("partners")
  @Header("Cache-Control", "no-store")
  async partners(): Promise<unknown> {
    const partners = await listPartnersWithAggregates(this.database);
    return {
      items: partners.map((partner) => ({
        id: partner.id,
        email: partner.email,
        displayName: partner.displayName,
        status: partner.status,
        emailVerified: partner.emailVerified,
        referralCode: partner.referralCode,
        commissionBps: partner.commissionBps,
        subCommissionBps: partner.subCommissionBps,
        parentPartnerId: partner.parentPartnerId,
        parentEmail: partner.parentEmail,
        referredUsers: partner.referredUsers,
        teamSize: partner.teamSize,
        earnedNano: partner.earnedNano.toString(),
        paidNano: partner.paidNano.toString(),
        createdAt: partner.createdAt.toISOString(),
      })),
    };
  }

  @Patch("partners/:id")
  async patchPartner(@Param("id") id: string, @Body() body: unknown): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("invalid partner id");
    const parsed = adminPatchPartnerSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid partner update");
    const updated = await updatePartnerAdmin(this.database, id, {
      ...(parsed.data.commissionBps !== undefined ? { commissionBps: parsed.data.commissionBps } : {}),
      ...(parsed.data.subCommissionBps !== undefined ? { subCommissionBps: parsed.data.subCommissionBps } : {}),
      ...(parsed.data.status !== undefined ? { status: parsed.data.status } : {}),
      actorId: "sales-admin-key",
    });
    if (!updated) throw new NotFoundException("partner not found");
    return { updated: true };
  }

  @Get("payouts")
  @Header("Cache-Control", "no-store")
  async payouts(@Query() query: unknown): Promise<unknown> {
    const parsed = adminPayoutsQuerySchema.safeParse(query ?? {});
    if (!parsed.success) throw new BadRequestException("invalid payouts query");
    const payouts = await listPayouts(this.database, parsed.data.status);
    return {
      items: payouts.map((payout) => ({
        id: payout.id,
        partnerId: payout.partnerId,
        amountNano: payout.amountNano.toString(),
        status: payout.status,
        method: payout.method,
        details: payout.details,
        requestedAt: payout.requestedAt.toISOString(),
        decidedAt: payout.decidedAt?.toISOString() ?? null,
        paidAt: payout.paidAt?.toISOString() ?? null,
        adminNote: payout.adminNote,
      })),
    };
  }

  @Post("payouts/:id/decision")
  @HttpCode(200)
  async decide(@Param("id") id: string, @Body() body: unknown): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("invalid payout id");
    const parsed = adminPayoutDecisionSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid payout decision");
    try {
      const payout = await decidePayout(this.database, {
        payoutId: id,
        decision: parsed.data.action,
        note: parsed.data.note ?? null,
        actorId: "sales-admin-key",
      });
      return {
        payout: {
          id: payout.id,
          partnerId: payout.partnerId,
          amountNano: payout.amountNano.toString(),
          status: payout.status,
          decidedAt: payout.decidedAt?.toISOString() ?? null,
          paidAt: payout.paidAt?.toISOString() ?? null,
          adminNote: payout.adminNote,
        },
      };
    } catch (error) {
      if (error instanceof InvalidPayoutTransitionError) throw new UnprocessableEntityException(error.message);
      throw error;
    }
  }
}
