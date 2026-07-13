import { Module } from "@nestjs/common";
import { ConfigModule } from "@nestjs/config";
import { validateEnvironment } from "./config.js";
import { HealthController } from "./health.controller.js";
import { InfrastructureModule } from "./infrastructure.module.js";
import { PaymentsModule } from "./payments.module.js";

@Module({
  imports: [
    ConfigModule.forRoot({ isGlobal: true, validate: validateEnvironment }),
    InfrastructureModule,
    PaymentsModule,
  ],
  controllers: [HealthController],
})
export class AppModule {}
