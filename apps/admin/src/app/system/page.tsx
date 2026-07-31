"use client";

// Система — порт 1:1 функций system() и systemVerdict() из
// crates/server/src/admin-panel.js: ёмкость флота vs спрос, рекомендации
// по докупке подписок и балансы engine-аккаунтов. Опрос каждые 10 с.
import Link from "next/link";
import { memo } from "react";
import { api } from "@/lib/api";
import { usePoll } from "@/lib/usePoll";
import { count, money, ratio } from "@/lib/format";
import {
  Banner,
  CardGrid,
  EmptyRow,
  LoadingGrid,
  PageHead,
  Pill,
  SectionHeader,
  StatCard,
  TableCard,
  type Tone,
} from "@/components/ui";
import { useSpendStatsModal, type OkDirectoryRow } from "@/components/spend-stats-modal";
import { OkBadge, OkInfo, okDirectory } from "./ok";

const POLL_INTERVAL_MS = 10_000;

// Полная форма GET /overview, которую читает системная вкладка (lib/types.ts
// держит усечённый EngineOverview для Сводки — здесь нужны supply/demand/headroom).
export interface SystemAccount {
  account?: string;
  handle?: string;
  status?: string;
  balance_usd?: number;
  spent_usd?: number;
  mult?: number;
}

export interface SystemOverview {
  subs?: number;
  ref_mult?: number;
  target_headroom?: number;
  supply?: {
    avail_usd?: Record<string, number>;
    cap_usd?: Record<string, number>;
    consumed_usd?: Record<string, number>;
    util?: Record<string, number>;
    health?: { healthy?: number; cooling?: number; suspect?: number; dead?: number };
  };
  demand?: {
    balance_usd?: number;
    reserved_usd?: number;
    spent_usd?: number;
    active_accounts?: number;
    potential_realapi_usd?: number;
  };
  headroom?: Record<string, number | null>;
  coverage?: Record<string, number>;
  recommend?: { subs_needed?: number; gap?: number };
  accounts?: SystemAccount[];
}

interface SystemData {
  overview: SystemOverview | null;
  okDir: Map<string, OkDirectoryRow>;
}

// Все источники параллельно; /overview деградирует в null (warn-баннер), карта
// OpenKeys — в пустой Map (строки просто остаются без подписи). /capacity в
// легаси запрашивается в этом же Promise.all, но в разметке не используется —
// запрос сохранён 1:1.
async function loadSystem(): Promise<SystemData> {
  const [overview, , okDir] = await Promise.all([
    api<SystemOverview>("/overview").catch(() => null),
    api<unknown>("/capacity").catch(() => null),
    okDirectory(),
  ]);
  return { overview, okDir };
}

export type SystemVerdict = { kind: "ok" | "warn" | "bad"; title: string; detail: string };

// Вердикт по запасу ёмкости — порт systemVerdict() из admin-panel.js.
export function systemVerdict(overview: SystemOverview): SystemVerdict {
  const gap = overview.recommend?.gap ?? 0;
  const h5 = overview.headroom?.["5h"] ?? null;
  const h7 = overview.headroom?.["7d"] ?? null;
  const target = overview.target_headroom ?? 0;
  const coverage = overview.coverage?.["7d"] ?? 0;
  const cooling = overview.supply?.health?.cooling ?? 0;
  const total = overview.subs ?? 0;
  const critical = (value: number | null): boolean => value != null && value < 1;
  const tight = (value: number | null): boolean => value != null && value < target;
  if (critical(h5) || critical(h7) || (total > 0 && cooling >= total)) {
    return {
      kind: "bad",
      title: "Дефицит ёмкости — нужно +" + Math.max(1, gap) + " подписок",
      detail: "headroom 5h " + ratio(h5) + " / 7d " + ratio(h7) + " · потребление близко к потолку",
    };
  }
  if (gap > 0 || tight(h5) || tight(h7) || coverage > 1 || cooling > 0) {
    const why =
      gap > 0
        ? "рекомендуется +" + gap + " подписок"
        : tight(h5) || tight(h7)
          ? "запас ниже цели ×" + target
          : coverage > 1
            ? "балансы клиентов ×" + coverage + " к ёмкости"
            : cooling + " подписок остывают";
    return {
      kind: "warn",
      title: "Под контролем, но нужно внимание",
      detail: why + " · headroom 5h " + ratio(h5) + " / 7d " + ratio(h7),
    };
  }
  return {
    kind: "ok",
    title: "Запаса ёмкости хватает",
    detail:
      "headroom 5h " + ratio(h5) + " / 7d " + ratio(h7) + " · подписок " + total + ", цель ×" + target + " выдержана",
  };
}

// Карточки «доступно» по горизонтам: [ключ avail_usd, подпись, ключ headroom].
const HORIZONS: ReadonlyArray<readonly [string, string, "5h" | "7d" | null]> = [
  ["7d", "7 дней", "7d"],
  ["1d", "1 день", null],
  ["5h", "5 часов (burst)", "5h"],
];

const AccountRows = memo(function AccountRows({
  accounts,
  okDir,
}: {
  accounts: SystemAccount[];
  okDir: Map<string, OkDirectoryRow>;
}) {
  if (!accounts.length) return <EmptyRow columns={6} />;
  return (
    <>
      {accounts.map((account) => (
        <tr key={account.account ?? account.handle}>
          <td className="left mono muted">{account.account}</td>
          <td className="left">
            <b>{account.handle || "—"}</b>
            <OkBadge handle={account.handle} />
            <OkInfo meta={account.account ? okDir.get(account.account) : undefined} />
          </td>
          <td>
            <Pill kind={account.status === "active" ? "ok" : "bad"}>{account.status}</Pill>
          </td>
          <td>
            <b>{money(account.balance_usd)}</b>
          </td>
          <td>
            <b>{money(account.spent_usd)}</b>
          </td>
          <td>×{account.mult}</td>
        </tr>
      ))}
    </>
  );
});

export default function SystemPage() {
  const { data: result } = usePoll("system", loadSystem, { interval: POLL_INTERVAL_MS });
  const { openSpendStats, spendStatsModal } = useSpendStatsModal();

  if (!result) {
    return (
      <>
        <PageHead title="Система" sub="данные загружаются, навигация уже доступна" />
        <LoadingGrid />
      </>
    );
  }

  const { overview, okDir } = result;

  // Без /overview страница сводится к warn-баннеру — как в легаси system().
  if (!overview) {
    return (
      <>
        <PageHead
          title="Система"
          sub="ёмкость, спрос и рекомендации по флоту"
          badge={<Pill kind="warn">degraded</Pill>}
        />
        <Banner kind="warn" title="Свежая системная сводка недоступна">
          Остальные разделы работают. Панель автоматически проверяет восстановление источника.
        </Banner>
      </>
    );
  }

  const supply = overview.supply ?? {};
  const health = supply.health ?? {};
  const demand = overview.demand ?? {};
  const recommend = overview.recommend ?? {};
  const availUsd = supply.avail_usd ?? {};
  const capUsd = supply.cap_usd ?? {};
  const consumedUsd = supply.consumed_usd ?? {};
  const util = supply.util ?? {};
  const mult = overview.ref_mult ?? 0;
  const subs = overview.subs ?? 0;
  const gap = recommend.gap ?? 0;
  const accounts = overview.accounts ?? [];
  const verdict = systemVerdict(overview);
  const verdictDot: Tone = verdict.kind === "ok" ? "" : verdict.kind;

  return (
    <>
      <PageHead
        title="Система"
        sub="ёмкость, спрос и рекомендации по флоту"
        badge={<Pill kind={verdict.kind}>{count(subs, "подписка", "подписки", "подписок")}</Pill>}
      />

      <Banner kind={verdict.kind} dot={verdictDot} title={verdict.title}>
        {verdict.detail}
      </Banner>

      <SectionHeader title="Предложение — real-API USD" />
      <CardGrid>
        {HORIZONS.map(([key, label, headKey]) => {
          const available = availUsd[key] || 0;
          return (
            <StatCard
              key={key}
              label={"доступно · " + label}
              value={money(available)}
              hint={"клиентам ×" + mult + " = " + money(available * mult) + (headKey ? " · запас " + ratio(overview.headroom?.[headKey]) : "")}
            />
          );
        })}
        <StatCard
          label="балансы клиентов"
          value={money(demand.balance_usd)}
          hint={
            "резерв " +
            money(demand.reserved_usd) +
            " · coverage 7d ×" +
            (overview.coverage?.["7d"] ?? 0) +
            " · активных " +
            (demand.active_accounts ?? "—")
          }
        />
      </CardGrid>

      <SectionHeader title="Флот и спрос" />
      <CardGrid>
        <StatCard
          label="подписки"
          value={subs}
          hint={
            (health.healthy ?? 0) +
            " живых · " +
            (health.cooling ?? 0) +
            " cooling · " +
            Number(health.suspect || 0) +
            " suspect · " +
            Number(health.dead || 0) +
            " dead"
          }
        />
        <StatCard
          label="утилизация средняя"
          value={Math.round((util["7d"] ?? 0) * 100) + "%"}
          hint={"7d · " + Math.round((util["5h"] ?? 0) * 100) + "% за 5h"}
        />
        <StatCard
          label="ёмкость окон 7д"
          value={money(capUsd["7d"])}
          hint={"потреблено " + money(consumedUsd["7d"]) + " · 5h " + money(capUsd["5h"]) + " / " + money(consumedUsd["5h"])}
        />
        <StatCard
          label="всего потрачено"
          value={money(demand.spent_usd)}
          hint={"потенциальный спрос " + money(demand.potential_realapi_usd) + " real-API"}
          onClick={openSpendStats}
          title="Разбивка: сутки / 7 дней / 30 дней"
        />
        <StatCard
          label="рекомендация"
          value={gap > 0 ? "+" + gap : "ok"}
          hint={"нужно " + (recommend.subs_needed ?? 0) + " подписок · есть " + subs}
        />
      </CardGrid>

      <SectionHeader title="Подписки" sub="живой статус флота" />
      <Banner kind="ok" title="Детальный статус подписок — на отдельной странице">
        <Link className="link" href="/subscriptions">
          Открыть «Подписки»
        </Link>{" "}
        — окна, cooling, quota, lifecycle и transport по Claude, GPT и Gemini.
      </Banner>

      <SectionHeader title={"Аккаунты движка · " + accounts.length} />
      <TableCard>
        <table>
          <thead>
            <tr>
              <th className="left">account</th>
              <th className="left">handle</th>
              <th>статус</th>
              <th>баланс</th>
              <th>
                <span data-spend-stats title="Разбивка: сутки / 7 дней / 30 дней" onClick={openSpendStats}>
                  потрачено
                </span>
              </th>
              <th>множитель</th>
            </tr>
          </thead>
          <tbody>
            <AccountRows accounts={accounts} okDir={okDir} />
          </tbody>
        </table>
      </TableCard>

      <footer>
        Обновление каждые 10с, пока вкладка видима · «доступно» учитывает сбросы окон · «запас» = доступно ÷ текущее
        потребление · клиентам ×{mult}
      </footer>

      {spendStatsModal}
    </>
  );
}
