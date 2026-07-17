import {
  BadRequestException,
  Body,
  Controller,
  Get,
  Header,
  NotFoundException,
  Param,
  Patch,
  Post,
  UseGuards,
} from "@nestjs/common";
import {
  createBusinessInviteSchema,
  setBusinessPricingSchema,
} from "@claude-api/contracts";
import { BusinessCustomerNotFoundError } from "@claude-api/db";
import { z } from "zod";
import { AdminGuard } from "./admin.guard.js";
import { AdminService } from "./admin.service.js";

const uuidSchema = z.string().uuid();

@Controller("admin")
@UseGuards(AdminGuard)
export class AdminController {
  constructor(private readonly admin: AdminService) {}

  @Get("users")
  @Header("Cache-Control", "no-store")
  async listUsers(): Promise<unknown> {
    return this.admin.listUsers();
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
