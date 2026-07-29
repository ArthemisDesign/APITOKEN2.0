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
  createBusinessInviteSchema,
  setBusinessPricingSchema,
} from "@claude-api/contracts";
import {
  BusinessCustomerNotFoundError,
  BusinessInvitationConflictError,
  BusinessInvitationNotFoundError,
} from "@claude-api/db";
import { z } from "zod";
import { AdminGuard } from "./admin.guard.js";
import { AdminCreditError, AdminService } from "./admin.service.js";

const uuidSchema = z.string().uuid();
const creditSchema = z.object({ amount_usd: z.string() });
const reasonSchema = z.string().trim().min(3).max(300);
const inviteActionSchema = z.object({ reason: reasonSchema }).strict();
const resendInviteSchema = z.object({
  reason: reasonSchema,
  expiresInDays: z.number().int().min(1).max(30).default(7),
  idempotencyKey: z.string().uuid(),
}).strict();
const userListSchema = z.object({
  limit: z.coerce.number().int().min(1).max(100).default(50),
  offset: z.coerce.number().int().min(0).default(0),
  q: z.string().trim().max(200).optional(),
  status: z.enum(["active", "disabled"]).optional(),
  auth: z.enum(["password", "google", "github"]).optional(),
  customer_type: z.enum(["b2c", "b2b"]).optional(),
});

@Controller("admin")
@UseGuards(AdminGuard)
export class AdminController {
  constructor(private readonly admin: AdminService) {}

  @Get("users")
  @Header("Cache-Control", "no-store")
  async listUsers(
    @Query("limit") limit?: string,
    @Query("offset") offset?: string,
    @Query("q") q?: string,
    @Query("status") status?: string,
    @Query("auth") auth?: string,
    @Query("customer_type") customerType?: string,
  ): Promise<unknown> {
    const parsed = userListSchema.safeParse({ limit, offset, q, status, auth, customer_type: customerType });
    if (!parsed.success) throw new BadRequestException("invalid user list filters");
    return this.admin.listUsers({
      limit: parsed.data.limit,
      offset: parsed.data.offset,
      ...(parsed.data.q === undefined ? {} : { search: parsed.data.q }),
      ...(parsed.data.status === undefined ? {} : { status: parsed.data.status }),
      ...(parsed.data.auth === undefined ? {} : { auth: parsed.data.auth }),
      ...(parsed.data.customer_type === undefined ? {} : { customerType: parsed.data.customer_type }),
    });
  }

  @Post("users/:id/credit")
  @Header("Cache-Control", "no-store")
  async creditUser(@Param("id") id: string, @Body() body: unknown): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("user ID must be a UUID");
    const parsed = creditSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    try {
      return await this.admin.creditUser(id, parsed.data.amount_usd);
    } catch (error) {
      if (error instanceof AdminCreditError) throw new HttpException(error.message, error.status);
      throw error;
    }
  }

  @Get("checkouts/:id/refund-eligibility")
  @Header("Cache-Control", "no-store")
  async refundEligibility(@Param("id") id: string): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("checkout ID must be a UUID");
    return this.admin.refundEligibility(id);
  }

  @Post("business-invites")
  @Header("Cache-Control", "no-store")
  async createBusinessInvite(
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const parsed = createBusinessInviteSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    try {
      return await this.admin.createBusinessInvite({
        discountPercent: parsed.data.discountPercent,
        expiresInDays: parsed.data.expiresInDays,
        reason: parsed.data.reason,
        idempotencyKey: parsed.data.idempotencyKey,
        ...(parsed.data.email === undefined ? {} : { email: parsed.data.email }),
        actorId: adminActor(actorHeader),
      });
    } catch (error) {
      if (error instanceof BusinessInvitationConflictError) throw new HttpException(error.message, 409);
      throw error;
    }
  }

  @Get("business-invites/:id/link")
  @Header("Cache-Control", "no-store")
  async businessInviteLink(@Param("id") id: string): Promise<unknown> {
    assertUuid(id, "invitation ID");
    try {
      return await this.admin.getBusinessInviteLink(id);
    } catch (error) {
      if (error instanceof BusinessInvitationNotFoundError) throw new NotFoundException(error.message);
      throw error;
    }
  }

  @Post("business-invites/:id/revoke")
  @Header("Cache-Control", "no-store")
  async revokeBusinessInvite(
    @Param("id") id: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    assertUuid(id, "invitation ID");
    const parsed = inviteActionSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    try {
      return await this.admin.revokeBusinessInvite(id, adminActor(actorHeader), parsed.data.reason);
    } catch (error) {
      if (error instanceof BusinessInvitationNotFoundError) throw new NotFoundException(error.message);
      throw error;
    }
  }

  @Post("business-invites/:id/resend")
  @Header("Cache-Control", "no-store")
  async resendBusinessInvite(
    @Param("id") id: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    assertUuid(id, "invitation ID");
    const parsed = resendInviteSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    try {
      return await this.admin.resendBusinessInvite(
        id,
        adminActor(actorHeader),
        parsed.data.reason,
        parsed.data.expiresInDays,
        parsed.data.idempotencyKey,
      );
    } catch (error) {
      if (error instanceof BusinessInvitationNotFoundError) throw new NotFoundException(error.message);
      if (error instanceof BusinessInvitationConflictError) throw new HttpException(error.message, 409);
      throw error;
    }
  }

  @Patch("business-users/:id/pricing")
  @Header("Cache-Control", "no-store")
  async setBusinessPricing(
    @Param("id") id: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("user ID must be a UUID");
    const parsed = setBusinessPricingSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    try {
      return await this.admin.setBusinessPricing(
        id,
        parsed.data.discountPercent,
        adminActor(actorHeader),
        parsed.data.reason,
      );
    } catch (error) {
      if (error instanceof BusinessCustomerNotFoundError) throw new NotFoundException(error.message);
      throw error;
    }
  }
}

function adminActor(value: string | undefined): string {
  const actor = value?.trim();
  return actor ? actor.slice(0, 200) : "admin-panel";
}

function assertUuid(value: string, label: string): void {
  if (!uuidSchema.safeParse(value).success) throw new BadRequestException(`${label} must be a UUID`);
}
