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
import { ReferralSalesClient } from "./referral-sales.client.js";
import { ReferralService } from "./referral.service.js";

@Module({
  controllers: [
    ReferralController,
    ReferralApplicationController,
    AdminReferralController,
    AdminReferralApplicationController,
    AdminUserReferralController,
  ],
  providers: [AdminGuard, ReferralSalesClient, ReferralService, ReferralApplicationService],
})
export class ReferralModule {}
