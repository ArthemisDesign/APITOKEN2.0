import { Module } from "@nestjs/common";
import { AuthModule } from "./auth.module.js";
import { PartnerController } from "./partner.controller.js";

@Module({
  imports: [AuthModule],
  controllers: [PartnerController],
})
export class PartnerModule {}
