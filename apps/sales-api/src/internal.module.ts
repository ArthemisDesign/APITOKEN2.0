import { Module } from "@nestjs/common";
import { InternalController, InternalKeyGuard } from "./internal.controller.js";

@Module({
  controllers: [InternalController],
  providers: [InternalKeyGuard],
})
export class InternalModule {}
