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
import { AdminUserNotFoundError } from "@claude-api/db";
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
  topups(@Query("limit") value?: string): Promise<Record<string, unknown>> {
    return this.operations.topups(parseLimit(value));
  }

  @Get("audit")
  @Header("Cache-Control", "no-store")
  audit(@Query("limit") value?: string): Promise<Record<string, unknown>> {
    return this.operations.audit(parseLimit(value));
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
      if (error instanceof AdminUserNotFoundError) throw new NotFoundException(error.message);
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
