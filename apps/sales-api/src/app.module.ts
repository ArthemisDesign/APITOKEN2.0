import { Module } from "@nestjs/common";
import { ConfigModule } from "@nestjs/config";
import { AdminModule } from "./admin.module.js";
import { AuthModule } from "./auth.module.js";
import { validateEnvironment } from "./config.js";
import { EmailService } from "./email.service.js";
import { HealthController } from "./health.controller.js";
import { InfrastructureModule } from "./infrastructure.module.js";
import { InternalModule } from "./internal.module.js";
import { PartnerModule } from "./partner.module.js";
import { PayoutModule } from "./payout/payout.module.js";
import { ReadinessService } from "./readiness.service.js";

@Module({
  imports: [
    ConfigModule.forRoot({ isGlobal: true, validate: validateEnvironment }),
    InfrastructureModule,
    AuthModule,
    PartnerModule,
    AdminModule,
    PayoutModule,
    InternalModule,
  ],
  controllers: [HealthController],
  providers: [ReadinessService, EmailService],
})
export class AppModule {}
