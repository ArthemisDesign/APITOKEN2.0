"use client";

import Link from "next/link";
import { Banner, CardGrid, EmptyRow, LoadingGrid, PageHead, Pill, SectionHeader, StatCard, TableCard } from "@/components/ui";
import { formatDate, nanoMoney } from "@/lib/format";
import { localeFor, useI18n } from "@/lib/i18n";
import { useResources } from "@/lib/resources";
import type { AdminPartner, AdminPartnerPayout, PartnerRequestsPage } from "./types";

type OverviewData = {
  partners: { items: AdminPartner[] };
  requests: PartnerRequestsPage;
  payouts: { items: AdminPartnerPayout[] };
};

export default function PartnersPage() {
  const { lang, t } = useI18n();
  const locale = localeFor(lang);
  const result = useResources<OverviewData>({
    partners: "/admin/referral/partners",
    requests: "/admin/referral/requests?status=pending&limit=100",
    payouts: "/admin/referral/payouts?status=requested",
  });

  if (result.isLoading && Object.values(result.data).every((value) => value === undefined)) {
    return <><PageHead title={t("Partner Program", "Партнёрская программа")} sub={t("Loading Commerce-linked accounts", "Загружаем связанные Commerce-аккаунты")} /><LoadingGrid label={t("Loading Partner Overview", "Загрузка партнёрской сводки")} /></>;
  }

  const partners = result.data.partners?.items ?? [];
  const requests = result.data.requests?.items ?? [];
  const payouts = result.data.payouts?.items ?? [];
  const active = partners.filter((partner) => partner.status === "active" && partner.programEnabled).length;
  const referrals = partners.reduce((sum, partner) => sum + partner.referredUsers, 0);
  const teamMembers = partners.reduce((sum, partner) => sum + partner.teamSize, 0);
  const payableNano = sumNano(partners.map((partner) => partner.payableNano));
  const recent = [...partners].sort((left, right) => right.createdAt.localeCompare(left.createdAt)).slice(0, 8);

  return <>
    <PageHead title={t("Partner Program", "Партнёрская программа")} sub={t("Commerce accounts, delegated authority, requests, and payouts in one control room", "Commerce-аккаунты, делегируемые права, заявки и выплаты в одном разделе")} badge={<Pill kind={requests.length || payouts.length ? "warn" : "ok"}>{requests.length + payouts.length} {t("await review", "ждут решения")}</Pill>} />
    {!result.data.partners ? <Banner kind="bad" title={t("Partner Data Unavailable", "Данные партнёров недоступны")}>{t("The Commerce referral projection did not load. Other sections remain available.", "Commerce-проекция партнёров не загрузилась. Остальные разделы продолжают работать.")}</Banner> : null}
    <CardGrid>
      <StatCard label={t("Active Partners", "Активные партнёры")} value={active} hint={`${partners.length} ${t("total accounts", "всего аккаунтов")}`} />
      <StatCard label={t("Referred Accounts", "Привлечённые аккаунты")} value={referrals} hint={t("identified by Commerce email", "определяются по Commerce email")} />
      <StatCard label={t("Team Members", "Участники Team")} value={teamMembers} hint={t("direct edges across all partners", "прямые связи всех партнёров")} />
      <StatCard label={t("Total Payable", "Всего к выплате")} value={nanoMoney(payableNano)} hint={`${payouts.length} ${t("payout requests", "заявок на выплату")}`} />
    </CardGrid>

    <div className="partner-overview-actions"><Link className="btn" href="/partners/onboarding">{t("Enable Partner Access", "Сделать партнёром")}</Link><Link className="btn ghost" href="/partners/requests">{t("Review Requests", "Рассмотреть заявки")}</Link><Link className="btn ghost" href="/partners/directory">{t("Open Partner Directory", "Открыть список")}</Link></div>

    <SectionHeader title={t("Recent Partners", "Новые партнёры")} sub={t("Email is the only visible account identity", "Email — единственный отображаемый идентификатор аккаунта")} />
    <TableCard><table><thead><tr><th className="left">Email</th><th>{t("Status", "Статус")}</th><th>{t("Direct Commission", "Прямая комиссия")}</th><th>{t("Team Share Max", "Макс. Team-доля")}</th><th>{t("Referrals", "Рефералы")}</th><th>{t("Payable", "К выплате")}</th><th>{t("Created", "Создан")}</th></tr></thead><tbody>
      {recent.length ? recent.map((partner) => <tr key={`${partner.email ?? "missing"}-${partner.createdAt}`}><td className="left">{partner.email ? <Link className="partner-email-link" href={`/partners/${encodeURIComponent(partner.email)}`} translate="no">{partner.email}</Link> : <span className="partner-missing-email">{t("Commerce email unavailable", "Commerce email недоступен")}</span>}</td><td><Pill kind={partner.status === "active" && partner.programEnabled ? "ok" : partner.status === "suspended" ? "bad" : "warn"}>{partner.programEnabled ? statusLabel(partner.status, t) : t("Disabled", "Отключён")}</Pill></td><td>{formatBps(partner.commissionBps)}</td><td>{formatBps(partner.teamOverrideMaxBps)}</td><td>{partner.referredUsers}</td><td>{nanoMoney(partner.payableNano)}</td><td>{formatDate(partner.createdAt, false, locale)}</td></tr>) : <EmptyRow columns={7} text={t("No Partner Accounts", "Партнёрских аккаунтов нет")} />}
    </tbody></table></TableCard>
  </>;
}

function sumNano(values: string[]): string {
  return values.reduce((sum, value) => sum + BigInt(value), 0n).toString();
}

function statusLabel(status: AdminPartner["status"], t: (en: string, ru: string) => string): string {
  if (status === "active") return t("Active", "Активен");
  if (status === "suspended") return t("Suspended", "Приостановлен");
  return t("Pending", "Ожидает");
}

function formatBps(value: number): string {
  const percent = value / 100;
  return `${Number.isInteger(percent) ? percent : percent.toFixed(2).replace(/0+$/, "").replace(/\.$/, "")}%`;
}
