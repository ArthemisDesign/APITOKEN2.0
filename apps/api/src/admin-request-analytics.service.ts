import { Inject, Injectable } from "@nestjs/common";
import type {
  EngineRequestFactLogical,
  EngineRequestFactPage,
  EngineRequestFactSummary,
} from "@claude-api/contracts";
import type { EngineClient } from "@claude-api/engine-client";
import { ENGINE_CLIENT } from "./infrastructure.module.js";

export interface RequestAnalyticsQuery {
  from: number;
  to: number;
  accountId?: string;
  cursor?: string;
  limit?: number;
}

@Injectable()
export class AdminRequestAnalyticsService {
  constructor(@Inject(ENGINE_CLIENT) private readonly engine: EngineClient) {}

  summary(query: RequestAnalyticsQuery): Promise<EngineRequestFactSummary> {
    return this.engine.getRequestFactSummary({
      from: query.from,
      to: query.to,
      ...(query.accountId === undefined ? {} : { accountId: query.accountId }),
    });
  }

  page(query: RequestAnalyticsQuery): Promise<EngineRequestFactPage> {
    return this.engine.listRequestFacts({
      from: query.from,
      to: query.to,
      ...(query.accountId === undefined ? {} : { accountId: query.accountId }),
      ...(query.cursor === undefined ? {} : { cursor: query.cursor }),
      ...(query.limit === undefined ? {} : { limit: query.limit }),
    });
  }

  logical(logicalRequestId: string): Promise<EngineRequestFactLogical> {
    return this.engine.getRequestFactsByLogicalId(logicalRequestId);
  }
}
