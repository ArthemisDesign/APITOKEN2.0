import { Module } from "@nestjs/common";
import { AuthController } from "./auth.controller.js";
import { SessionAuthGuard } from "./auth.guard.js";
import { AuthService } from "./auth.service.js";

@Module({
  controllers: [AuthController],
  providers: [AuthService, SessionAuthGuard],
  exports: [AuthService, SessionAuthGuard],
})
export class AuthModule {}
