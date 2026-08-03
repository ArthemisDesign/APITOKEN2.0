import { Inject, Injectable } from "@nestjs/common";
import {
  getAdminFinanceOverview,
  getAdminFinanceFunnel,
  listAdminFinanceChurnSignals,
  listAdminFinanceCohorts,
  listAdminFinanceRevenueDaily,
  listAdminFinanceTopCustomers,
  listAdminPayingUsers,
  listAdminRefunds,
  type AdminPayingUsersQuery,
  type Database,
} from "@claude-api/db";
import { DATABASE } from "./infrastructure.module.js";
import { nanoToUsd } from "./admin-operations.service.js";

/**
 * Блок «Финансы» админ-панели: read-only агрегаты по commerce PostgreSQL. Деньги в ответах —
 * строки integer nano-USD (инвариант проекта: без float/JS number); рядом кладутся usd-строки
 * через тот же точный nanoToUsd, что у соседних admin-эндпоинтов. Все доли/дельты — null при
 * нулевом знаменателе (никакого NaN/Infinity наружу).
 */
@Injectable()
export class AdminFinanceService {
  constructor(@Inject(DATABASE) private readonly database: Database) {}

  async overview(): Promise<Record<string, unknown>> {
    const value = await getAdminFinanceOverview(this.database);
    const revenue30d = BigInt(value.revenue30dNano);
    const revenuePrev30d = BigInt(value.revenuePrev30dNano);
    const arpu30d = divideNano(revenue30d, value.activeUsers30d);
    const arppu30d = divideNano(revenue30d, value.payingUsers30d);
    const avgCheck30d = divideNano(revenue30d, value.payments30dCount);
    return {
      generated_at: new Date().toISOString(),
      revenue_30d_nano: value.revenue30dNano,
      revenue_30d_usd: nanoToUsd(value.revenue30dNano),
      revenue_prev_30d_nano: value.revenuePrev30dNano,
      revenue_prev_30d_usd: nanoToUsd(value.revenuePrev30dNano),
      revenue_delta_pct: deltaPct(revenue30d, revenuePrev30d),
      payments_30d_count: value.payments30dCount,
      paying_users_30d: value.payingUsers30d,
      active_users_30d: value.activeUsers30d,
      arpu_30d_nano: arpu30d,
      arpu_30d_usd: arpu30d === null ? null : nanoToUsd(arpu30d),
      arppu_30d_nano: arppu30d,
      arppu_30d_usd: arppu30d === null ? null : nanoToUsd(arppu30d),
      paying_share_pct: pctOf(BigInt(value.payingUsers30d), BigInt(value.activeUsers30d)),
      avg_check_30d_nano: avgCheck30d,
      avg_check_30d_usd: avgCheck30d === null ? null : nanoToUsd(avgCheck30d),
      tiers: value.tiers.map((tier) => ({
        tier: tier.customerType === "b2b" ? "b2b" : `b2c_tier_${tier.tier ?? "?"}`,
        users: tier.users,
      })),
    };
  }

  async revenue(days: number): Promise<Record<string, unknown>> {
    const rows = await listAdminFinanceRevenueDaily(this.database, days);
    const byDay = new Map<string, {
      total: bigint;
      count: number;
      providers: Map<string, bigint>;
    }>();
    const totalsProviders = new Map<string, bigint>();
    let total = 0n;
    let totalCount = 0;
    for (const row of rows) {
      const amount = BigInt(row.totalNano);
      const day = byDay.get(row.day) ?? { total: 0n, count: 0, providers: new Map<string, bigint>() };
      day.total += amount;
      day.count += row.paymentsCount;
      day.providers.set(row.provider, (day.providers.get(row.provider) ?? 0n) + amount);
      byDay.set(row.day, day);
      totalsProviders.set(row.provider, (totalsProviders.get(row.provider) ?? 0n) + amount);
      total += amount;
      totalCount += row.paymentsCount;
    }
    const series = [...byDay.entries()].map(([day, value]) => ({
      day,
      total_nano: value.total.toString(),
      total_usd: nanoToUsd(value.total.toString()),
      payments_count: value.count,
      by_provider: Object.fromEntries([...value.providers.entries()].map(([provider, nano]) => [
        provider,
        nano.toString(),
      ])),
    }));
    return {
      days,
      series,
      totals: {
        total_nano: total.toString(),
        total_usd: nanoToUsd(total.toString()),
        payments_count: totalCount,
        by_provider: Object.fromEntries([...totalsProviders.entries()].map(([provider, nano]) => [
          provider,
          nano.toString(),
        ])),
      },
    };
  }

  async funnel(days: number): Promise<Record<string, unknown>> {
    const rows = await getAdminFinanceFunnel(this.database, days);
    const serialize = (row: {
      created: number; paid: number; canceled: number; failed: number; expired: number;
      pending: number; avgSecondsToPay: number | null; paidNano: string;
    }) => {
      const avgCheck = divideNano(BigInt(row.paidNano), row.paid);
      return {
        created: row.created,
        paid: row.paid,
        canceled: row.canceled,
        failed: row.failed,
        expired: row.expired,
        pending: row.pending,
        conversion_pct: pctOf(BigInt(row.paid), BigInt(row.created)),
        avg_seconds_to_pay: row.avgSecondsToPay === null ? null : Math.round(row.avgSecondsToPay),
        avg_check_nano: avgCheck,
        avg_check_usd: avgCheck === null ? null : nanoToUsd(avgCheck),
        paid_nano: row.paidNano,
        paid_usd: nanoToUsd(row.paidNano),
      };
    };
    const totals = {
      created: 0, paid: 0, canceled: 0, failed: 0, expired: 0, pending: 0,
      paidNano: "0",
      avgSecondsToPay: null as number | null,
    };
    let paidTimed = 0;
    let weightedSeconds = 0;
    let paidNano = 0n;
    for (const row of rows) {
      totals.created += row.created;
      totals.paid += row.paid;
      totals.canceled += row.canceled;
      totals.failed += row.failed;
      totals.expired += row.expired;
      totals.pending += row.pending;
      paidNano += BigInt(row.paidNano);
      if (row.avgSecondsToPay !== null && row.paidTimed > 0) {
        paidTimed += row.paidTimed;
        weightedSeconds += row.avgSecondsToPay * row.paidTimed;
      }
    }
    totals.paidNano = paidNano.toString();
    totals.avgSecondsToPay = paidTimed > 0 ? weightedSeconds / paidTimed : null;
    return {
      days,
      totals: serialize(totals),
      by_provider: rows.map((row) => ({ provider: row.provider, ...serialize(row) })),
    };
  }

  async topCustomers(days: number, limit: number): Promise<Record<string, unknown>> {
    const value = await listAdminFinanceTopCustomers(this.database, days, limit);
    const topupsTotal = BigInt(value.topupsTotalNano);
    const spendTotal = BigInt(value.spendTotalNano);
    return {
      days,
      limit,
      topups: value.topups.map((row) => ({
        user_id: row.userId,
        email: row.email,
        total_nano: row.totalNano,
        total_usd: nanoToUsd(row.totalNano),
        payments_count: row.paymentsCount,
        share_pct: pctOf(BigInt(row.totalNano), topupsTotal),
      })),
      spend: value.spend.map((row) => ({
        user_id: row.userId,
        email: row.email,
        spent_nano: row.spentNano,
        spent_usd: nanoToUsd(row.spentNano),
        share_pct: pctOf(BigInt(row.spentNano), spendTotal),
      })),
      totals: {
        topups_nano: value.topupsTotalNano,
        topups_usd: nanoToUsd(value.topupsTotalNano),
        spend_nano: value.spendTotalNano,
        spend_usd: nanoToUsd(value.spendTotalNano),
      },
    };
  }

  async payingUsers(query: AdminPayingUsersQuery): Promise<Record<string, unknown>> {
    const value = await listAdminPayingUsers(this.database, query);
    const providerMoney = (amounts: Record<"anthropic" | "openai" | "google" | "other", string>) => ({
      anthropic_nano: amounts.anthropic,
      openai_nano: amounts.openai,
      google_nano: amounts.google,
      other_nano: amounts.other,
    });
    return {
      generated_at: new Date().toISOString(),
      days: value.days,
      total: value.total,
      limit: value.limit,
      offset: value.offset,
      summary: {
        paying_users: value.summary.payingUsers,
        active_spenders: value.summary.activeSpenders,
        paid_nano: value.summary.paidNano,
        spent_nano: value.summary.spentNano,
        provider_spend: providerMoney(value.summary.providerSpendNano),
        provider_users: {
          anthropic: value.summary.providerUsers.anthropic,
          openai: value.summary.providerUsers.openai,
          google: value.summary.providerUsers.google,
          other: value.summary.providerUsers.other,
        },
      },
      rows: value.rows.map((row) => ({
        user_id: row.userId,
        email: row.email,
        display_name: row.displayName,
        status: row.status,
        customer_type: row.customerType,
        tier: row.tier,
        multiplier_bp: row.multiplierBp,
        paid_nano: row.paidNano,
        payments_count: row.paymentsCount,
        last_paid_at: row.lastPaidAt?.toISOString() ?? null,
        spent_nano: row.spentNano,
        provider_spend: providerMoney(row.providerSpendNano),
        active_api_keys: row.activeApiKeys,
        last_seen_at: row.lastSeenAt?.toISOString() ?? null,
        created_at: row.createdAt.toISOString(),
      })),
    };
  }

  async refunds(limit: number, offset: number): Promise<Record<string, unknown>> {
    const value = await listAdminRefunds(this.database, limit, offset);
    let pageAmount = 0n;
    for (const row of value.rows) pageAmount += BigInt(row.amountNano);
    return {
      rows: value.rows.map((row) => ({
        id: row.id,
        user_id: row.userId,
        email: row.email,
        provider: row.provider,
        provider_payment_id: row.providerPaymentId,
        amount_nano: row.amountNano,
        amount_usd: nanoToUsd(row.amountNano),
        currency: row.currency,
        status: row.status,
        paid_at: row.paidAt?.toISOString() ?? null,
        updated_at: row.updatedAt.toISOString(),
      })),
      total: value.total,
      limit,
      offset,
      page_amount_nano: pageAmount.toString(),
      page_amount_usd: nanoToUsd(pageAmount.toString()),
      total_amount_nano: value.totalNano,
      total_amount_usd: nanoToUsd(value.totalNano),
    };
  }

  async cohorts(weeks: number): Promise<Record<string, unknown>> {
    const rows = await listAdminFinanceCohorts(this.database, weeks);
    return {
      weeks,
      cohorts: rows.map((row) => ({
        week: row.week,
        registered: row.registered,
        paid_users: row.paidUsers,
        paid_share_pct: pctOf(BigInt(row.paidUsers), BigInt(row.registered)),
        median_days_to_first_payment: row.medianDaysToFirstPayment === null
          ? null
          : Math.round(row.medianDaysToFirstPayment * 10) / 10,
        revenue_nano: row.revenueNano,
        revenue_usd: nanoToUsd(row.revenueNano),
      })),
    };
  }

  async churnSignals(days: number, limit: number): Promise<Record<string, unknown>> {
    const rows = await listAdminFinanceChurnSignals(this.database, days, limit);
    return {
      days,
      rows: rows.map((row) => ({
        user_id: row.userId,
        email: row.email,
        last_seen_at: row.lastSeenAt?.toISOString() ?? null,
        last_paid_at: row.lastPaidAt?.toISOString() ?? null,
        spent_30d_nano: row.spent30dNano,
        spent_30d_usd: nanoToUsd(row.spent30dNano),
      })),
    };
  }
}

/** Доля числителя от знаменателя в процентах с 1 знаком; null при нулевом знаменателе. */
function pctOf(part: bigint, total: bigint): number | null {
  if (total <= 0n) return null;
  return Number((part * 1000n) / total) / 10;
}

/** Дельта текущего периода к предыдущему в процентах с 1 знаком; null, если база нулевая. */
function deltaPct(current: bigint, previous: bigint): number | null {
  if (previous <= 0n) return null;
  return Number(((current - previous) * 1000n) / previous) / 10;
}

/** Целочисленное деление nano-суммы на счётчик; null при нулевом знаменателе. */
function divideNano(total: bigint, count: number): string | null {
  if (count <= 0) return null;
  return (total / BigInt(count)).toString();
}
