import { Module } from "@nestjs/common";
import { AdminController } from "./admin.controller.js";
import { AdminEventsController } from "./admin-events.controller.js";
import { AdminEventsService } from "./admin-events.service.js";
import { AdminKeyGuard } from "./admin.guard.js";
import { AuthModule } from "./auth.module.js";
import { CommerceService } from "./commerce.service.js";
import { PartnerRequestEffectService } from "./partner-request-effect.service.js";

@Module({
  imports: [AuthModule],
  controllers: [AdminController, AdminEventsController],
  providers: [AdminKeyGuard, AdminEventsService, CommerceService, PartnerRequestEffectService],
})
export class AdminModule {}
