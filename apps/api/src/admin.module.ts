import { Module } from "@nestjs/common";
import { AdminController } from "./admin.controller.js";
import { AdminGuard } from "./admin.guard.js";
import { AdminOperationsController } from "./admin-operations.controller.js";
import { AdminOperationsService } from "./admin-operations.service.js";
import { AdminService } from "./admin.service.js";

@Module({
  controllers: [AdminController, AdminOperationsController],
  providers: [AdminGuard, AdminService, AdminOperationsService],
})
export class AdminModule {}
