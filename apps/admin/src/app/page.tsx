"use client";

// Сводка — порт 1:1 функции dashboard() из crates/server/src/admin-panel.js.
// Данные грузятся параллельно и обновляются только по producer SSE invalidation.
import Link from "next/link";
import { compactOverviewUrl } from "@/lib/engine-urls";
import { useResources } from "@/lib/resources";
import { formatDate, money, nanoMoney } from "@/lib/format";
import type {
  CommerceDashboard,
  EngineOverview,
  PartnerOverview,
  PipelineHealth,
  SettlementHealth,
} from "@/lib/types";
import { Banner, CardGrid, LoadingGrid, PageHead, Pill, SectionHeader, StatCard } from "@/components/ui";
import { useSpendStatsModal } from "@/components/spend-stats-modal";

interface DashboardData {
  data: CommerceDashboard;
  engine: EngineOverview;
  partners: PartnerOverview;
  pipes: PipelineHealth;
  settle: SettlementHealth;
}

const show = (value: number | null | undefined): number | "—" => value ?? "—";

export default function DashboardPage() {
  const { data: result, isLoading } = useResources<DashboardData>({
    data: "/admin/dashboard",
    engine: compactOverviewUrl(),
    partners: "/partner-admin/overview",
    pipes: "/admin/pipeline-health",
    settle: "/settlement-health",
  });
  const { openSpendStats, spendStatsModal } = useSpendStatsModal();

  if (isLoading && Object.values(result).every((value) => value === undefined)) {
    return (
      <>
        <PageHead title="Сводка" sub="данные загружаются, навигация уже доступна" />
        <LoadingGrid />
      </>
    );
  }

  const { data, engine, partners, pipes, settle } = result;
  const u = data?.users ?? {};
  const t = data?.topups ?? {};
  const p = data?.platform ?? {};

  const engineAccountsTotal = engine?.accounts_total ?? (engine?.accounts ?? []).length;
  const engineAccountsActive =
    engine?.accounts_active ?? (engine?.accounts ?? []).filter((account) => account.status === "active").length;
  const crm =
    engine?.crm ??
    (engine?.accounts ?? []).find((account) => String(account.handle ?? "").toLowerCase() === "crm-parsing");
  const degraded = !data || Boolean(p.engine_error) || !engine || !partners || !crm;

  // Денежные пайплайны: warn/bad баннер-строка в начале сводки, клик ведёт на /finance.
  // Источники деградируют молча (null → без баннера), как соседние контуры.
  const moneyReasons: string[] = [];
  let moneyKind: "" | "warn" | "bad" = "";
  if (pipes && pipes.verdict && pipes.verdict !== "ok") {
    moneyKind = pipes.verdict === "bad" ? "bad" : "warn";
    moneyReasons.push(...(pipes.verdict_reasons ?? []));
  }
  if (settle) {
    const out = settle.outbox ?? {};
    if ((out.failed_24h ?? 0) > 0) {
      moneyKind = "bad";
      moneyReasons.push(`settlement: ${out.failed_24h} failed за 24ч`);
    }
    if ((out.backlog ?? 0) > 0) {
      moneyKind = moneyKind || "warn";
      moneyReasons.push(`settlement: backlog ${out.backlog}`);
    }
  }

  return (
    <>
      <PageHead
        title="Сводка"
        sub="все контуры одним взглядом"
        badge={<Pill kind={data ? "ok" : "warn"}>{show(u.active)} active</Pill>}
      />

      <Banner kind={degraded ? "warn" : "ok"} title={degraded ? "Есть контуры, требующие внимания" : "Все административные контуры доступны"}>
        обновлено {formatDate(data?.generated_at, true)} · сессий {show(p.active_sessions)} · engine errors{" "}
        {show(p.engine_error)} · CRM {crm ? crm.status : "account missing"}
      </Banner>

      {moneyKind ? (
        <Banner kind={moneyKind} title="Проблема в денежных пайплайнах" href="/finance">
          {moneyReasons.length ? moneyReasons.join(" · ") + " · " : ""}разбор — вкладка «Финансы»
        </Banner>
      ) : null}

      <SectionHeader title="Аккаунты по контурам" sub="commerce · engine · partners · CRM" />
      <CardGrid>
        <StatCard
          label="commerce accounts"
          value={show(u.total)}
          hint={`${show(u.active)} активны · ${show(u.disabled)} отключены`}
        />
        <StatCard
          label="engine accounts"
          value={engine ? engineAccountsTotal : "—"}
          hint={engine ? `${engineAccountsActive} active` : "источник недоступен"}
          onClick={openSpendStats}
          title="Разбивка: сутки / 7 дней / 30 дней"
        />
        <StatCard
          label="partner accounts"
          value={partners ? show(partners.partners) : "—"}
          hint={
            partners ? (
              <>
                {show(partners.activePartners)} active · {show(partners.referredUsers)} referrals
                <br />
                комиссии {nanoMoney(partners.totalCommissionsNano)} · к выплате {nanoMoney(partners.pendingPayoutsNano)} · выплачено {nanoMoney(partners.paidPayoutsNano)}
              </>
            ) : (
              "источник недоступен"
            )
          }
        />
        <StatCard
          label="CRM & Parsing"
          value={crm ? (crm.status ?? "—") : "не найден"}
          hint={crm ? `${crm.handle ?? "—"} · ${money(crm.balance_usd)}` : "нужен engine account crm-parsing"}
        />
      </CardGrid>

      <SectionHeader title="Клиенты и регистрации" />
      <CardGrid>
        <StatCard
          label="всего клиентов"
          value={show(u.total)}
          hint={`${show(u.active)} активны · ${show(u.disabled)} отключены`}
        />
        <StatCard
          label="OAuth-регистрации"
          value={show(u.registered_oauth)}
          hint={`сейчас OAuth-only ${show(u.oauth_only)} · hybrid ${show(u.hybrid)} · Google ${show(u.google)} · GitHub ${show(u.github)}`}
        />
        <StatCard
          label="обычная регистрация"
          value={show(u.registered_password)}
          hint={`сейчас password-only ${show(u.password_only)}`}
        />
        <StatCard
          label="новые за 30 дней"
          value={show(u.registered_30d)}
          hint={`24ч ${show(u.registered_24h)} · active 7д ${show(u.active_7d)}`}
        />
      </CardGrid>

      <SectionHeader title="Деньги и пополнения" />
      <CardGrid>
        <StatCard
          label="успешные пополнения"
          value={show(t.paid_count)}
          hint={`${show(t.paid_users)} платящих клиентов`}
        />
        <StatCard
          label="пополнено всего"
          value={data ? money(t.paid_usd) : "—"}
          hint={`30д ${data ? money(t.paid_30d_usd) : "—"} · ${show(t.paid_30d_count)} шт.`}
        />
        <StatCard
          label="ручные начисления"
          value={show(t.manual_count)}
          hint={`${data ? money(t.manual_usd) : "—"} · 30д ${show(t.manual_30d_count)}${data && t.manual_30d_usd != null ? " / " + money(t.manual_30d_usd) : ""}`}
        />
        <StatCard
          label="ожидают оплаты"
          value={show(t.pending_checkouts)}
          hint={`ошибок 30д ${show(t.failed_30d)} · возвратов ${show(t.refunded_count)}${data && t.refunded_usd != null ? " на " + money(t.refunded_usd) : ""}`}
        />
      </CardGrid>

      <SectionHeader title="Платформа" />
      <CardGrid>
        <StatCard label="API-ключи" value={show(p.active_api_keys)} hint={`активны из ${show(p.total_api_keys)}`} />
        <StatCard label="B2C / B2B" value={`${show(p.b2c_users)} / ${show(p.b2b_users)}`} hint="клиенты по типу тарифа" />
        <StatCard
          label="engine active"
          value={show(p.engine_active)}
          hint={`pending ${show(p.engine_pending)} · disabled ${show(p.engine_disabled)}`}
        />
        <StatCard label="защищены 2FA" value={show(u.totp)} hint={`email подтверждено ${show(u.verified)}`} />
      </CardGrid>

      <footer>
        Подробный единый список engine, commerce, partner и CRM-аккаунтов — на странице{" "}
        <Link className="link" href="/accounts">
          «Аккаунты»
        </Link>
        . Флоты подписок Claude, GPT и Gemini — на странице{" "}
        <Link className="link" href="/subscriptions">
          «Подписки»
        </Link>
        .
      </footer>

      {spendStatsModal}
    </>
  );
}
