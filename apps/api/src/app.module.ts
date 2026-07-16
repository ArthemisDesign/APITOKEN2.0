import { Module } from "@nestjs/common";
import { ConfigModule } from "@nestjs/config";
import { validateEnvironment } from "./config.js";
import { HealthController } from "./health.controller.js";
import { InfrastructureModule } from "./infrastructure.module.js";
import { PaymentsModule } from "./payments.module.js";
import { ReadinessService } from "./readiness.service.js";
import { AuthModule } from "./auth.module.js";
import { AccountModule } from "./account.module.js";
import { AdminModule } from "./admin.module.js";

@Module({
  imports: [
    ConfigModule.forRoot({ isGlobal: true, validate: validateEnvironment }),
    InfrastructureModule,
    AuthModule,
    AccountModule,
    AdminModule,
    PaymentsModule,
  ],
  controllers: [HealthController],
  providers: [ReadinessService],
})
export class AppModule {}
