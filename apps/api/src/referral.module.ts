import { Module } from "@nestjs/common";
import { AdminGuard } from "./admin.guard.js";
import {
  AdminReferralController,
  AdminUserReferralController,
  ReferralController,
} from "./referral.controller.js";
import {
  AdminReferralApplicationController,
  ReferralApplicationController,
} from "./referral-applications.controller.js";
import { ReferralApplicationService } from "./referral-applications.service.js";
import { DevbotPartnerNotifier } from "./devbot-partner.notifier.js";
import { ReferralInvitationController } from "./referral-invitations.controller.js";
import { ReferralInvitationService } from "./referral-invitations.service.js";
import { ReferralSalesClient } from "./referral-sales.client.js";
import { ReferralService } from "./referral.service.js";

@Module({
  controllers: [
    ReferralController,
    ReferralApplicationController,
    ReferralInvitationController,
    AdminReferralController,
    AdminReferralApplicationController,
    AdminUserReferralController,
  ],
  providers: [AdminGuard, ReferralSalesClient, ReferralService, DevbotPartnerNotifier, ReferralApplicationService, ReferralInvitationService],
})
export class ReferralModule {}
