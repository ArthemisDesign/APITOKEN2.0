import { Module } from "@nestjs/common";
import { ConfigModule } from "@nestjs/config";
import { AiService } from "./ai.service.js";
import { validateEnvironment } from "./config.js";
import { CrmController } from "./crm.controller.js";
import { crmDatabaseProvider } from "./db.provider.js";
import { HealthController } from "./health.controller.js";
import { IngestKeyGuard } from "./ingest.guard.js";
import { IngestService } from "./ingest.service.js";
import { ReadinessService } from "./readiness.service.js";
import { SegmentationService } from "./segmentation.service.js";

@Module({
  imports: [ConfigModule.forRoot({ isGlobal: true, validate: validateEnvironment })],
  controllers: [HealthController, CrmController],
  providers: [
    crmDatabaseProvider,
    ReadinessService,
    AiService,
    IngestService,
    SegmentationService,
    IngestKeyGuard,
  ],
})
export class AppModule {}
