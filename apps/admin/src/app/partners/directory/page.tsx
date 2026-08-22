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

function statusLabel(status: AdminPartner["status"], t: (en: string, ru: string) => string): string {
  if (status === "active") return t("Active", "Активен");
  if (status === "suspended") return t("Suspended", "Приостановлен");
  return t("Pending", "Ожидает");
}

export default function PartnerDirectoryPage() {
  const { t } = useI18n();
  const router = useRouter();
  const search = useSearchParams();
  const query = search.get("q") ?? "";
  const statusParam = search.get("status");
  const status: StatusFilter = statusParam === "active" || statusParam === "suspended" || statusParam === "pending" ? statusParam : "all";
  const sortParam = search.get("sort");
  const sort: SortKey = sortParam === "payable" || sortParam === "team" || sortParam === "referrals" ? sortParam : "created";
  const deferredQuery = useDeferredValue(query.trim().toLocaleLowerCase());
  const { data, isLoading, refresh } = useResource<{ items: AdminPartner[] }>("/admin/referral/partners");

  function replaceFilters(patch: { query?: string; status?: StatusFilter; sort?: SortKey }) {
    const params = new URLSearchParams(search.toString());
    const nextQuery = patch.query ?? query;
    const nextStatus = patch.status ?? status;
    const nextSort = patch.sort ?? sort;
    if (nextQuery) params.set("q", nextQuery); else params.delete("q");
    if (nextStatus !== "all") params.set("status", nextStatus); else params.delete("status");
    if (nextSort !== "created") params.set("sort", nextSort); else params.delete("sort");
    router.replace(`/partners/directory${params.size ? `?${params}` : ""}`, { scroll: false });
  }

  const items = useMemo(() => {
    const filtered = (data?.items ?? []).filter((partner) => {
      if (status !== "all" && partner.status !== status) return false;
      if (!deferredQuery) return true;
      return [partner.email, partner.referralCode, partner.parentEmail]
        .some((value) => value?.toLocaleLowerCase().includes(deferredQuery));
    });
    return filtered.sort((left, right) => {
      if (sort === "payable") return compareNano(right.payableNano, left.payableNano);
      if (sort === "team") return right.teamSize - left.teamSize;
      if (sort === "referrals") return right.referredUsers - left.referredUsers;
      return right.createdAt.localeCompare(left.createdAt);
    });
  }, [data, deferredQuery, sort, status]);

  if (isLoading && !data) return <><PageHead title={t("Partner Directory", "Партнёры")} sub={t("Loading authority records", "Загружаем права и условия")} /><LoadingGrid label={t("Loading Partner Directory", "Загрузка списка партнёров")} /></>;

  return <>
    <PageHead title={t("Partner Directory", "Партнёры")} sub={t("Commerce email is the visible identity for every partner", "Commerce email — отображаемый идентификатор каждого партнёра")} badge={<Pill kind="info">{items.length}</Pill>} />
    {!data ? <Banner kind="bad" title={t("Directory Unavailable", "Список недоступен")}>{t("Refresh the page or check the Commerce referral API.", "Обновите страницу или проверьте Commerce referral API.")}</Banner> : null}
    <div className="partner-directory-toolbar">
      <label className="field"><span>{t("Search", "Поиск")}</span><input name="partnerSearch" type="search" autoComplete="off" spellCheck={false} value={query} onChange={(event) => replaceFilters({ query: event.target.value })} placeholder={t("Email or referral code…", "Email или реферальный код…")} /></label>
      <label className="field"><span>{t("Status", "Статус")}</span><select name="partnerStatus" value={status} onChange={(event) => replaceFilters({ status: event.target.value as StatusFilter })}><option value="all">{t("All", "Все")}</option><option value="active">{t("Active", "Активные")}</option><option value="suspended">{t("Suspended", "Приостановленные")}</option><option value="pending">{t("Pending", "Ожидающие")}</option></select></label>
      <label className="field"><span>{t("Sort", "Сортировка")}</span><select name="partnerSort" value={sort} onChange={(event) => replaceFilters({ sort: event.target.value as SortKey })}><option value="created">{t("Newest", "Новые")}</option><option value="payable">{t("Payable", "К выплате")}</option><option value="team">{t("Team Size", "Размер команды")}</option><option value="referrals">{t("Referrals", "Рефералы")}</option></select></label>
      <button type="button" className="btn ghost" onClick={refresh}>{t("Refresh", "Обновить")}</button>
    </div>
    <TableCard><table className="partner-directory-table"><thead><tr>
      <th className="left">Email</th><th>{t("Status", "Статус")}</th><th>{t("Direct Commission", "Прямая комиссия")}</th><th>{t("Retained Team Share Max", "Макс. удерживаемая Team-доля")}</th><th>{t("B2B Authority", "B2B-права")}</th><th>{t("Team", "Команда")}</th><th>{t("Referrals", "Рефералы")}</th><th>{t("Payable", "К выплате")}</th><th><span className="sr-only">{t("Open", "Открыть")}</span></th>
    </tr></thead><tbody>
      {items.length ? items.map((partner) => {
        const email = partner.email;
        return <tr key={`${email ?? "missing-email"}-${partner.createdAt}`}>
          <td className="left">{email ? <Link className="partner-email-link" href={`/partners/${encodeURIComponent(email)}`} title={email} translate="no">{email}</Link> : <span className="partner-missing-email">{t("Commerce email unavailable", "Commerce email недоступен")}</span>}<div className="sub mono" translate="no">{partner.referralCode}{partner.parentEmail ? ` · ↳ ${partner.parentEmail}` : ""}</div></td>
          <td><Pill kind={partner.status === "active" ? "ok" : partner.status === "suspended" ? "bad" : "warn"}>{statusLabel(partner.status, t)}</Pill>{!partner.programEnabled ? <div className="sub">{t("program disabled", "программа отключена")}</div> : null}</td>
          <td>{formatBps(partner.commissionBps)}</td><td>{formatBps(partner.teamOverrideMaxBps)}</td>
          <td><div className="permission-stack"><span>{partner.b2bEnabled ? `≤ ${formatBps(partner.b2bMaxDiscountBps)}` : "—"}</span><small>{partner.b2bCanDelegate ? t("may delegate", "делегирует") : t("no delegation", "без делегирования")}</small></div></td>
          <td>{partner.teamSize}<div className="sub">{partner.teamInvitesEnabled ? t("invites enabled", "приглашения включены") : t("invites disabled", "приглашения отключены")}</div></td><td>{partner.referredUsers}</td><td><b>{nanoMoney(partner.payableNano)}</b>{BigInt(partner.debtNano) > 0n ? <div className="sub partner-bad">{nanoMoney(partner.debtNano)} {t("debt", "долг")}</div> : null}</td>
          <td>{email ? <Link className="btn ghost" href={`/partners/${encodeURIComponent(email)}`}>{t("Open", "Открыть")}</Link> : "—"}</td>
        </tr>;
      }) : <EmptyRow columns={9} text={t("No partners match the filters", "Нет партнёров по выбранным фильтрам")} />}
    </tbody></table></TableCard>
  </>;
}

function compareNano(left: string, right: string): number {
  const a = BigInt(left);
  const b = BigInt(right);
  return a === b ? 0 : a > b ? 1 : -1;
}

function formatBps(value: number): string {
  const percent = value / 100;
  return `${Number.isInteger(percent) ? percent : percent.toFixed(2).replace(/0+$/, "").replace(/\.$/, "")}%`;
}
