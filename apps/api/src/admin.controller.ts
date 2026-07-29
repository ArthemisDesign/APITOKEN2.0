import {
  BadRequestException,
  Body,
  Controller,
  Get,
  Header,
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
import { BusinessCustomerNotFoundError } from "@claude-api/db";
import { z } from "zod";
import { AdminGuard } from "./admin.guard.js";
import { AdminCreditError, AdminService } from "./admin.service.js";

const uuidSchema = z.string().uuid();
const creditSchema = z.object({ amount_usd: z.string() });
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
  async createBusinessInvite(@Body() body: unknown): Promise<unknown> {
    const parsed = createBusinessInviteSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    return this.admin.createBusinessInvite(parsed.data);
  }

  @Patch("business-users/:id/pricing")
  @Header("Cache-Control", "no-store")
  async setBusinessPricing(@Param("id") id: string, @Body() body: unknown): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("user ID must be a UUID");
    const parsed = setBusinessPricingSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    try {
      return await this.admin.setBusinessPricing(id, parsed.data.discountPercent);
    } catch (error) {
      if (error instanceof BusinessCustomerNotFoundError) throw new NotFoundException(error.message);
      throw error;
    }
  }
}
