// Типы payload'ов и чистые хелперы вкладки «Финансы» — порт 1:1 секции finance()
// из crates/server/src/admin-panel.js. Все поля опциональны: панель деградирует
// молча (catch → null, пропуски → "—"). Деньги — легаси-поля коммерции в долларах
// (*_usd, только отображение через money()) и nanoUSD-строки (*_nano → nanoMoney).

// GET /admin/finance/overview
export interface FinanceOverview {
  revenue_30d_usd?: number;
  revenue_prev_30d_usd?: number;
  revenue_delta_pct?: number | null;
  arppu_30d_usd?: number | null;
  arpu_30d_usd?: number | null;
  avg_check_30d_usd?: number | null;
  paying_users_30d?: number;
  paying_share_pct?: number | null;
  active_users_30d?: number;
  payments_30d_count?: number;
  customer_classes?: { customer_class?: string; users?: number }[];
}

// GET /admin/finance/revenue?days=N
export interface RevenuePoint {
  day?: string;
  total_usd?: number | string;
  /** nanoUSD-значения по провайдерам (легаси делит на 1e9 только для графика). */
  by_provider?: Record<string, number | string | null>;
}

export interface FinanceRevenue {
  series?: RevenuePoint[];
  totals?: {
    total_usd?: number;
    payments_count?: number;
    by_provider?: Record<string, unknown>;
  };
}

// GET /admin/finance/funnel?days=30
export interface FunnelProviderRow {
  provider?: string;
  created?: number;
  paid?: number;
  conversion_pct?: number | null;
  avg_seconds_to_pay?: number | null;
  avg_check_usd?: number | null;
}

export interface FinanceFunnel {
  totals?: {
    created?: number;
    paid?: number;
    pending?: number;
    canceled?: number;
    failed?: number;
    expired?: number;
    conversion_pct?: number | null;
    avg_seconds_to_pay?: number | null;
    avg_check_usd?: number | null;
  };
  by_provider?: FunnelProviderRow[];
}

// GET /admin/finance/top-customers?days=30&limit=20
export interface TopCustomer {
  email?: string;
  user_id?: string;
  total_usd?: number;
  spent_usd?: number;
  payments_count?: number;
  share_pct?: number | null;
}

export interface FinanceTopCustomers {
  topups?: TopCustomer[];
  spend?: TopCustomer[];
  totals?: { topups_usd?: number; spend_usd?: number };
}

// GET /admin/refunds?limit=25&offset=N
export interface RefundRow {
  email?: string;
  user_id?: string;
  provider?: string;
  amount_usd?: number;
  status?: string;
  adjustment_status?: string | null;
  adjustment_confirmed_at?: string | null;
  adjustment_last_error?: string | null;
  paid_at?: string;
  updated_at?: string;
  provider_payment_id?: string;
}

export interface AdminRefunds {
  rows?: RefundRow[];
  total?: number;
  page_amount_usd?: number;
  total_amount_usd?: number;
}

// GET /admin/finance/cohorts?weeks=8
export interface FinanceCohorts {
  cohorts?: {
    week?: string;
    registered?: number;
    paid_share_pct?: number | null;
    paid_users?: number;
    median_days_to_first_payment?: number | null;
    revenue_usd?: number;
  }[];
}

// GET /admin/finance/churn-signals?days=14
export interface FinanceChurn {
  rows?: {
    email?: string;
    user_id?: string;
    last_seen_at?: string;
    last_paid_at?: string;
    spent_30d_usd?: number;
  }[];
}

// GET /admin/pipeline-health
export interface PipelineHealth {
  verdict?: string;
  verdict_reasons?: string[];
  engine_credits?: {
    stuck_nano?: string | null;
    dead_count?: number;
    oldest_unconfirmed_age_seconds?: number | null;
    counts_by_status?: Record<string, number | undefined>;
  };
  webhook_events?: {
    failed_24h?: number;
    failed_total?: number;
    recent_failures?: {
      provider?: string;
      event_type?: string;
      attempts?: number;
      received_at?: string;
      last_error?: string;
    }[];
  };
  email_outbox?: {
    failed_total?: number;
    recent_failures?: { template?: string; attempts?: number; last_error?: string }[];
  };
  engine_pricing_jobs?: {
    retry_count?: number;
    oldest_unconfirmed_age_seconds?: number | null;
    counts_by_status?: Record<string, number | undefined>;
    recent_errors?: {
      reason?: string;
      user_id?: string;
      engine_account_id?: string;
      status?: string;
      attempts?: number;
      last_error?: string;
    }[];
  };
}

// GET /settlement-health
export interface SettlementHealth {
  backlog_threshold_secs?: number;
  outbox?: {
    pending?: number;
    backlog?: number;
    failed_24h?: number;
    failed?: number;
    pending_with_error?: number;
    done?: number;
    oldest_unsettled_age_secs?: number | null;
    recent_failed?: {
      request_id?: string;
      actual_usd?: number;
      attempts?: number;
      updated_ts?: number;
      last_error?: string;
    }[];
  };
  pricing_consumer?: { unacked?: number; oldest_unacked_age_secs?: number | null };
}

// Окна графика выручки — financeWindows из admin-panel.js.
export const FINANCE_WINDOWS: ReadonlyArray<readonly [number, string]> = [
  [7, "7 дней"],
  [30, "30 дней"],
  [90, "90 дней"],
];

// refundPage.limit из admin-panel.js.
export const REFUND_PAGE_LIMIT = 25;

const TIER_NAMES: Record<string, string> = {
  b2c: "B2C",
  b2b: "B2B",
};

// customerClassName: класс клиента в читаемый ярлык сводки (тир-лестница retired 2026-08-04).
export function customerClassName(key: string): string {
  return TIER_NAMES[key] ?? key;
}

// plainBar: округление и зажим процента в 0–100 (как в admin-panel.js).
export function clampPercent(value: number | string | null | undefined): number {
  return Math.min(100, Math.max(0, Math.round(Number(value) || 0)));
}

// Доля стадии воронки от созданных чекаутов, один знак после запятой (stage() в легаси).
export function funnelShare(value: number | null | undefined, created: number): number {
  return created ? Math.round((Number(value || 0) / created) * 1000) / 10 : 0;
}

// Откат ушедшей за пределы страницы возвратов на последнюю (как в finance() легаси):
// null — откат не нужен, иначе новый offset.
export function clampRefundOffset(offset: number, limit: number, total: number | undefined): number | null {
  if (total == null || total <= 0 || offset < total) return null;
  return Math.max(0, Math.floor((total - 1) / limit) * limit);
}

// Точка/серия SVG-графика (формат lineChart из admin-panel.js).
export type ChartPoint = { ts: number; value: number | null };
export type ChartSeries = { label: string; color?: string; points: ChartPoint[] };

// Серии графика «Выручка по дням»: основная линия ($/день) + по линии на провайдера
// из totals.by_provider; отсутствующее значение провайдера за день — разрыв (null).
export function buildRevenueSeries(revenue: FinanceRevenue): ChartSeries[] {
  const rows = revenue.series ?? [];
  const providers = Object.keys(revenue.totals?.by_provider ?? {});
  const dayTs = (day: string | undefined) => Date.parse((day ?? "") + "T00:00:00Z") / 1000;
  const main: ChartSeries = {
    label: "выручка $/день",
    points: rows.map((point) => ({ ts: dayTs(point.day), value: Number(point.total_usd) })),
  };
  return [main].concat(
    providers.map((name) => ({
      label: name,
      points: rows.map((point) => ({
        ts: dayTs(point.day),
        value:
          point.by_provider && point.by_provider[name] != null ? Number(point.by_provider[name]) / 1e9 : null,
      })),
    })),
  );
}
