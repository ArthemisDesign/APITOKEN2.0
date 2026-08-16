import {
  BadRequestException,
  Body,
  CanActivate,
  Controller,
  ExecutionContext,
  Get,
  Header,
  HttpCode,
  Injectable,
  NotFoundException,
  Post,
  Query,
  UnauthorizedException,
  UseGuards,
} from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { z } from "zod";
import { safeEqual } from "./admin.guard.js";
import type { Environment } from "./config.js";
import { CrmBridgeService } from "./crm-bridge.service.js";

const externalRefSchema = z.string().uuid();

@Injectable()
export class CrmBridgeGuard implements CanActivate {
  constructor(private readonly config: ConfigService<Environment, true>) {}

  canActivate(context: ExecutionContext): boolean {
    const configured = this.config.get("CRM_CONTROL_KEY", { infer: true });
    if (!configured) throw new NotFoundException();
    const request = context.switchToHttp().getRequest<{
      headers: Record<string, string | string[] | undefined>;
    }>();
    const supplied = request.headers["x-api-key"];
    if (typeof supplied !== "string" || !safeEqual(configured, supplied)) {
      throw new UnauthorizedException("CRM bridge authentication required");
    }
    return true;
  }
}

@Controller("internal/crm")
@UseGuards(CrmBridgeGuard)
export class CrmBridgeController {
  constructor(private readonly bridge: CrmBridgeService) {}

  @Post("referral-link")
  @HttpCode(200)
  @Header("Cache-Control", "no-store")
  async referralLink(@Body() body: unknown) {
    const parsed = z.object({ externalRef: externalRefSchema }).strict().safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid CRM referral link payload");
    return this.bridge.ensureReferralLink(parsed.data.externalRef);
  }

  @Get("referral-profile")
  @Header("Cache-Control", "no-store")
  async referralProfile(@Query("externalRef") externalRef?: string) {
    const parsed = externalRefSchema.safeParse(externalRef);
    if (!parsed.success) throw new BadRequestException("invalid CRM externalRef");
    return this.bridge.referralProfile(parsed.data);
  }
}
