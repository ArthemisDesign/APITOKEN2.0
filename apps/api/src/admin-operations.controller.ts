import {
  BadRequestException,
  Body,
  Controller,
  Get,
  Header,
  Headers,
  HttpException,
  NotFoundException,
  Param,
  Patch,
  Post,
  Query,
  UseGuards,
} from "@nestjs/common";
import {
  AdminUserNotFoundError,
  BusinessCustomerNotFoundError,
  CustomerProfileNotFoundError,
} from "@claude-api/db";
import { z } from "zod";
import { AdminGuard } from "./admin.guard.js";
import { AdminOperationError, AdminOperationsService } from "./admin-operations.service.js";

const uuidSchema = z.string().uuid();
const limitSchema = z.coerce.number().int().min(1).max(500).default(100);
const reasonSchema = z.string().trim().min(3).max(300);
const balanceAdjustmentSchema = z.object({
  amount_usd: z.string().regex(/^[1-9][0-9]{0,4}$/),
  reason: reasonSchema,
  idempotency_key: z.string().uuid(),
}).strict();
const statusSchema = z.object({
  status: z.enum(["active", "disabled"]),
  reason: reasonSchema,
}).strict();
const securityActionSchema = z.object({ reason: reasonSchema }).strict();
const convertBusinessSchema = z.object({
  reason: reasonSchema,
  discountPercent: z.number().int().min(0).max(95),
}).strict();
// Один status-фильтр применяется к обоим спискам topups: к платежам — по payments.status,
// к чекаутам — по checkout_sessions.status (объединение обоих enum'ов). Без status каждый
// список сохраняет историческое окно (payments: paid_at IS NOT NULL, checkouts: <> 'paid').
const topupsQuerySchema = z.object({
  limit: limitSchema,
  offset: z.coerce.number().int().min(0).default(0),
  q: z.string().trim().max(200).optional(),
  provider: z.string().trim().min(1).max(40).optional(),
  status: z.enum(["paid", "refunded", "disputed", "failed", "pending", "canceled", "creating"]).optional(),
});
// from/to — ISO 8601 даты-времени (как timestamps в ответах API); границы created_at включительно.
// q ищет подстроку в target_id и в metadata::text (case-insensitive).
const auditQuerySchema = z.object({
  limit: limitSchema,
  offset: z.coerce.number().int().min(0).default(0),
  action: z.string().trim().min(1).max(120).optional(),
  actor_type: z.string().trim().min(1).max(80).optional(),
  q: z.string().trim().max(200).optional(),
  from: z.string().datetime({ offset: true }).optional(),
  to: z.string().datetime({ offset: true }).optional(),
});

@Controller("admin")
@UseGuards(AdminGuard)
export class AdminOperationsController {
  constructor(private readonly operations: AdminOperationsService) {}

  @Get("dashboard")
  @Header("Cache-Control", "no-store")
  dashboard(): Promise<Record<string, unknown>> {
    return this.operations.dashboard();
  }

  @Get("topups")
  @Header("Cache-Control", "no-store")
  topups(
    @Query("limit") limit?: string,
    @Query("offset") offset?: string,
    @Query("q") q?: string,
    @Query("provider") provider?: string,
    @Query("status") status?: string,
  ): Promise<Record<string, unknown>> {
    const parsed = topupsQuerySchema.safeParse({ limit, offset, q, provider, status });
    if (!parsed.success) throw new BadRequestException("invalid topups filters");
    return this.operations.topups({
      limit: parsed.data.limit,
      offset: parsed.data.offset,
      ...(parsed.data.q === undefined ? {} : { q: parsed.data.q }),
      ...(parsed.data.provider === undefined ? {} : { provider: parsed.data.provider }),
      ...(parsed.data.status === undefined ? {} : { status: parsed.data.status }),
    });
  }

  @Get("audit")
  @Header("Cache-Control", "no-store")
  audit(
    @Query("limit") limit?: string,
    @Query("offset") offset?: string,
    @Query("action") action?: string,
    @Query("actor_type") actorType?: string,
    @Query("q") q?: string,
    @Query("from") from?: string,
    @Query("to") to?: string,
  ): Promise<Record<string, unknown>> {
    const parsed = auditQuerySchema.safeParse({ limit, offset, action, actor_type: actorType, q, from, to });
    if (!parsed.success) throw new BadRequestException("invalid audit filters");
    return this.operations.audit({
      limit: parsed.data.limit,
      offset: parsed.data.offset,
      ...(parsed.data.action === undefined ? {} : { action: parsed.data.action }),
      ...(parsed.data.actor_type === undefined ? {} : { actorType: parsed.data.actor_type }),
      ...(parsed.data.q === undefined ? {} : { q: parsed.data.q }),
      ...(parsed.data.from === undefined ? {} : { from: new Date(parsed.data.from) }),
      ...(parsed.data.to === undefined ? {} : { to: new Date(parsed.data.to) }),
    });
  }

  @Get("audit/actions")
  @Header("Cache-Control", "no-store")
  auditActions(): Promise<Record<string, unknown>> {
    return this.operations.auditActions();
  }

  @Get("business-invites")
  @Header("Cache-Control", "no-store")
  businessInvites(@Query("limit") value?: string): Promise<Record<string, unknown>> {
    return this.operations.businessInvites(parseLimit(value));
  }

  @Post("users/:id/balance-adjustments")
  @Header("Cache-Control", "no-store")
  async creditUser(
    @Param("id") id: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<Record<string, unknown>> {
    assertUserId(id);
    const parsed = balanceAdjustmentSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    return this.mapErrors(() => this.operations.creditUser({
      userId: id,
      amountUsd: parsed.data.amount_usd,
      reason: parsed.data.reason,
      idempotencyKey: parsed.data.idempotency_key,
      actorId: adminActor(actorHeader),
    }));
  }

  @Post("users/:id/bonus/revoke")
  @Header("Cache-Control", "no-store")
  async revokeBonus(
    @Param("id") id: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<Record<string, unknown>> {
    assertUserId(id);
    const parsed = securityActionSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    return this.mapErrors(() => this.operations.revokeSignupBonus({
      userId: id,
      reason: parsed.data.reason,
      actorId: adminActor(actorHeader),
    }));
  }

  @Patch("users/:id/status")
  @Header("Cache-Control", "no-store")
  async setUserStatus(
    @Param("id") id: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<Record<string, unknown>> {
    assertUserId(id);
    const parsed = statusSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    return this.mapErrors(() => this.operations.setUserStatus({
      userId: id,
      ...parsed.data,
      actorId: adminActor(actorHeader),
    }));
  }

  @Post("users/:id/convert-to-business")
  @Header("Cache-Control", "no-store")
  async convertToBusiness(
    @Param("id") id: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<Record<string, unknown>> {
    assertUserId(id);
    const parsed = convertBusinessSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    return this.mapErrors(() => this.operations.convertToBusiness({
      userId: id,
      reason: parsed.data.reason,
      discountPercent: parsed.data.discountPercent,
      actorId: adminActor(actorHeader),
    }));
  }


  @Post("users/:id/sessions/revoke")
  @Header("Cache-Control", "no-store")
  async revokeSessions(
    @Param("id") id: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<Record<string, unknown>> {
    assertUserId(id);
    const parsed = securityActionSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    return this.mapErrors(() => this.operations.revokeSessions({
      userId: id,
      reason: parsed.data.reason,
      actorId: adminActor(actorHeader),
    }));
  }

  @Post("users/:id/totp/reset")
  @Header("Cache-Control", "no-store")
  async resetTotp(
    @Param("id") id: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<Record<string, unknown>> {
    assertUserId(id);
    const parsed = securityActionSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    return this.mapErrors(() => this.operations.resetTotp({
      userId: id,
      reason: parsed.data.reason,
      actorId: adminActor(actorHeader),
    }));
  }

  private async mapErrors(action: () => Promise<Record<string, unknown>>): Promise<Record<string, unknown>> {
    try {
      return await action();
    } catch (error) {
      if (error instanceof AdminOperationError) throw new HttpException(error.message, error.status);
      if (
        error instanceof AdminUserNotFoundError
        || error instanceof CustomerProfileNotFoundError
        || error instanceof BusinessCustomerNotFoundError
      ) throw new NotFoundException(error.message);
      throw error;
    }
  }
}

function parseLimit(value: string | undefined): number {
  const parsed = limitSchema.safeParse(value);
  if (!parsed.success) throw new BadRequestException("limit must be an integer from 1 to 500");
  return parsed.data;
}

function assertUserId(id: string): void {
  if (!uuidSchema.safeParse(id).success) throw new BadRequestException("user ID must be a UUID");
}

function adminActor(value: string | undefined): string {
  const normalized = value?.trim() ?? "";
  return /^[A-Za-z0-9._@-]{1,80}$/.test(normalized) ? normalized : "unknown";
}
