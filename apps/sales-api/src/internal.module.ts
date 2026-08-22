import { Module } from "@nestjs/common";
import { InternalController, InternalPartnersController, InternalKeyGuard } from "./internal.controller.js";
import { CommercePartnerController } from "./commerce-partner.controller.js";
import { CommerceService } from "./commerce.service.js";

@Module({
  controllers: [InternalController, InternalPartnersController, CommercePartnerController],
  providers: [InternalKeyGuard, CommerceService],
})
export class InternalModule {}
