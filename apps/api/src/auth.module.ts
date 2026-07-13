import { Global, Module } from "@nestjs/common";
import { APP_GUARD } from "@nestjs/core";
import { AuthController } from "./auth.controller.js";
import { SessionAuthGuard } from "./auth.guard.js";
import { AuthService } from "./auth.service.js";
import { OriginGuard } from "./origin.guard.js";

@Global()
@Module({
  controllers: [AuthController],
  providers: [AuthService, SessionAuthGuard, { provide: APP_GUARD, useClass: OriginGuard }],
  exports: [AuthService, SessionAuthGuard],
})
export class AuthModule {}
