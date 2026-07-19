import { Module } from "@nestjs/common";
import { InternalController, InternalPartnersController, InternalKeyGuard } from "./internal.controller.js";

@Module({
  controllers: [InternalController, InternalPartnersController],
  providers: [InternalKeyGuard],
})
export class InternalModule {}
