import { Module } from "@nestjs/common";
import { AdminAccountsController } from "./admin-accounts.controller.js";
import { AdminAccountsService } from "./admin-accounts.service.js";
import { AdminController } from "./admin.controller.js";
import { AdminGuard } from "./admin.guard.js";
import { AdminOperationsController } from "./admin-operations.controller.js";
import { AdminOperationsService } from "./admin-operations.service.js";
import { AdminService } from "./admin.service.js";
import { InternalAdminAuthController } from "./internal-admin-auth.controller.js";

@Module({
  controllers: [AdminController, AdminOperationsController, AdminAccountsController, InternalAdminAuthController],
  providers: [AdminGuard, AdminService, AdminOperationsService, AdminAccountsService],
})
export class AdminModule {}
