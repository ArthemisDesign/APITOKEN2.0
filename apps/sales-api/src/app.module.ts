import { Module } from "@nestjs/common";
import { ConfigModule } from "@nestjs/config";
import { AdminModule } from "./admin.module.js";
import { AuthModule } from "./auth.module.js";
import { validateEnvironment } from "./config.js";
import { EmailService } from "./email.service.js";
import { HealthController } from "./health.controller.js";
import { InfrastructureModule } from "./infrastructure.module.js";
import { PartnerModule } from "./partner.module.js";
import { ReadinessService } from "./readiness.service.js";
import { SyncService } from "./sync.service.js";

@Module({
  imports: [
    ConfigModule.forRoot({ isGlobal: true, validate: validateEnvironment }),
    InfrastructureModule,
    AuthModule,
    PartnerModule,
    AdminModule,
  ],
  controllers: [HealthController],
  providers: [ReadinessService, SyncService, EmailService],
})
export class AppModule {}
