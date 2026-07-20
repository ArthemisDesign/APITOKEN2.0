import { Module } from "@nestjs/common";
import { AdminKeyGuard } from "../admin.guard.js";
import { PayoutController } from "./payout.controller.js";
import { PayoutService } from "./payout.service.js";

@Module({
  controllers: [PayoutController],
  providers: [AdminKeyGuard, PayoutService],
  exports: [PayoutService],
})
export class PayoutModule {}
