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
  createBusinessInviteSchema,
  pricingCatalogJobStageRequestV2Schema,
  pricingSwitchJobStageRequestV2Schema,
  pricingPolicyDeliveryRepairRequestV2Schema,
  pricingReleaseActivationOperatorV2Schema,
  pricingStageControlMutationReasonV2Schema,
  pricingReleaseActivationReconcileRequestV2Schema,
  pricingReleaseOrchestrationStageRequestV2Schema,
  pricingReleaseActivationStageRequestV2Schema,
  pricingStage5DryRunRequestV2Schema,
  pricingStage5MaterializeRequestV2Schema,
  pricingStage5RunQueryV2Schema,
  pricingStage6PlanQueryV2Schema,
  pricingStage6StageRequestV2Schema,
  pricingStage8CaptureStageRequestV2Schema,
  pricingShadowRolloutStageRequestV2Schema,
  providerSwitchEditorMutationSchema,
  pricingPolicyMutationSchema,
  serviceAccountInventoryMutationV2Schema,
  serviceAccountInventoryServiceIdV2Schema,
  setBusinessPricingSchema,
} from "@claude-api/contracts";
import {
  BusinessCustomerNotFoundError,
  BusinessInvitationConflictError,
  BusinessInvitationNotFoundError,
  FundingNormalizationJobV2Error,
  PricingControlJobStageError,
  PricingPolicyDeliveryRepairError,
  PricingPolicyWriteError,
  PricingReleaseActivationJobV2Error,
  PricingReleaseOrchestrationV2Error,
  PricingShadowRolloutV2Error,
  Stage5MaterializerV2Error,
  PricingStage8CaptureJobV2Error,
  ServiceAccountInventoryV2Error,
} from "@claude-api/db";
import { EngineClientError } from "@claude-api/engine-client";
import { z } from "zod";
import { AdminGuard } from "./admin.guard.js";
import {
  AdminCreditError,
  AdminService,
  AdminServiceAccountInventoryError,
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
    const reason = pricingStageControlMutationReasonV2Schema.safeParse(
      (body as { reason?: unknown })?.reason,
    );
    const actor = pricingReleaseActivationOperatorV2Schema.safeParse(actorHeader?.trim());
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
        ...(parsed.data.policy === undefined ? {} : { policy: parsed.data.policy }),
        ...(parsed.data.email === undefined ? {} : { email: parsed.data.email }),
        actorId: adminActor(actorHeader),
      });
    } catch (error) {
      if (error instanceof BusinessInvitationConflictError) throw new HttpException(error.message, 409);
      throwPricingPolicyHttpError(error);
      throw error;
    }
  }

  @Get("pricing-policies/global-b2c")
  @Header("Cache-Control", "no-store")
  async getGlobalB2cPricingPolicy(): Promise<unknown> {
    return this.getManagedPolicy("global_b2c", "global-b2c");
  }

  @Get("pricing-catalog")
  @Header("Cache-Control", "no-store")
  async getManagedPricingCatalog(@Query("product_id") productId?: string): Promise<unknown> {
    try {
      return await this.admin.getManagedPricingCatalog(managedProductId(productId));
    } catch (error) {
      throwPricingPolicyHttpError(error);
      throw error;
    }
  }

  @Patch("provider-switches")
  @Header("Cache-Control", "no-store")
  async updateManagedProviderSwitches(
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const parsed = providerSwitchEditorMutationSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    try {
      return await this.admin.updateManagedProviderSwitches(parsed.data, adminActor(actorHeader));
    } catch (error) {
      throwPricingPolicyHttpError(error);
      throw error;
    }
  }

  @Patch("pricing-policies/global-b2c")
  @Header("Cache-Control", "no-store")
  async updateGlobalB2cPricingPolicy(
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    return this.updateManagedPolicy("global_b2c", "global-b2c", body, actorHeader);
  }

  @Get("business-invites/:id/pricing-policy")
  @Header("Cache-Control", "no-store")
  async getBusinessInvitePricingPolicy(@Param("id") id: string): Promise<unknown> {
    assertUuid(id, "invitation ID");
    return this.getManagedPolicy("b2b_invitation", id);
  }

  @Patch("business-invites/:id/pricing-policy")
  @Header("Cache-Control", "no-store")
  async updateBusinessInvitePricingPolicy(
    @Param("id") id: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    assertUuid(id, "invitation ID");
    return this.updateManagedPolicy("b2b_invitation", id, body, actorHeader);
  }

  @Get("business-users/:id/pricing-policy")
  @Header("Cache-Control", "no-store")
  async getBusinessUserPricingPolicy(@Param("id") id: string): Promise<unknown> {
    assertUuid(id, "user ID");
    return this.getManagedPolicy("b2b_client", id);
  }

  @Get("service-policies")
  @Header("Cache-Control", "no-store")
  async listServicePricingPolicies(): Promise<unknown> {
    try {
      return await this.admin.listManagedServicePricingPolicies();
    } catch (error) {
      throwPricingPolicyHttpError(error);
      throw error;
    }
  }

  @Get("service-account-inventory")
  @Header("Cache-Control", "no-store")
  getServiceAccountInventoryV2(): Promise<unknown> {
    return this.admin.getServiceAccountInventoryV2();
  }

  @Get("pricing-stage5-v2")
  @Header("Cache-Control", "no-store")
  async getPricingStage5RunV2(
    @Query("plan_digest") planDigest: string | undefined,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const input = pricingStage5RunQueryV2Schema.safeParse({ plan_digest: planDigest });
    if (!input.success) throw new BadRequestException(input.error.flatten());
    verifiedAdminActor(actorHeader);
    const run = await this.admin.getPricingStage5RunV2(input.data.plan_digest);
    if (run === null) throw new NotFoundException("exact Stage 5 plan does not exist");
    return run;
  }

  @Post("pricing-stage5-v2/dry-run")
  @Header("Cache-Control", "no-store")
  async dryRunPricingStage5V2(
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const input = pricingStage5DryRunRequestV2Schema.safeParse(body ?? {});
    if (!input.success) throw new BadRequestException(input.error.flatten());
    verifiedAdminActor(actorHeader);
    try {
      return await this.admin.dryRunPricingStage5V2();
    } catch (error) {
      throwPricingStageControlHttpError(error);
      throw error;
    }
  }

  @Post("pricing-stage5-v2/materialize")
  @Header("Cache-Control", "no-store")
  async materializePricingStage5V2(
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const input = pricingStage5MaterializeRequestV2Schema.safeParse(body);
    if (!input.success) throw new BadRequestException(input.error.flatten());
    const actor = verifiedAdminActor(actorHeader);
    try {
      return await this.admin.materializePricingStage5V2(input.data, actor);
    } catch (error) {
      throwPricingStageControlHttpError(error);
      throw error;
    }
  }

  @Get("pricing-stage6-v2")
  @Header("Cache-Control", "no-store")
  async getPricingStage6V2(
    @Query("plan_digest") planDigest: string | undefined,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const input = pricingStage6PlanQueryV2Schema.safeParse({ plan_digest: planDigest });
    if (!input.success) throw new BadRequestException(input.error.flatten());
    verifiedAdminActor(actorHeader);
    try {
      return await this.admin.getPricingStage6V2(input.data.plan_digest);
    } catch (error) {
      throwPricingStageControlHttpError(error);
      throw error;
    }
  }

  @Post("pricing-stage6-v2/stage")
  @Header("Cache-Control", "no-store")
  async stagePricingStage6V2(
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const input = pricingStage6StageRequestV2Schema.safeParse(body);
    if (!input.success) throw new BadRequestException(input.error.flatten());
    const actor = verifiedAdminActor(actorHeader);
    try {
      return await this.admin.stagePricingStage6V2(input.data, actor);
    } catch (error) {
      throwPricingStageControlHttpError(error);
      throw error;
    }
  }

  @Post("pricing-catalog-jobs/stage")
  @Header("Cache-Control", "no-store")
  async stagePricingCatalogJobV2(
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const input = pricingCatalogJobStageRequestV2Schema.safeParse(body);
    if (!input.success) throw new BadRequestException(input.error.flatten());
    const actor = verifiedAdminActor(actorHeader);
    try {
      return await this.admin.stagePricingCatalogJobV2(input.data, actor);
    } catch (error) {
      throwPricingStageControlHttpError(error);
      throw error;
    }
  }

  @Post("pricing-switch-jobs/stage")
  @Header("Cache-Control", "no-store")
  async stagePricingSwitchJobV2(
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const input = pricingSwitchJobStageRequestV2Schema.safeParse(body);
    if (!input.success) throw new BadRequestException(input.error.flatten());
    const actor = verifiedAdminActor(actorHeader);
    try {
      return await this.admin.stagePricingSwitchJobV2(input.data, actor);
    } catch (error) {
      throwPricingStageControlHttpError(error);
      throw error;
    }
  }

  @Post("pricing-policy-delivery-repairs")
  @Header("Cache-Control", "no-store")
  async repairPricingPolicyDeliveryV2(    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const input = pricingPolicyDeliveryRepairRequestV2Schema.safeParse(body);
    if (!input.success) throw new BadRequestException(input.error.flatten());
    const actor = verifiedAdminActor(actorHeader);
    try {
      return await this.admin.repairPricingPolicyDeliveryV2(input.data, actor);
    } catch (error) {
      throwPricingStageControlHttpError(error);
      throw error;
    }
  }

  @Get("pricing-release-activation-v2")
  @Header("Cache-Control", "no-store")
  getPricingReleaseActivationControlV2(): Promise<unknown> {
    return this.admin.getPricingReleaseActivationControlV2();
  }

  @Get("pricing-stage8-capture-v2")
  @Header("Cache-Control", "no-store")
  getPricingStage8CaptureControlV2(): Promise<unknown> {
    return this.admin.getPricingStage8CaptureControlV2();
  }

  @Post("pricing-stage8-capture-v2/stage")
  @Header("Cache-Control", "no-store")
  async stagePricingStage8CaptureV2(
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const input = pricingStage8CaptureStageRequestV2Schema.safeParse(body);
    const actor = pricingReleaseActivationOperatorV2Schema.safeParse(actorHeader?.trim());
    if (!input.success) throw new BadRequestException(input.error.flatten());
    if (!actor.success) throw new BadRequestException("verified admin actor is required");
    try {
      return await this.admin.stagePricingStage8CaptureV2(input.data, actor.data);
    } catch (error) {
      if (error instanceof PricingStage8CaptureJobV2Error) {
        throw new HttpException(error.message, error.permanent ? 409 : 503);
      }
      throw error;
    }
  }

  @Get("pricing-shadow-rollout-v2")
  @Header("Cache-Control", "no-store")
  getPricingShadowRolloutControlV2(): Promise<unknown> {
    return this.admin.getPricingShadowRolloutControlV2();
  }

  @Post("pricing-shadow-rollout-v2/stage")
  @Header("Cache-Control", "no-store")
  async stagePricingShadowRolloutV2(
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const input = pricingShadowRolloutStageRequestV2Schema.safeParse(body);
    const actor = pricingReleaseActivationOperatorV2Schema.safeParse(actorHeader?.trim());
    if (!input.success) throw new BadRequestException(input.error.flatten());
    if (!actor.success) throw new BadRequestException("verified admin actor is required");
    try {
      return await this.admin.stagePricingShadowRolloutV2(input.data, actor.data);
    } catch (error) {
      if (error instanceof PricingShadowRolloutV2Error) {
        const statusCode = error.permanent ? 409 : 503;
        throw new HttpException({
          statusCode,
          message: error.permanent
            ? "pricing shadow rollout conflicts with durable authority"
            : "pricing shadow rollout authority is temporarily unavailable",
          code: error.code,
        }, statusCode);
      }
      throw error;
    }
  }

  @Post("pricing-release-activation-v2/stage")
  @Header("Cache-Control", "no-store")
  async stagePricingReleaseActivationV2(
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const input = pricingReleaseActivationStageRequestV2Schema.safeParse(body);
    const actor = pricingReleaseActivationOperatorV2Schema.safeParse(actorHeader?.trim());
    if (!input.success) throw new BadRequestException(input.error.flatten());
    if (!actor.success) throw new BadRequestException("verified admin actor is required");
    try {
      return await this.admin.stagePricingReleaseActivationV2(input.data, actor.data);
    } catch (error) {
      if (error instanceof PricingReleaseActivationJobV2Error) {
        throw new HttpException(error.message, error.permanent ? 409 : 503);
      }
      throw error;
    }
  }

  @Get("pricing-release-orchestration-v2")
  @Header("Cache-Control", "no-store")
  async getPricingReleaseOrchestrationV2(): Promise<unknown> {
    return this.admin.getPricingReleaseOrchestrationControlV2();
  }

  @Post("pricing-release-orchestration-v2/stage")
  @Header("Cache-Control", "no-store")
  async stagePricingReleaseOrchestrationV2(
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const input = pricingReleaseOrchestrationStageRequestV2Schema.safeParse(body);
    const actor = pricingReleaseActivationOperatorV2Schema.safeParse(actorHeader?.trim());
    if (!input.success) throw new BadRequestException(input.error.flatten());
    if (!actor.success) throw new BadRequestException("verified admin actor is required");
    try {
      return await this.admin.stagePricingReleaseOrchestrationV2(input.data, actor.data);
    } catch (error) {
      if (error instanceof PricingReleaseOrchestrationV2Error) {
        throw new HttpException(error.message, error.permanent ? 409 : 503);
      }
      throw error;
    }
  }

  @Post("pricing-release-activation-v2/reconcile")
  @Header("Cache-Control", "no-store")
  async reconcilePricingReleaseActivationV2(
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const input = pricingReleaseActivationReconcileRequestV2Schema.safeParse(body);
    const actor = pricingReleaseActivationOperatorV2Schema.safeParse(actorHeader?.trim());
    if (!input.success) throw new BadRequestException(input.error.flatten());
    if (!actor.success) throw new BadRequestException("verified admin actor is required");
    try {
      return await this.admin.reconcilePricingReleaseActivationV2(input.data, actor.data);
    } catch (error) {
      if (error instanceof PricingReleaseActivationJobV2Error) {
        throw new HttpException(error.message, error.permanent ? 409 : 503);
      }
      throw error;
    }
  }

  @Put("service-account-inventory/:id")
  @Header("Cache-Control", "no-store")
  async upsertServiceAccountInventoryV2(
    @Param("id") id: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const serviceId = serviceAccountInventoryServiceIdV2Schema.safeParse(id);
    const mutation = serviceAccountInventoryMutationV2Schema.safeParse(body);
    if (!serviceId.success) throw new BadRequestException("service account ID is invalid");
    if (!mutation.success) throw new BadRequestException(mutation.error.flatten());
    try {
      return await this.admin.upsertServiceAccountInventoryV2(
        serviceId.data,
        mutation.data,
        adminActor(actorHeader),
      );
    } catch (error) {
      if (error instanceof AdminServiceAccountInventoryError) {
        if (error.code === "engine_account_missing") throw new NotFoundException(error.message);
        throw new HttpException(error.message, 409);
      }
      if (error instanceof ServiceAccountInventoryV2Error) {
        throw new HttpException(error.message, 409);
      }
      throw error;
    }
  }

  @Get("service-policies/:id")
  @Header("Cache-Control", "no-store")
  async getServicePricingPolicy(
    @Param("id") id: string,
    @Query("product_id") productId?: string,
  ): Promise<unknown> {
    return this.getManagedPolicy("service", serviceOwnerId(id), managedProductId(productId));
  }

  @Patch("service-policies/:id")
  @Header("Cache-Control", "no-store")
  async updateServicePricingPolicy(
    @Param("id") id: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
    @Query("product_id") productId?: string,
  ): Promise<unknown> {
    return this.updateManagedPolicy(
      "service",
      serviceOwnerId(id),
      body,
      actorHeader,
      managedProductId(productId),
    );
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
          ...(parsed.data.policy === undefined ? {} : { policy: parsed.data.policy }),
        },
        adminActor(actorHeader),
        parsed.data.reason,
      );
    } catch (error) {
      if (error instanceof BusinessCustomerNotFoundError) throw new NotFoundException(error.message);
      throwPricingPolicyHttpError(error);
      throw error;
    }
  }

  private async getManagedPolicy(
    ownerType: "global_b2c" | "b2b_client" | "b2b_invitation" | "service",
    ownerId: string,
    productId?: string,
  ): Promise<unknown> {
    try {
      return await this.admin.getManagedPricingPolicy(ownerType, ownerId, productId);
    } catch (error) {
      if (error instanceof BusinessCustomerNotFoundError) throw new NotFoundException(error.message);
      throwPricingPolicyHttpError(error);
      throw error;
    }
  }

  private async updateManagedPolicy(
    ownerType: "global_b2c" | "b2b_client" | "b2b_invitation" | "service",
    ownerId: string,
    body: unknown,
    actorHeader?: string,
    productId?: string,
  ): Promise<unknown> {
    const parsed = pricingPolicyMutationSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    try {
      return await this.admin.updateManagedPricingPolicy(
        ownerType,
        ownerId,
        parsed.data,
        adminActor(actorHeader),
        productId,
      );
    } catch (error) {
      throwPricingPolicyHttpError(error);
      throw error;
    }
  }
}

function adminActor(value: string | undefined): string {
  const actor = value?.trim();
  return actor ? actor.slice(0, 200) : "admin-panel";
}

function verifiedAdminActor(value: string | undefined): string {
  const actor = pricingReleaseActivationOperatorV2Schema.safeParse(value?.trim());
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

function throwPricingPolicyHttpError(error: unknown): void {
  if (!(error instanceof PricingPolicyWriteError)) return;
  if (error.code === "policy_not_found") throw new NotFoundException(error.message);
  if (error.code === "invalid_owner_rule" || error.code === "rule_outside_catalog") {
    throw new BadRequestException(error.message);
  }
  throw new HttpException(error.message, 409);
}

function pricingStageControlException(message: string, code: string, status: number): HttpException {
  const response = { statusCode: status, message, code };
  return status === 404 ? new NotFoundException(response) : new HttpException(response, status);
}

function throwPricingStageControlHttpError(error: unknown): void {
  if (error instanceof PricingControlJobStageError) {
    throw pricingStageControlException(error.message, "pricing_control_job_not_found", 404);
  }
  if (error instanceof PricingPolicyDeliveryRepairError) {
    if (error.code === "repair_job_not_found") {
      throw pricingStageControlException(error.message, error.code, 404);
    }
    throw pricingStageControlException(error.message, error.code, 409);
  }
  if (error instanceof Stage5MaterializerV2Error) {
    throw pricingStageControlException(
      error.message,
      error.code,
      error.code.endsWith("_unavailable") ? 503 : 409,
    );
  }
  if (error instanceof FundingNormalizationJobV2Error) {
    throw pricingStageControlException(
      error.message,
      error.terminal ? "funding_normalization_terminal" : "funding_normalization_unavailable",
      error.terminal ? 409 : 503,
    );
  }
  if (error instanceof EngineClientError) {
    throw pricingStageControlException(
      error.message,
      error.retryable ? "engine_client_unavailable" : "engine_client_invalid",
      error.retryable ? 503 : 409,
    );
  }
}
