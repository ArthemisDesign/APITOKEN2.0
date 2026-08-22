"use client";

import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { useDeferredValue, useMemo } from "react";
import { Banner, EmptyRow, LoadingGrid, PageHead, Pill, TableCard } from "@/components/ui";
import { nanoMoney } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { useResource } from "@/lib/resources";
import type { AdminPartner } from "../types";

type StatusFilter = "all" | AdminPartner["status"];
type SortKey = "created" | "payable" | "team" | "referrals";

function identity(partner: AdminPartner): string {
  return partner.email ?? partner.displayName ?? (partner.telegramUsername ? `@${partner.telegramUsername}` : `partner-${partner.id.slice(0, 8)}`);
}

function statusLabel(status: AdminPartner["status"], t: (en: string, ru: string) => string): string {
  if (status === "active") return t("Active", "Активен");
  if (status === "suspended") return t("Suspended", "Приостановлен");
  return t("Pending", "Ожидает");
}

export default function PartnerDirectoryPage() {
  const { t } = useI18n();
  const router = useRouter();
  const search = useSearchParams();
  const { data, isLoading } = useResource<{ items: AdminPartner[] }>("/partner-admin/partners");
  const query = search.get("q") ?? "";
  const statusParam = search.get("status");
  const status: StatusFilter = statusParam === "active" || statusParam === "suspended" || statusParam === "pending" ? statusParam : "all";
  const sortParam = search.get("sort");
  const sort: SortKey = sortParam === "payable" || sortParam === "team" || sortParam === "referrals" ? sortParam : "created";
  const deferredQuery = useDeferredValue(query.trim().toLowerCase());

  function replaceFilters(next: { query?: string; status?: StatusFilter; sort?: SortKey }) {
    const nextQuery = next.query ?? query;
    const nextStatus = next.status ?? status;
    const nextSort = next.sort ?? sort;
    const params = new URLSearchParams();
    if (nextQuery.trim()) params.set("q", nextQuery);
    if (nextStatus !== "all") params.set("status", nextStatus);
    if (nextSort !== "created") params.set("sort", nextSort);
    router.replace(`/partners/directory${params.size ? `?${params}` : ""}`, { scroll: false });
  }

  const items = useMemo(() => {
    const filtered = (data?.items ?? []).filter((partner) => {
      if (status !== "all" && partner.status !== status) return false;
      if (!deferredQuery) return true;
      return [partner.email, partner.displayName, partner.telegramUsername, partner.referralCode, partner.parentEmail]
        .some((value) => value?.toLowerCase().includes(deferredQuery));
    });
    return filtered.sort((left, right) => {
      if (sort === "payable") {
        const leftValue = BigInt(left.payableNano);
        const rightValue = BigInt(right.payableNano);
        return rightValue === leftValue ? 0 : rightValue > leftValue ? 1 : -1;
      }
      if (sort === "team") return right.teamSize - left.teamSize;
      if (sort === "referrals") return right.referredUsers - left.referredUsers;
      return right.createdAt.localeCompare(left.createdAt);
    });
  }, [data, deferredQuery, sort, status]);

  if (isLoading && !data) return <><PageHead title={t("Partner directory", "Партнёры")} sub={t("Loading authority records", "Загружаем права и условия")} /><LoadingGrid label={t("Loading partner directory", "Загрузка списка партнёров")} /></>;

  return <>
    <PageHead title={t("Partner directory", "Партнёры")} sub={t(
      "Commission terms, Team ceilings, B2B authority and operational balances",
      "Комиссии, лимиты Team, B2B-права и операционные балансы",
    )} badge={<Pill kind="info">{items.length} / {data?.items.length ?? 0}</Pill>} />
    {!data ? <Banner kind="bad" title={t("Directory unavailable", "Список недоступен")}>/partner-admin/partners</Banner> : null}
    <div className="partner-directory-toolbar">
      <label className="field"><span>{t("Search", "Поиск")}</span><input name="partnerSearch" type="search" autoComplete="off" spellCheck={false} value={query} onChange={(event) => replaceFilters({ query: event.target.value })} placeholder={t("Email, name, Telegram or code…", "Email, имя, Telegram или код…")} /></label>
      <label className="field"><span>{t("Status", "Статус")}</span><select name="partnerStatus" value={status} onChange={(event) => replaceFilters({ status: event.target.value as StatusFilter })}><option value="all">{t("All", "Все")}</option><option value="active">{t("Active", "Активные")}</option><option value="suspended">{t("Suspended", "Приостановленные")}</option><option value="pending">{t("Pending", "Ожидающие")}</option></select></label>
      <label className="field"><span>{t("Sort", "Сортировка")}</span><select name="partnerSort" value={sort} onChange={(event) => replaceFilters({ sort: event.target.value as SortKey })}><option value="created">{t("Newest", "Новые")}</option><option value="payable">{t("Payable", "К выплате")}</option><option value="team">{t("Team size", "Размер команды")}</option><option value="referrals">{t("Referrals", "Рефералы")}</option></select></label>
    </div>
    <TableCard><table className="partner-directory-table"><thead><tr>
      <th className="left">{t("Partner", "Партнёр")}</th><th>{t("Status", "Статус")}</th><th>{t("Direct", "Прямая")}</th><th>{t("Team override max", "Макс. надбавка")}</th><th>{t("B2B authority", "B2B-права")}</th><th>{t("Team", "Команда")}</th><th>{t("Referrals", "Рефералы")}</th><th>{t("Payable", "К выплате")}</th><th><span className="sr-only">{t("Open", "Открыть")}</span></th>
    </tr></thead><tbody>
      {items.length ? items.map((partner) => <tr key={partner.id}>
        <td className="left"><Link className="partner-email-link" href={`/partners/${partner.id}`} title={identity(partner)} translate="no">{identity(partner)}</Link>{partner.email && partner.telegramUsername ? <div className="sub" translate="no">@{partner.telegramUsername}</div> : null}<div className="sub mono" translate="no">{partner.referralCode}{partner.parentEmail ? ` · ↳ ${partner.parentEmail}` : ""}</div></td>
        <td><Pill kind={partner.status === "active" ? "ok" : partner.status === "suspended" ? "bad" : "warn"}>{statusLabel(partner.status, t)}</Pill></td>
        <td>{formatBps(partner.commissionBps)}</td><td>{formatBps(partner.teamOverrideMaxBps)}</td>
        <td><div className="permission-stack"><span>{partner.b2bEnabled ? `≤ ${formatBps(partner.b2bMaxDiscountBps)}` : "—"}</span><small>{partner.b2bCanDelegate ? t("may delegate", "делегирует") : t("no delegation", "без делегирования")}</small></div></td>
        <td>{partner.teamSize}<div className="sub">{partner.teamInvitesEnabled ? t("invites on", "инвайты вкл") : t("invites off", "инвайты выкл")}</div></td><td>{partner.referredUsers}</td><td><b>{nanoMoney(partner.payableNano)}</b>{BigInt(partner.debtNano) > 0n ? <div className="sub partner-bad">{nanoMoney(partner.debtNano)} {t("debt", "долг")}</div> : null}</td>
        <td><Link className="btn ghost" href={`/partners/${partner.id}`}>{t("Open", "Открыть")}</Link></td>
      </tr>) : <EmptyRow columns={9} text={t("No partners match the filters", "Нет партнёров по выбранным фильтрам")} />}
    </tbody></table></TableCard>
  </>;
}

function formatBps(value: number): string {
  const percent = value / 100;
  return `${Number.isInteger(percent) ? percent : percent.toFixed(2).replace(/0+$/, "").replace(/\.$/, "")}%`;
}
