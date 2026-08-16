import { Module } from "@nestjs/common";
import { InfrastructureModule } from "./infrastructure.module.js";
import { CrmBridgeController, CrmBridgeGuard } from "./crm-bridge.controller.js";
import { CrmBridgeService } from "./crm-bridge.service.js";

@Module({
  imports: [InfrastructureModule],
  controllers: [CrmBridgeController],
  providers: [CrmBridgeGuard, CrmBridgeService],
})
export class CrmBridgeModule {}
