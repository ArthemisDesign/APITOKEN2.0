import { Module } from "@nestjs/common";
import { AdminController } from "./admin.controller.js";
import { AdminKeyGuard } from "./admin.guard.js";
import { CommerceService } from "./commerce.service.js";

@Module({
  controllers: [AdminController],
  providers: [AdminKeyGuard, CommerceService],
})
export class AdminModule {}
