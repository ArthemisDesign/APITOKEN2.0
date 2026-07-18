import { Module } from "@nestjs/common";
import { InfrastructureModule } from "./infrastructure.module.js";
import { SalesFeedController, SalesFeedGuard } from "./sales-feed.controller.js";

@Module({
  imports: [InfrastructureModule],
  controllers: [SalesFeedController],
  providers: [SalesFeedGuard],
})
export class SalesFeedModule {}
