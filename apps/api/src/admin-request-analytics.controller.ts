import {
  BadRequestException,
  Controller,
  Get,
  Header,
  Param,
  Query,
  UseGuards,
} from "@nestjs/common";
import { z } from "zod";
import { AdminGuard } from "./admin.guard.js";
import {
  AdminRequestAnalyticsService,
  type RequestAnalyticsQuery,
} from "./admin-request-analytics.service.js";

const querySchema = z.object({
  from: z.coerce.number().int().nonnegative(),
  to: z.coerce.number().int().positive(),
  accountId: z.string().min(1).max(128).optional(),
  cursor: z.string().min(1).max(64).optional(),
  limit: z.coerce.number().int().min(1).max(200).optional(),
}).superRefine((value, ctx) => {
  if (value.to <= value.from || value.to - value.from > 30 * 86_400) {
    ctx.addIssue({ code: z.ZodIssueCode.custom, message: "invalid request analytics window" });
  }
});

@Controller("admin/request-analytics")
@UseGuards(AdminGuard)
export class AdminRequestAnalyticsController {
  constructor(private readonly analytics: AdminRequestAnalyticsService) {}

  @Get("summary")
  @Header("Cache-Control", "no-store")
  summary(
    @Query("from") from?: string,
    @Query("to") to?: string,
    @Query("account_id") accountId?: string,
  ) {
    return this.analytics.summary(parseQuery({ from, to, accountId }));
  }

  @Get()
  @Header("Cache-Control", "no-store")
  page(
    @Query("from") from?: string,
    @Query("to") to?: string,
    @Query("account_id") accountId?: string,
    @Query("cursor") cursor?: string,
    @Query("limit") limit?: string,
  ) {
    return this.analytics.page(parseQuery({ from, to, accountId, cursor, limit }));
  }

  @Get("logical/:id")
  @Header("Cache-Control", "no-store")
  logical(@Param("id") id: string) {
    if (!/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(id)) {
      throw new BadRequestException("invalid logical request ID");
    }
    return this.analytics.logical(id);
  }
}

function parseQuery(input: Record<string, string | undefined>): RequestAnalyticsQuery {
  const parsed = querySchema.safeParse(input);
  if (!parsed.success) throw new BadRequestException("invalid request analytics query");
  return {
    from: parsed.data.from,
    to: parsed.data.to,
    ...(parsed.data.accountId === undefined ? {} : { accountId: parsed.data.accountId }),
    ...(parsed.data.cursor === undefined ? {} : { cursor: parsed.data.cursor }),
    ...(parsed.data.limit === undefined ? {} : { limit: parsed.data.limit }),
  };
}
