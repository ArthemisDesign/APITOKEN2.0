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
