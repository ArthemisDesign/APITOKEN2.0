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
  Put,
  Query,
  UseGuards,
} from "@nestjs/common";
import {
  adminActorSchema,
  adminReasonSchema,
  createBusinessInviteSchema,
  setBusinessPricingSchema,
} from "@claude-api/contracts";
import {
  BusinessCustomerNotFoundError,
  BusinessInvitationConflictError,
  BusinessInvitationNotFoundError,
} from "@claude-api/db";
import { EngineClientError } from "@claude-api/engine-client";
import { z } from "zod";
import { AdminGuard } from "./admin.guard.js";
import {
  AdminCreditError,
  AdminService,
} from "./admin.service.js";

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
  // Сортировка — только закрытый enum: значение уходит в ORDER BY белого списка на стороне БД.
  // balance_usd/spent_usd осознанно недоступны: это live-поля движка, доклеиваемые после
  // пагинации, — глобальную сортировку по ним на стороне БД не построить (см. admin-overview.ts).
  sort: z.enum(["created_at", "last_seen_at", "paid_total", "topup_total", "spent_30d"]).default("created_at"),
  dir: z.enum(["asc", "desc"]).default("desc"),
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
    @Query("sort") sort?: string,
    @Query("dir") dir?: string,
  ): Promise<unknown> {
    const parsed = userListSchema.safeParse({
      limit, offset, q, status, auth, customer_type: customerType, sort, dir,
    });
    if (!parsed.success) throw new BadRequestException("invalid user list filters");
    return this.admin.listUsers({
      limit: parsed.data.limit,
      offset: parsed.data.offset,
      sort: parsed.data.sort,
      dir: parsed.data.dir,
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

  @Post("users/:id/provisioning-repair")
  @Header("Cache-Control", "no-store")
  async repairUserProvisioning(
    @Param("id") id: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("user ID must be a UUID");
    const reason = adminReasonSchema.safeParse(
      (body as { reason?: unknown })?.reason,
    );
    const actor = adminActorSchema.safeParse(actorHeader?.trim());
    if (!reason.success) throw new BadRequestException("reason is required");
    if (!actor.success) throw new BadRequestException("verified admin actor is required");
    return this.admin.repairUserProvisioningV2(id, actor.data, reason.data);
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
        expiresInDays: parsed.data.expiresInDays,
        reason: parsed.data.reason,
        idempotencyKey: parsed.data.idempotencyKey,
        ...(parsed.data.discountPercent === undefined ? {} : { discountPercent: parsed.data.discountPercent }),
        ...(parsed.data.email === undefined ? {} : { email: parsed.data.email }),
        actorId: adminActor(actorHeader),
      });
    } catch (error) {
      if (error instanceof BusinessInvitationConflictError) throw new HttpException(error.message, 409);
      throw error;
    }
  }





  /** The customer's per-provider discount overrides; the default lives on the user record. */
  @Get("business-users/:id/pricing")
  @Header("Cache-Control", "no-store")
  async getBusinessUserPricing(@Param("id") id: string): Promise<unknown> {
    assertUuid(id, "user ID");
    return this.admin.getBusinessPricing(id);
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
        {
          ...(parsed.data.discountPercent === undefined ? {} : { discountPercent: parsed.data.discountPercent }),
          ...(parsed.data.providers === undefined ? {} : { providers: parsed.data.providers }),
        },
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

function verifiedAdminActor(value: string | undefined): string {
  const actor = adminActorSchema.safeParse(value?.trim());
  if (!actor.success) throw new BadRequestException("verified admin actor is required");
  return actor.data;
}

function assertUuid(value: string, label: string): void {
  if (!uuidSchema.safeParse(value).success) throw new BadRequestException(`${label} must be a UUID`);
}

function serviceOwnerId(value: string): string {
  const ownerId = value.trim();
  if (!ownerId || ownerId.length > 200 || ownerId.includes("/")) {
    throw new BadRequestException("service policy owner ID is invalid");
  }
  return ownerId;
}

function managedProductId(value: string | undefined): string {
  const productId = value?.trim() || "main";
  if (!/^[a-z][a-z0-9_-]{0,63}$/.test(productId)) {
    throw new BadRequestException("pricing product ID is invalid");
  }
  return productId;
}


function pricingStageControlException(message: string, code: string, status: number): HttpException {
  const response = { statusCode: status, message, code };
  return status === 404 ? new NotFoundException(response) : new HttpException(response, status);
}

