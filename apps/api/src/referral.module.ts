import { Module } from "@nestjs/common";
import { AdminGuard } from "./admin.guard.js";
import {
  AdminReferralController,
  AdminUserReferralController,
  ReferralController,
} from "./referral.controller.js";
import { ReferralSalesClient } from "./referral-sales.client.js";
import { ReferralService } from "./referral.service.js";

@Module({
  controllers: [ReferralController, AdminReferralController, AdminUserReferralController],
  providers: [AdminGuard, ReferralSalesClient, ReferralService],
})
export class ReferralModule {}
