import {
  BadRequestException,
  Body,
  Controller,
  Get,
  Header,
  Headers,
  HttpCode,
  Param,
  Post,
  Query,
  UseGuards,
} from "@nestjs/common";
import { z } from "zod";
import { AdminGuard } from "./admin.guard.js";
import { CurrentAuth, type RequestAuth, SessionAuthGuard } from "./auth.guard.js";
import { ReferralApplicationService } from "./referral-applications.service.js";

const bps = (maximum: number) => z.number().int().min(0).max(maximum);
const authoritySchema = z.object({
  teamOverrideMaxBps: bps(2_000),
  teamInvitesEnabled: z.boolean(),
  b2bEnabled: z.boolean(),
  b2bMaxDiscountBps: bps(9_500),
  b2bCanDelegate: z.boolean(),
}).strict().refine(
  (value) => value.b2bEnabled || (value.b2bMaxDiscountBps === 0 && !value.b2bCanDelegate),
  "a disabled B2B grant cannot retain a ceiling or delegation",
);
const submissionSchema = z.object({ message: z.string().trim().max(2_000).default("") }).strict();
const listSchema = z.object({
  status: z.enum(["pending", "approved", "rejected"]).optional(),
  limit: z.coerce.number().int().min(1).max(200).optional(),
}).strict();
const decisionSchema = z.object({
  action: z.enum(["approve", "reject"]),
  note: z.string().trim().min(1).max(2_000),
  commissionBps: bps(10_000).optional(),
  authority: authoritySchema.optional(),
}).strict();

function adminActor(header: string | undefined): string {
  const actor = (header ?? "").trim();
  return actor.length > 0 && actor.length <= 120 ? actor : "admin";
}

@Controller("referral/applications")
@UseGuards(SessionAuthGuard)
export class ReferralApplicationController {
  constructor(private readonly applications: ReferralApplicationService) {}

  @Get("me")
  @Header("Cache-Control", "no-store")
  mine(@CurrentAuth() current: RequestAuth): Promise<unknown> {
    return this.applications.mine(current.user.id);
  }

  @Post()
  @HttpCode(201)
  submit(@CurrentAuth() current: RequestAuth, @Body() body: unknown): Promise<unknown> {
    const parsed = submissionSchema.safeParse(body ?? {});
    if (!parsed.success) throw new BadRequestException("invalid application");
    return this.applications.submit(current.user.id, parsed.data.message);
  }
}

@Controller("admin/referral/applications")
@UseGuards(AdminGuard)
export class AdminReferralApplicationController {
  constructor(private readonly applications: ReferralApplicationService) {}

  @Get()
  @Header("Cache-Control", "no-store")
  list(@Query() query: unknown): Promise<unknown> {
    const parsed = listSchema.safeParse(query ?? {});
    if (!parsed.success) throw new BadRequestException("invalid application query");
    return this.applications.list(parsed.data);
  }

  @Post(":applicationId/decision")
  @HttpCode(200)
  decide(
    @Param("applicationId") applicationId: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader: string | undefined,
  ): Promise<unknown> {
    const id = z.string().uuid().safeParse(applicationId);
    if (!id.success) throw new BadRequestException("invalid application id");
    const parsed = decisionSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid decision");
    return this.applications.decide({ ...parsed.data, id: id.data, actor: adminActor(actorHeader) });
  }
}
