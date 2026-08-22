"use client";

// Аккаунты — порт 1:1 функции accounts() из crates/server/src/admin-panel.js
// (строки 477-506): единый реестр engine, commerce, partner и CRM-аккаунтов.
// Источники: /overview (engine), /admin/dashboard (счётчик commerce),
// /partner-admin/partner-analytics?sort=created_at&dir=desc (пейджер по 50),
// /openkeys-admin/lookup (подписи OpenKeys, ленивый кэш на сессию).
// Источники обновляются по SSE-prefixes; общий freshness-bridge страхует потерянные события.
//
// POST /admin/accounts/query здесь не вызывается: в легаси его из браузера
// дергает не панель, а коммерческий бэкенд (packages/engine-client) для
// server-side снапшотов; у вкладки «Аккаунты» такого запроса нет.
import Link from "next/link";
import { startTransition, useEffect, useMemo, useState, type ReactElement } from "react";
import { useResources } from "@/lib/resources";
import { ago, count, money, nanoMoney } from "@/lib/format";
import type { CommerceDashboard } from "@/lib/types";
import { Banner, EmptyRow, LoadingGrid, PageHead, Pill, SectionHeader, TableCard } from "@/components/ui";
import { isOpenkeys, useSpendStatsModal, type OkDirectoryRow } from "@/components/spend-stats-modal";
import { OkInfo } from "./ok-directory";

const PARTNER_LIMIT = 50;

// GET /overview — поля engine-аккаунта, которые показывает эта страница.
interface EngineAccountRow {
  account?: string;
  handle?: string;
  status?: string;
  balance_usd?: number;
  spent_usd?: number;
  mult?: number | string;
}

// GET /partner-admin/partner-analytics — постраничная аналитика партнёров.
interface PartnerAnalyticsItem {
  id?: string;
  referralCode?: string;
  telegramUsername?: string;
  email?: string;
  displayName?: string;
  status?: string;
  referredUsers?: number;
  depositsTotalNano?: string;
  earnedTotalNano?: string;
  lastSeenAt?: string;
}

interface PartnerAnalyticsResponse {
  items?: PartnerAnalyticsItem[];
  totals?: { total?: number };
}

interface AccountsData {
  overview: { accounts?: EngineAccountRow[] };
  dashboard: CommerceDashboard;
  partners: PartnerAnalyticsResponse;
  directory: { rows?: OkDirectoryRow[] };
}

const isCrmAccount = (handle: string | undefined): boolean => String(handle ?? "").toLowerCase() === "crm-parsing";

// Email — основной идентификатор аккаунта; displayName и Telegram остаются fallback для legacy-строк.
export function partnerName(partner: PartnerAnalyticsItem): string {
  return partner.email || partner.displayName || (partner.telegramUsername ? `@${partner.telegramUsername}` : "—");
}

// Зажатие пейджера партнёров: если список сократился и текущий offset вышел
// за хвост, переходим на последнюю страницу и грузим её заново (как accounts()
// в легаси: partnerOffset>=partnerTotal && partnerTotal>0 → пересчёт и re-fetch).
export function clampPartnerOffset(offset: number, total: number): number {
  if (total <= 0 || offset < total) return offset;
  return Math.max(0, Math.floor((total - 1) / PARTNER_LIMIT) * PARTNER_LIMIT);
}

function partnerStatusKind(status: string | undefined): "ok" | "bad" | "warn" {
  if (status === "active") return "ok";
  if (status === "suspended") return "bad";
  return "warn";
}

export default function AccountsPage(): ReactElement {
  const [partnerOffset, setPartnerOffset] = useState(0);
  const { data, isLoading } = useResources<AccountsData>({
    overview: "/overview",
    dashboard: "/admin/dashboard",
    partners: `/partner-admin/partner-analytics?sort=created_at&dir=desc&limit=${PARTNER_LIMIT}&offset=${partnerOffset}`,
    directory: "/openkeys-admin/lookup",
  });
  const { openSpendStats, spendStatsModal } = useSpendStatsModal();
  const okDir = useMemo(
    () => new Map((data.directory?.rows ?? []).map((row) => [String(row.engineAccountId ?? ""), row])),
    [data.directory],
  );

  const partnerTotal = data?.partners
    ? data.partners.totals?.total || (data.partners.items ?? []).length
    : 0;

  const effectivePartnerOffset = clampPartnerOffset(partnerOffset, partnerTotal);

  useEffect(() => {
    if (data.partners && effectivePartnerOffset !== partnerOffset) {
      startTransition(() => setPartnerOffset(effectivePartnerOffset));
    }
  }, [data.partners, effectivePartnerOffset, partnerOffset]);

  const goToOffset = (next: number): void => {
    startTransition(() => setPartnerOffset(next));
  };

  const engineAccounts = useMemo(() => data?.overview?.accounts ?? [], [data]);
  const crm = engineAccounts.find((account) => isCrmAccount(account.handle));

  const engineRows = useMemo(
    () =>
      engineAccounts.map((account, index) => {
        const domain = isCrmAccount(account.handle) ? "crm.apitoken.sale" : "api.apitoken.sale";
        return (
          <tr key={account.account ?? account.handle ?? index}>
            <td className="left">
              <b>{account.handle || "—"}</b>
              {isOpenkeys(account.handle) ? (
                <span className="okb" title="Выпущен через OpenKeys">
                  OpenKeys
                </span>
              ) : null}
              <div className="sub mono">{account.account ?? ""}</div>
              <OkInfo meta={okDir.get(String(account.account ?? ""))} />
            </td>
            <td className="left">{domain}</td>
            <td>
              <Pill kind={account.status === "active" ? "ok" : "bad"}>{account.status ?? ""}</Pill>
            </td>
            <td>
              <b>{money(account.balance_usd)}</b>
            </td>
            <td>{money(account.spent_usd)}</td>
            <td>×{account.mult ?? ""}</td>
          </tr>
        );
      }),
    [engineAccounts, okDir],
  );

  const partnerItems = useMemo(() => data?.partners?.items ?? [], [data]);
  const partnerRows = useMemo(
    () =>
      partnerItems.map((partner, index) => (
        <tr key={partner.id ?? index}>
          <td className="left">
            <b translate="no">{partnerName(partner)}</b>
            <div className="sub mono" translate="no">
              {partner.id ?? ""} · {partner.referralCode ?? ""}
            </div>
          </td>
          <td>
            <Pill kind={partnerStatusKind(partner.status)}>{partner.status ?? ""}</Pill>
          </td>
          <td>{partner.referredUsers ?? "—"}</td>
          <td>{nanoMoney(partner.depositsTotalNano)}</td>
          <td>{nanoMoney(partner.earnedTotalNano)}</td>
          <td>{ago(partner.lastSeenAt)}</td>
        </tr>
      )),
    [partnerItems],
  );

  if (isLoading && Object.values(data).every((value) => value === undefined)) {
    return (
      <>
        <PageHead title="Аккаунты" sub="данные загружаются, навигация уже доступна" />
        <LoadingGrid />
      </>
    );
  }

  const commerceTotal = data.dashboard?.users?.total || 0;
  const totalRecords = commerceTotal + engineAccounts.length + partnerTotal;

  const domains: Array<[domain: string, label: string, sub: string, url: string | null]> = [
    ["admin.apitoken.sale", "central admin", "commerce + engine + partner account control", null],
    [
      "admin.partners.apitoken.sale",
      "partner admin",
      "unchanged APIToken Partners operator console",
      "https://admin.partners.apitoken.sale/admin",
    ],
    [
      "crm.apitoken.sale",
      "CRM & Parsing",
      crm ? `engine account ${crm.handle} · ${crm.status}` : "engine account crm-parsing is missing",
      "https://crm.apitoken.sale",
    ],
    ["content-studio.apitoken.sale", "content studio", "private editorial workspace", "https://content-studio.apitoken.sale"],
  ];

  return (
    <>
      <PageHead
        title="Аккаунты"
        sub="engine, commerce, partner и CRM в одном реестре"
        badge={<Pill kind={crm ? "ok" : "warn"}>{count(totalRecords, "запись", "записи", "записей")}</Pill>}
      />

      <Banner kind={crm ? "ok" : "warn"} title="Единый реестр аккаунтов">
        commerce {commerceTotal} · engine {engineAccounts.length} · partners {partnerTotal} · CRM{" "}
        {crm ? "connected" : "missing"}
      </Banner>

      <SectionHeader title="Внутренние домены" />
      <div className="domain-grid">
        {domains.map(([domain, label, sub, url]) => (
          <div className="domain" key={domain}>
            <b>
              {url ? (
                <a className="link" target="_blank" rel="noreferrer" href={url}>
                  {domain}
                </a>
              ) : (
                domain
              )}
            </b>
            <Pill kind={domain === "crm.apitoken.sale" ? (crm ? "ok" : "warn") : "info"}>{label}</Pill>
            <div className="sub">{sub}</div>
          </div>
        ))}
      </div>

      <SectionHeader title="Engine и service accounts" sub={String(engineAccounts.length)} />
      <TableCard>
        <table>
          <thead>
            <tr>
              <th className="left">account</th>
              <th className="left">домен</th>
              <th>статус</th>
              <th>баланс</th>
              <th>
                <button type="button" className="table-action" onClick={openSpendStats} title="Разбивка: сутки / 7 дней / 30 дней">
                  потрачено
                </button>
              </th>
              <th>множитель</th>
            </tr>
          </thead>
          <tbody>{engineRows.length ? engineRows : <EmptyRow columns={6} />}</tbody>
        </table>
      </TableCard>

      <SectionHeader title="Partner accounts" sub={String(partnerTotal)} />
      <TableCard>
        <table>
          <thead>
            <tr>
              <th className="left">партнёр</th>
              <th>статус</th>
              <th>рефералы</th>
              <th>депозиты</th>
              <th>заработано</th>
              <th>был(а)</th>
            </tr>
          </thead>
          <tbody>{partnerRows.length ? partnerRows : <EmptyRow columns={6} />}</tbody>
        </table>
      </TableCard>

      <div className="pager">
        <span>
          {partnerTotal ? effectivePartnerOffset + 1 : 0}–{Math.min(effectivePartnerOffset + PARTNER_LIMIT, partnerTotal)} из{" "}
          {partnerTotal}
        </span>
        <button
          type="button"
          className="btn ghost"
          disabled={effectivePartnerOffset <= 0}
          onClick={() => goToOffset(Math.max(0, effectivePartnerOffset - PARTNER_LIMIT))}
        >
          Назад
        </button>
        <button
          type="button"
          className="btn ghost"
          disabled={effectivePartnerOffset + PARTNER_LIMIT >= partnerTotal}
          onClick={() => goToOffset(effectivePartnerOffset + PARTNER_LIMIT)}
        >
          Дальше
        </button>
      </div>

      <footer>
        Полная аналитика партнёров и выплаты — на странице{" "}
        <Link className="link" href="/partners">
          «Партнёры»
        </Link>
        . Все {commerceTotal} commerce-аккаунтов доступны с действиями на странице{" "}
        <Link className="link" href="/users">
          «Пользователи»
        </Link>
        ; полный partner workflow остаётся на admin.partners.apitoken.sale.
      </footer>

      {spendStatsModal}
    </>
  );
}
