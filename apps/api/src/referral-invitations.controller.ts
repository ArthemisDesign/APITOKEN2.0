import { BadRequestException, Body, Controller, Get, Header, HttpCode, Post, UseGuards } from "@nestjs/common";
import { z } from "zod";
import { CurrentAuth, type RequestAuth, SessionAuthGuard } from "./auth.guard.js";
import { ReferralInvitationService } from "./referral-invitations.service.js";

const declineSchema = z.object({ inviteId: z.string().uuid() }).strict();

@Controller("referral/invitation")
@UseGuards(SessionAuthGuard)
export class ReferralInvitationController {
  constructor(private readonly invitations: ReferralInvitationService) {}

  @Get()
  @Header("Cache-Control", "no-store")
  pending(@CurrentAuth() current: RequestAuth): Promise<unknown> {
    return this.invitations.pending(current.user.id);
  }

  @Post("accept")
  @HttpCode(200)
  accept(@CurrentAuth() current: RequestAuth): Promise<unknown> {
    return this.invitations.accept(current.user.id);
  }

  @Post("decline")
  @HttpCode(200)
  decline(@CurrentAuth() current: RequestAuth, @Body() body: unknown): Promise<unknown> {
    const parsed = declineSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid invitation decline");
    return this.invitations.decline(current.user.id, parsed.data.inviteId);
  }
}
