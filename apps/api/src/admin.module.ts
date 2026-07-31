import { Module } from "@nestjs/common";
import { AdminAccountsController } from "./admin-accounts.controller.js";
import { AdminAccountsService } from "./admin-accounts.service.js";
import { AdminController } from "./admin.controller.js";
import { AdminFinanceController } from "./admin-finance.controller.js";
import { AdminFinanceService } from "./admin-finance.service.js";
import { AdminGuard } from "./admin.guard.js";
import { AdminOperationsController } from "./admin-operations.controller.js";
import { AdminOperationsService } from "./admin-operations.service.js";
import { AdminPipelinesController } from "./admin-pipelines.controller.js";
import { AdminPipelinesService } from "./admin-pipelines.service.js";
import { AdminService } from "./admin.service.js";
import { InternalAdminAuthController } from "./internal-admin-auth.controller.js";

@Module({
  controllers: [
    AdminController,
    AdminOperationsController,
    AdminAccountsController,
    AdminFinanceController,
    AdminPipelinesController,
    InternalAdminAuthController,
  ],
  providers: [
    AdminGuard,
    AdminService,
    AdminOperationsService,
    AdminAccountsService,
    AdminFinanceService,
    AdminPipelinesService,
  ],
})
export class AdminModule {}
