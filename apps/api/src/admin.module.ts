import { Module } from "@nestjs/common";
import { AdminController } from "./admin.controller.js";
import { AdminGuard } from "./admin.guard.js";
import { AdminService } from "./admin.service.js";

@Module({
  controllers: [AdminController],
  providers: [AdminGuard, AdminService],
})
export class AdminModule {}
