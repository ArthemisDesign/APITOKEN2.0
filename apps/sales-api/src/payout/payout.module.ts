import { Module } from "@nestjs/common";
import { AdminKeyGuard } from "../admin.guard.js";
import { PayoutController } from "./payout.controller.js";
import { PayoutService } from "./payout.service.js";
import { SyncModule } from "../sync.module.js";

@Module({
  imports: [SyncModule],
  controllers: [PayoutController],
  providers: [AdminKeyGuard, PayoutService],
  exports: [PayoutService],
})
export class PayoutModule {}
