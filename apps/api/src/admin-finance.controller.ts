import {
  BadRequestException,
  Controller,
  Get,
  Header,
  Query,
  UseGuards,
} from "@nestjs/common";
import { z } from "zod";
import { AdminGuard } from "./admin.guard.js";
import { AdminFinanceService } from "./admin-finance.service.js";

const revenueDaysSchema = z.coerce.number().int().refine(
  (value) => [7, 30, 90].includes(value),
).default(30);
const windowDaysSchema = z.coerce.number().int().min(1).max(365).default(30);
const churnDaysSchema = z.coerce.number().int().min(1).max(90).default(14);
const weeksSchema = z.coerce.number().int().min(1).max(26).default(8);
const topLimitSchema = z.coerce.number().int().min(1).max(100).default(20);
const limitSchema = z.coerce.number().int().min(1).max(500).default(50);
const offsetSchema = z.coerce.number().int().min(0).max(1_000_000).default(0);
const engineSpendDaysSchema = z.enum(["1", "7", "30"]).default("30")
  .transform((value) => Number(value) as 1 | 7 | 30);
const payingUsersSchema = z.object({  days: z.enum(["1", "7", "30"]).default("30").transform((value) => Number(value) as 1 | 7 | 30),
  limit: z.coerce.number().int().min(1).max(100).default(50),
  offset: z.coerce.number().int().min(0).max(1_000_000).default(0),
  q: z.string().trim().max(200).optional(),
  status: z.enum(["active", "disabled"]).optional(),
  provider: z.enum(["anthropic", "openai", "google", "other"]).optional(),
  funding: z.enum(["payments", "manual", "bonus", "all", "spenders"]).optional(),
  includeUsage: z.enum(["true", "false"]).optional().transform((value) => value === "true"),
  sort: z.enum(["spent", "paid", "last_paid", "last_seen"]).default("spent"),
  dir: z.enum(["asc", "desc"]).default("desc"),
});

@Controller("admin")
@UseGuards(AdminGuard)
export class AdminFinanceController {
  constructor(private readonly finance: AdminFinanceService) {}

  @Get("finance/overview")
  @Header("Cache-Control", "no-store")
  overview(): Promise<Record<string, unknown>> {
    return this.finance.overview();
  }

  @Get("finance/revenue")
  @Header("Cache-Control", "no-store")
  revenue(@Query("days") value?: string): Promise<Record<string, unknown>> {
    return this.finance.revenue(parseWith(revenueDaysSchema, value, "days must be one of 7, 30, 90"));
  }

  @Get("finance/funnel")
  @Header("Cache-Control", "no-store")
  funnel(@Query("days") value?: string): Promise<Record<string, unknown>> {
    return this.finance.funnel(parseWith(windowDaysSchema, value, "days must be an integer from 1 to 365"));
  }

  @Get("finance/top-customers")
  @Header("Cache-Control", "no-store")
  topCustomers(
    @Query("days") daysValue?: string,
    @Query("limit") limitValue?: string,
  ): Promise<Record<string, unknown>> {
    return this.finance.topCustomers(
      parseWith(windowDaysSchema, daysValue, "days must be an integer from 1 to 365"),
      parseWith(topLimitSchema, limitValue, "limit must be an integer from 1 to 100"),
    );
  }

  @Get("finance/paying-users")
  @Header("Cache-Control", "no-store")
  payingUsers(
    @Query("days") days?: string,
    @Query("limit") limit?: string,
    @Query("offset") offset?: string,
    @Query("q") q?: string,
    @Query("status") status?: string,
    @Query("provider") provider?: string,
    @Query("sort") sort?: string,
    @Query("dir") dir?: string,
    @Query("funding") funding?: string,
    @Query("include_usage") includeUsage?: string,
  ): Promise<Record<string, unknown>> {
    const parsed = payingUsersSchema.safeParse({
      days, limit, offset, q, status, provider, sort, dir, funding, includeUsage,
    });
    if (!parsed.success) throw new BadRequestException("invalid paying users filters");
    return this.finance.payingUsers({
      days: parsed.data.days,
      limit: parsed.data.limit,
      offset: parsed.data.offset,
      sort: parsed.data.sort,
      dir: parsed.data.dir,
      ...(parsed.data.q === undefined ? {} : { q: parsed.data.q }),
      ...(parsed.data.status === undefined ? {} : { status: parsed.data.status }),
      ...(parsed.data.provider === undefined ? {} : { provider: parsed.data.provider }),
      ...(parsed.data.funding === undefined ? {} : { funding: parsed.data.funding }),
      ...(parsed.data.includeUsage ? { includeUsage: true } : {}),
    });
  }

  @Get("finance/engine-spend")
  @Header("Cache-Control", "no-store")
  engineSpend(@Query("days") value?: string): Promise<Record<string, unknown>> {
    const parsed = engineSpendDaysSchema.safeParse(value);
    if (!parsed.success) throw new BadRequestException("days must be one of 1, 7, 30");
    return this.finance.engineSpend(parsed.data);
  }

  @Get("finance/cohorts")
  @Header("Cache-Control", "no-store")
  cohorts(@Query("weeks") value?: string): Promise<Record<string, unknown>> {
    return this.finance.cohorts(parseWith(weeksSchema, value, "weeks must be an integer from 1 to 26"));
  }

  @Get("finance/churn-signals")
  @Header("Cache-Control", "no-store")
  churnSignals(
    @Query("days") daysValue?: string,
    @Query("limit") limitValue?: string,
  ): Promise<Record<string, unknown>> {
    return this.finance.churnSignals(
      parseWith(churnDaysSchema, daysValue, "days must be an integer from 1 to 90"),
      parseWith(limitSchema, limitValue, "limit must be an integer from 1 to 500"),
    );
  }

  @Get("refunds")
  @Header("Cache-Control", "no-store")
  refunds(
    @Query("limit") limitValue?: string,
    @Query("offset") offsetValue?: string,
  ): Promise<Record<string, unknown>> {
    return this.finance.refunds(
      parseWith(limitSchema, limitValue, "limit must be an integer from 1 to 500"),
      parseWith(offsetSchema, offsetValue, "offset must be a non-negative integer"),
    );
  }
}

function parseWith<Output, Input>(
  schema: z.ZodType<Output, z.ZodTypeDef, Input>,
  value: string | undefined,
  message: string,
): Output {
  const parsed = schema.safeParse(value);
  if (!parsed.success) throw new BadRequestException(message);
  return parsed.data;
}
