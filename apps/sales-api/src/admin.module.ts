import { Module } from "@nestjs/common";
import { AdminController } from "./admin.controller.js";
import { AdminKeyGuard } from "./admin.guard.js";

@Module({
  controllers: [AdminController],
  providers: [AdminKeyGuard],
})
export class AdminModule {}
