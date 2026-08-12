"use client";

// Тренды — порт 1:1 функции trends() из crates/server/src/admin-panel.js.
// История флота из metrics.db: GET /fleet-history?window=<окно>[&sub=<маска>],
// маски подписок — GET /subs (поле email). Новые снапшоты приходят по engine SSE.
import { startTransition, useMemo, useState, type ReactNode } from "react";
import { useResources } from "@/lib/resources";
import { count, duration, formatDate, money } from "@/lib/format";
import { Banner, LoadingGrid, PageHead, Pill } from "@/components/ui";
import { LineChart, type ChartPoint } from "./line-chart";

// Окна истории — как trendsWindows в admin-panel.js.
const TRENDS_WINDOWS: Array<[string, string]> = [
  ["24h", "24 часа"],
  ["7d", "7 дней"],
  ["30d", "30 дней"],
  ["90d", "90 дней"],
];

// Точка ряда /fleet-history (fleet — полный набор; per-sub — только cap/util).
interface FleetPoint {
  ts: number;
  avail_1h?: number | null;
  avail_5h?: number | null;
  avail_7d?: number | null;
  util5h?: number | null;
  util7d?: number | null;
  cap5h?: number | null;
  cap7d?: number | null;
  balance_usd?: number | null;
  potential_realapi?: number | null;
  subs_needed?: number | null;
  gap?: number | null;
}

interface FleetHistory {
  window?: string;
  bucket_secs?: number;
  series?: FleetPoint[];
}

interface SubsList {
  subs?: Array<{ email?: string }>;
}

const pct = (value: number) => Math.round(value * 100) + "%";
const integer = (value: number) => String(Math.round(value));

// Обёртка блока графика — порт trendsChart() (tcard + заголовок + подпись).
function TrendsChart(props: { title: string; sub: string; children: ReactNode }) {
  return (
    <div className="tcard" style={{ padding: 16, marginBottom: 12 }}>
      <div style={{ fontWeight: 650 }}>{props.title}</div>
      <div className="sub" style={{ margin: "2px 0 10px" }}>
        {props.sub}
      </div>
      {props.children}
    </div>
  );
}

export default function TrendsPage() {
  const [window, setWindow] = useState("7d");
  const [sub, setSub] = useState("");
  const suffix = "?window=" + window + (sub ? "&sub=" + encodeURIComponent(sub) : "");
  const { data: result, isLoading, updatedAt: fetchedAt } = useResources<{
    data: FleetHistory;
    subs: SubsList;
  }>({
    data: "/fleet-history" + suffix,
    subs: "/subs",
  });
  const subscriptions = useMemo(() => (result.subs?.subs ?? []).map((item) => item.email ?? ""), [result.subs]);

  // Наборы точек для графиков — пересчитываем только при смене данных/фильтра.
  const charts = useMemo(() => {
    const series = result?.data?.series ?? [];
    const pt = (key: keyof FleetPoint): ChartPoint[] => series.map((point) => ({ ts: point.ts, value: point[key] }));
    if (sub) {
      return (
        <>
          <TrendsChart title="Ёмкость подписки" sub="real-API $ окон 5ч/7д — по спаду cap видно деградацию до отвала">
            <LineChart series={[{ label: "cap 7д", points: pt("cap7d") }, { label: "cap 5ч", points: pt("cap5h") }]} fmt={money} />
          </TrendsChart>
          <TrendsChart title="Утилизация подписки" sub="доля окна, выбранная на момент снапшота">
            <LineChart series={[{ label: "util 7д", points: pt("util7d") }, { label: "util 5ч", points: pt("util5h") }]} fmt={pct} min={0} max={1} />
          </TrendsChart>
        </>
      );
    }
    return (
      <>
        <TrendsChart title="Доступная ёмкость флота" sub="real-API $ с учётом сбросов окон">
          <LineChart
            series={[
              { label: "доступно 7д", points: pt("avail_7d") },
              { label: "доступно 5ч", points: pt("avail_5h") },
              { label: "доступно 1ч", points: pt("avail_1h") },
            ]}
            fmt={money}
          />
        </TrendsChart>
        <TrendsChart title="Утилизация флота" sub="средняя по routable подпискам">
          <LineChart series={[{ label: "util 5ч", points: pt("util5h") }, { label: "util 7д", points: pt("util7d") }]} fmt={pct} min={0} max={1} />
        </TrendsChart>
        <TrendsChart title="Дефицит подписок" sub="максимум по бакету: сколько докупить для целевого запаса">
          <LineChart series={[{ label: "gap — докупить", points: pt("gap") }, { label: "нужно всего", points: pt("subs_needed") }]} fmt={integer} min={0} />
        </TrendsChart>
        <TrendsChart title="Баланс клиентов и потенциальный спрос" sub="деньги на счетах и их real-API эквивалент">
          <LineChart series={[{ label: "баланс клиентов", points: pt("balance_usd") }, { label: "потенциальный спрос", points: pt("potential_realapi") }]} fmt={money} />
        </TrendsChart>
      </>
    );
  }, [result, sub]);

  const toolbar = (
    <div className="toolbar">
      <div className="spend-tabs">
        {TRENDS_WINDOWS.map(([id, label]) => (
          <button
            key={id}
            type="button"
            className={"btn" + (window === id ? " on" : "")}
            onClick={() => startTransition(() => setWindow(id))}
          >
            {label}
          </button>
        ))}
      </div>
      <label className="sr-only" htmlFor="trends-sub">
        Подписка
      </label>
      <select id="trends-sub" value={sub} onChange={(event) => startTransition(() => setSub(event.target.value))}>
        <option value="">весь флот</option>
        {subscriptions.map((email) => (
          <option key={email} value={email}>
            {email}
          </option>
        ))}
      </select>
    </div>
  );

  if (isLoading && Object.values(result).every((value) => value === undefined)) {
    return (
      <>
        <PageHead title="Тренды" sub="данные загружаются, навигация уже доступна" />
        <LoadingGrid />
      </>
    );
  }

  const { data } = result;

  // /fleet-history не отвечает — toolbar остаётся рабочим (как bindTrends() в легаси).
  if (!data) {
    return (
      <>
        <PageHead
          title="Тренды"
          sub="история ёмкости, спроса и дефицита флота"
          badge={<Pill kind="warn">degraded</Pill>}
        />
        {toolbar}
        <Banner kind="warn" title="История флота временно недоступна">
          /fleet-history не отвечает. Остальные разделы работают, источник проверяется автоматически.
        </Banner>
      </>
    );
  }

  const series = data.series ?? [];
  const windowText = (TRENDS_WINDOWS.find((item) => item[0] === data.window) ?? ["", "окно"])[1].toLowerCase();

  return (
    <>
      <PageHead
        title="Тренды"
        sub="история ёмкости, спроса и дефицита флота"
        badge={<Pill kind={series.length ? "ok" : "warn"}>{count(series.length, "точка", "точки", "точек") + " · " + windowText}</Pill>}
      />
      {toolbar}
      {series.length ? (
        <Banner kind="ok" title={(sub ? "История подписки " + sub : "История флота") + ": " + count(series.length, "точка", "точки", "точек")}>
          окно {windowText} · бакет {duration(data.bucket_secs)} · обновлено {formatDate(fetchedAt, true)}
        </Banner>
      ) : (
        <Banner kind="warn" title={"За окно «" + windowText + "» данных пока нет"}>
          Коллектор пишет снапшот в metrics.db раз в минуту
          {sub ? "; ряд по маске " + sub + " пуст — проверьте другую подписку" : ""}.
        </Banner>
      )}
      {charts}
      <footer>
        Новые минутные снапшоты приходят по realtime-событию; ↻ оставлен для ручной проверки. Агрегация бакета: среднее по
        уровням и деньгам, максимум по gap/«нужно подписок» (планирование по худшей точке). Per-sub ряд строится по
        префиксу маски: при совпадении первых 4 символов email у двух подписок их ряды склеиваются.
      </footer>
    </>
  );
}
