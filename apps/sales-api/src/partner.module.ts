import { Module } from "@nestjs/common";
import { AuthModule } from "./auth.module.js";
import { PartnerController } from "./partner.controller.js";
import { CommerceService } from "./commerce.service.js";

@Module({
  imports: [AuthModule],
  controllers: [PartnerController],
  providers: [CommerceService],
})
export class PartnerModule {}
