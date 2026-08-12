import { Module } from "@nestjs/common";
import { SyncService } from "./sync.service.js";

/** One shared feed consumer; payout readiness invokes the same serialized service instance. */
@Module({
  providers: [SyncService],
  exports: [SyncService],
})
export class SyncModule {}
