import type { EngineSpendPeriod, EngineSpendStats } from "@claude-api/contracts";
import type { AdminEngineAccountOwner } from "@claude-api/db";

// Чистая часть эндпоинта «Расход движка»: движок в /spend-stats знает только account/handle,
// коммерция — только своих клиентов. Здесь они склеиваются, чтобы админка могла отделить
// расход клиентов от расхода аккаунтов без commerce-юзера (OpenKeys, внутренние/служебные).

export type EngineSpendAccountClass = "client" | "openkeys" | "internal";
export type EngineSpendWindow = 1 | 7 | 30;

export interface EngineSpendAccountRow {
  account: string;
  handle: string | null;
  account_class: EngineSpendAccountClass;
  owner: { user_id: string; email: string; customer_type: "b2c" | "b2b" | null } | null;
  requests: number;
  charge_usd: number;
  real_usd: number;
  last_ts: number;
}

export interface EngineSpendClassTotals {
  accounts: number;
  requests: number;
  charge_usd: number;
  real_usd: number;
}

/** Аккаунты портала OpenKeys узнаются только по handle — другого признака у движка нет. */
export function isOpenkeysHandle(handle: string | null | undefined): boolean {
  return /^openkeys-/i.test(String(handle ?? ""));
}

export function classifyEngineAccount(
  handle: string | null | undefined,
  owner: AdminEngineAccountOwner | undefined,
): EngineSpendAccountClass {
  if (owner) return "client";
  return isOpenkeysHandle(handle) ? "openkeys" : "internal";
}

const round2 = (value: number): number => Math.round(value * 100) / 100;

export function engineSpendWindowKey(days: EngineSpendWindow): "d1" | "d7" | "d30" {
  return days === 1 ? "d1" : days === 7 ? "d7" : "d30";
}

function emptyTotals(): EngineSpendClassTotals {
  return { accounts: 0, requests: 0, charge_usd: 0, real_usd: 0 };
}

export function buildEngineSpendWindow(
  period: EngineSpendPeriod,
  owners: ReadonlyMap<string, AdminEngineAccountOwner>,
): {
  requests: number;
  charge_usd: number;
  real_usd: number;
  providers: EngineSpendStats["periods"]["d1"]["providers"];
  models: EngineSpendStats["periods"]["d1"]["models"];
  accounts: EngineSpendAccountRow[];
  by_class: Record<EngineSpendAccountClass, EngineSpendClassTotals>;
} {
  const byClass: Record<EngineSpendAccountClass, EngineSpendClassTotals> = {
    client: emptyTotals(),
    openkeys: emptyTotals(),
    internal: emptyTotals(),
  };
  const accounts = period.accounts.map((account): EngineSpendAccountRow => {
    const owner = owners.get(account.account);
    const accountClass = classifyEngineAccount(account.handle, owner);
    const bucket = byClass[accountClass];
    bucket.accounts += 1;
    bucket.requests += account.requests;
    bucket.charge_usd = round2(bucket.charge_usd + account.charge_usd);
    bucket.real_usd = round2(bucket.real_usd + account.real_usd);
    return {
      account: account.account,
      handle: account.handle,
      account_class: accountClass,
      owner: owner
        ? { user_id: owner.userId, email: owner.email, customer_type: owner.customerType }
        : null,
      requests: account.requests,
      charge_usd: account.charge_usd,
      real_usd: account.real_usd,
      last_ts: account.last_ts,
    };
  });
  return {
    requests: period.requests,
    charge_usd: period.charge_usd,
    real_usd: period.real_usd,
    providers: period.providers,
    models: period.models,
    accounts,
    by_class: byClass,
  };
}
