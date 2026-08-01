"use client";

// Таблицы страницы «Подписки» — порт 1:1 разметки subscriptions() из
// crates/server/src/admin-panel.js (Claude / GPT / Gemini / transport details).
// Компоненты мемоизированы: рендер идёт каждый poll-тик (10 с).
import { memo, type ReactElement, type ReactNode } from "react";
import { ago, count, duration, formatDate, money, nanoMoney, windowLabel } from "@/lib/format";
import { Dot, EmptyRow, Pill, TableCard } from "@/components/ui";
import {
  barFromPercent,
  barFromRemaining,
  barFromUtil,
  deadLabel,
  geminiProfileStatus,
  homeStatus,
  stripProxyPort,
  type BarSpec,
} from "./logic";
import type {
  CapacitySub,
  ClaudeSub,
  CodexHome,
  CodexHomeWindow,
  CodexRateLimit,
  GeminiAffinity,
  GeminiModel,
  GeminiProfile,
  GeminiTransport,
} from "./types";

// Прогресс-бар окна: <span class="bar"><i style="width:N%"></span> + подпись «N%».
function Bar({ spec }: { spec: BarSpec }): ReactElement {
  return (
    <>
      <span className="bar">
        <i className={spec.kind} style={{ width: spec.percent + "%" }} />
      </span>
      <span className="bar-label">{spec.percent}%</span>
    </>
  );
}

/* ── Claude ─────────────────────────────────────────────── */

function ClaudeRow({ item, live }: { item: ClaudeSub; live: CapacitySub }): ReactElement {
  const isDead = item.auth_state === "dead";
  const isSuspect = item.auth_state === "suspect";
  const days = Number(item.sub_days_left || 0);
  const dayKind = days <= 0 ? "bad" : days < 7 ? "warn" : "ok";
  return (
    <tr>
      <td className="left">
        <b>{item.email}</b>
        {live.calibrated === false ? <span className="pill">калибровка</span> : null}
      </td>
      <td>
        {isDead ? (
          <Pill kind="bad">{deadLabel(item.dead_reason)}</Pill>
        ) : isSuspect ? (
          <Pill kind="warn">под наблюдением (auth)</Pill>
        ) : live.cooling ? (
          <Pill kind="warn">cooling</Pill>
        ) : (
          <Pill kind={item.status === "active" ? "ok" : "warn"}>{item.status ?? ""}</Pill>
        )}
        {item.has_token === false ? (
          <>
            {" "}
            <Pill kind="bad">нет токена</Pill>
          </>
        ) : null}
        {isDead && Number(item.dead_since_ts) > 0 ? (
          <div className="sub">мертва {ago((item.dead_since_ts ?? 0) * 1000)}</div>
        ) : null}
      </td>
      <td>
        <div>
          <Bar spec={barFromUtil(live.util5h)} />
        </div>
        <div className="sub">сброс {duration(live.reset5h_in)}</div>
      </td>
      <td>
        <div>
          <Bar spec={barFromUtil(live.util7d)} />
        </div>
        <div className="sub">сброс {duration(live.reset7d_in)}</div>
      </td>
      <td>
        <b>{money(live.rem5h_usd)}</b>
        <div className="sub">7д {money(live.rem7d_usd)}</div>
      </td>
      <td>
        <Dot kind={dayKind} /> {days > 0 ? days + " дн." : "—"}
        <div className="sub">добавлена {String(item.added ?? "").slice(0, 10) || "—"}</div>
      </td>
      <td>
        <b>{Number(item.peak_cap5h_usd) > 0 ? money(item.peak_cap5h_usd) : "—"}</b>
        <div className="sub">7д {Number(item.peak_cap7d_usd) > 0 ? money(item.peak_cap7d_usd) : "—"}</div>
      </td>
      <td className="left mono" title={item.proxy_host ?? ""}>
        {stripProxyPort(item.proxy_host)}
        {item.proxy_ok === false ? (
          <>
            {" "}
            <Pill kind="bad">мёртв</Pill>
          </>
        ) : null}
        <div className="sub">до {String(item.proxy_expire ?? "").slice(0, 10) || "—"}</div>
      </td>
    </tr>
  );
}

export const ClaudeTable = memo(function ClaudeTable({
  list,
  liveByEmail,
}: {
  list: ClaudeSub[];
  liveByEmail: Record<string, CapacitySub>;
}): ReactElement {
  return (
    <TableCard>
      <table>
        <thead>
          <tr>
            <th className="left">подписка</th>
            <th>статус</th>
            <th>окно 5 ч</th>
            <th>окно 7 д</th>
            <th>остаток 5 ч</th>
            <th>живёт ещё</th>
            <th>пик 5 ч</th>
            <th className="left">прокси</th>
          </tr>
        </thead>
        <tbody>
          {list.length ? (
            list.map((item, index) => (
              <ClaudeRow key={item.email ?? index} item={item} live={liveByEmail[item.email ?? ""] ?? {}} />
            ))
          ) : (
            <EmptyRow columns={8} />
          )}
        </tbody>
      </table>
    </TableCard>
  );
});

/* ── GPT (Codex homes) ──────────────────────────────────── */

// Фактическое окно home: бар процента + доказательная база измерения.
function gptWindowCell(w: CodexHomeWindow | undefined): ReactNode {
  if (!w) return "—";
  const usedPercent =
    w.used_fraction_units == null ? w.used_percent : Number(w.used_fraction_units) / 1_000_000;
  return (
    <>
      <div>
        <Bar spec={barFromPercent(usedPercent)} />
      </div>
      <div className="sub">
        {windowLabel(w.window_minutes)}
        {w.source === "unknown"
          ? " · калибровка накапливает данные"
          : ` · ${Number(w.samples || 0)} интервала · confidence ${Math.round(Number(w.confidence || 0) * 100)}%`}
      </div>
    </>
  );
}

const codexMoney = (
  nano: string | null | undefined,
  usd: number | null | undefined,
): string => (nano != null ? nanoMoney(nano) : usd == null ? "—" : money(usd));

// Остаток / вместимость окна в API-$; exact nanoUSD — источник истины, а workload evidence
// остаётся в title, чтобы компактная таблица не теряла объяснимость калибровки.
function gptBudgetCell(w: CodexHomeWindow | undefined): ReactNode {
  if (!w) return "—";
  const known =
    w.source === "workload_blend" ||
    (w.source !== "unknown" && (w.capacity_nano != null || w.cap_usd != null));
  const capacity = codexMoney(w.capacity_nano, w.cap_usd);
  const remaining = codexMoney(w.remaining_nano, w.remaining_usd);
  const capacityRange =
    w.low_nano != null || w.low_usd != null
      ? codexMoney(w.low_nano, w.low_usd) +
        "–" +
        (w.high_nano == null && w.high_usd == null ? "∞" : codexMoney(w.high_nano, w.high_usd))
      : "—";
  const remainingRange =
    w.remaining_low_nano != null || w.remaining_low_usd != null
      ? codexMoney(w.remaining_low_nano, w.remaining_low_usd) +
        "–" +
        (w.remaining_high_nano == null && w.remaining_high_usd == null
          ? "∞"
          : codexMoney(w.remaining_high_nano, w.remaining_high_usd))
      : "—";
  const fractionDelta = (Number(w.observed_fraction_units || 0) / 1_000_000).toFixed(6) + "%";
  const evidence = !known
    ? "Оценка появится после подтверждённого изменения квоты с соответствующим расходом."
    : `доверительный интервал capacity ${capacityRange} · remaining ${remainingRange} · evidence ${nanoMoney(
        w.observed_spend_nano,
      )} / Δquota ${fractionDelta} · ${Number(w.samples || 0)} интервала · confidence ${Math.round(
        Number(w.confidence || 0) * 100,
      )}%`;
  return (
    <div title={evidence}>
      <b>{remaining}</b>
      <div className="sub">
        {known
          ? `остаток из ${capacity} · ${windowLabel(w.window_minutes)}`
          : `накапливаем расход и изменение квоты · ${windowLabel(w.window_minutes)}`}
      </div>
    </div>
  );
}

const resetCell = (w: CodexRateLimit | undefined, nowSec: number): string =>
  w?.resets_at ? duration(w.resets_at - nowSec) : "—";

function GptRow({ home, nowSec }: { home: CodexHome; nowSec: number }): ReactElement {
  const windows = home.windows ?? [];
  const bySlot = (slot: string) => windows.find((w) => w.slot === slot);
  const primary = bySlot("primary");
  const secondary = bySlot("secondary");
  const status = homeStatus(home, nowSec);
  const rateLimits = home.rate_limits ?? {};
  const email = home.email?.trim();
  return (
    <tr>
      <td className="left">
        <b className={email ? undefined : "mono"}>{email || home.id || "—"}</b>
        {email && home.id ? <div className="sub mono">{home.id}</div> : null}
      </td>
      <td>
        <Pill kind={status.kind}>{status.label}</Pill>
      </td>
      <td>{home.inflight ?? 0}</td>
      <td>
        {gptWindowCell(primary)}
        {primary ? <div className="sub">сброс {resetCell(rateLimits.primary, nowSec)}</div> : null}
      </td>
      <td>
        {gptWindowCell(secondary)}
        {secondary ? <div className="sub">сброс {resetCell(rateLimits.secondary, nowSec)}</div> : null}
      </td>
      <td>
        {gptBudgetCell(primary)}
        {secondary ? (
          <>
            <div className="sub" style={{ marginTop: 5 }}>
              secondary
            </div>
            {gptBudgetCell(secondary)}
          </>
        ) : null}
      </td>
      <td>
        <b>{home.spend_nano_total != null ? nanoMoney(home.spend_nano_total) : money(home.spend_usd_total)}</b>
        <div className="sub">official-price</div>
      </td>
    </tr>
  );
}

export const GptTable = memo(function GptTable({
  homes,
  nowMs,
}: {
  homes: CodexHome[];
  /** Момент снимка (мс) из poller'а — отсчёты «до сброса» считаются от него. */
  nowMs: number;
}): ReactElement {
  const nowSec = (nowMs / 1000) | 0;
  return (
    <TableCard>
      <table>
        <thead>
          <tr>
            <th className="left">аккаунт / home</th>
            <th>статус</th>
            <th>в работе</th>
            <th>primary (факт. окно)</th>
            <th>secondary (факт. окно)</th>
            <th>остаток / вместимость API $</th>
            <th>потрачено</th>
          </tr>
        </thead>
        <tbody>
          {homes.length ? (
            homes.map((home, index) => <GptRow key={home.id ?? index} home={home} nowSec={nowSec} />)
          ) : (
            <EmptyRow columns={7} />
          )}
        </tbody>
      </table>
    </TableCard>
  );
});

/* ── Gemini ─────────────────────────────────────────────── */

function geminiWindow(profile: GeminiProfile, kind: string) {
  return (profile.windows ?? []).find((item) => item.window_kind === kind);
}

// Окно профиля в том же компактном формате, что GPT: бар и время сброса.
function geminiWindowCell(profile: GeminiProfile, kind: string, nowSec: number): ReactNode {
  const window = geminiWindow(profile, kind);
  if (!window) return "—";
  return (
    <>
      <div>
        <Bar spec={barFromRemaining(window.remaining_fraction)} />
      </div>
      <div className="sub">
        сброс {window.resets_at ? duration(Math.max(0, window.resets_at - nowSec)) : "—"}
      </div>
    </>
  );
}

// Остаток / вместимость в API-$; workload evidence остаётся доступным в title,
// но не растягивает основную таблицу подписок.
function geminiBudgetCell(profile: GeminiProfile, kind: string, label: string): ReactNode {
  const window = (profile.windows ?? []).find((item) => item.window_kind === kind);
  if (!window) return "—";
  const known = window.source === "workload_blend";
  const range =
    known && window.low_usd != null
      ? money(window.low_usd) + "–" + (window.high_usd == null ? "∞" : money(window.high_usd))
      : "—";
  const quotaDelta = (Number(window.observed_fraction_units || 0) / 1_000_000).toFixed(5) + "%";
  const evidence = !known
    ? "ждём полный интервал"
    : `workload envelope ${range} · evidence ${money(window.observed_spend_usd)} / Δquota ${quotaDelta} · ${Number(
        window.samples || 0,
      )} интервала · confidence ${Math.round(Number(window.confidence || 0) * 100)}%`;
  return (
    <div title={evidence}>
      <b>{window.remaining_usd == null ? "—" : money(window.remaining_usd)}</b>
      <div className="sub">
        остаток из {window.cap_usd == null ? "—" : money(window.cap_usd)} · {label}
      </div>
    </div>
  );
}

function GeminiModelCoverage({
  profile,
  models,
  nowSec,
}: {
  profile: GeminiProfile;
  models: GeminiModel[];
  nowSec: number;
}): ReactElement {
  const health = profile.model_cooling ?? [];
  const total = models.length || health.length;
  const available = profile.authenticated
    ? health.filter((model) => Number(model.cooling_until || 0) <= nowSec).length
    : 0;
  const degraded = health.filter((model) => Number(model.failure_streak || 0) > 0).length;
  const unknown = health.filter(
    (model) => Number(model.last_success_at || 0) === 0 && Number(model.last_failure_at || 0) === 0,
  ).length;
  return (
    <div title={`${degraded} degraded · ${unknown} без probe`}>
      <b>{health.length && total ? `${available}/${total}` : "—"}</b> доступны
      <div className="sub">
        {degraded ? `${degraded} degraded` : "без деградации"}
        {unknown ? ` · ${unknown} без probe` : ""}
      </div>
    </div>
  );
}

function GeminiRow({
  profile,
  models,
  nowSec,
}: {
  profile: GeminiProfile;
  models: GeminiModel[];
  nowSec: number;
}): ReactElement {
  const status = geminiProfileStatus(profile, nowSec);
  return (
    <tr>
      <td className="left">
        <b className="mono">{profile.id}</b>
      </td>
      <td>
        <Pill kind={status.kind}>{status.label}</Pill>
      </td>
      <td>{profile.inflight ?? 0}</td>
      <td>{geminiWindowCell(profile, "5h", nowSec)}</td>
      <td>{geminiWindowCell(profile, "weekly", nowSec)}</td>
      <td>
        {geminiBudgetCell(profile, "5h", "5ч")}
        {geminiWindow(profile, "weekly") ? (
          <div style={{ marginTop: 5 }}>
            {geminiBudgetCell(profile, "weekly", "7д")}
          </div>
        ) : null}
      </td>
      <td>
        <GeminiModelCoverage profile={profile} models={models} nowSec={nowSec} />
      </td>
      <td>
        <b>{money(profile.spend_usd_total)}</b>
        <div className="sub">
          probe {profile.last_probe_at ? duration(Math.max(0, nowSec - profile.last_probe_at)) + " назад" : "—"}
        </div>
        <div className="sub">
          quota {profile.quota_updated_at ? duration(Math.max(0, nowSec - profile.quota_updated_at)) + " назад" : "—"}
        </div>
      </td>
    </tr>
  );
}

export const GeminiTable = memo(function GeminiTable({
  profiles,
  models,
  now,
  nowMs,
}: {
  profiles: GeminiProfile[];
  models: GeminiModel[];
  /** Поле now из /gemini-subs (epoch-секунды по часам runtime); 0/отсутствие → момент снимка. */
  now?: number;
  /** Момент снимка (мс) из poller'а — fallback и база для отсчётов. */
  nowMs: number;
}): ReactElement {
  const nowSec = Number(now || nowMs / 1000);
  return (
    <TableCard>
      <table>
        <thead>
          <tr>
            <th className="left">профиль</th>
            <th>статус</th>
            <th>в работе</th>
            <th>окно 5 ч</th>
            <th>окно 7 д</th>
            <th>остаток / вместимость API $</th>
            <th>модели</th>
            <th>потрачено / probe</th>
          </tr>
        </thead>
        <tbody>
          {profiles.length ? (
            profiles.map((profile, index) => (
              <GeminiRow key={profile.id ?? index} profile={profile} models={models} nowSec={nowSec} />
            ))
          ) : (
            <EmptyRow columns={8} />
          )}
        </tbody>
      </table>
    </TableCard>
  );
});

function modelQuotaCell(model: GeminiModel, profiles: GeminiProfile[]): ReactNode {
  const quotaProfiles = profiles
    .map((profile) => (profile.quotas ?? []).filter((quota) => quota.model_id === model.id))
    .filter((quotas) => quotas.length > 0);
  const profileFractions = quotaProfiles
    .map((quotas) =>
      quotas
        .map((quota) => quota.remaining_fraction)
        .filter((value): value is number => value != null && Number.isFinite(value)),
    )
    .filter((fractions) => fractions.length > 0)
    .map((fractions) => Math.min(...fractions));
  if (!quotaProfiles.length) return "—";
  if (!profileFractions.length) return <span className="sub">официальный bucket · fraction не опубликован</span>;
  const remaining = Math.min(...profileFractions);
  return (
    <>
      <div>
        <Bar spec={barFromRemaining(remaining)} />
      </div>
      <div className="sub">
        осталось {Math.round(remaining * 100)}% · metadata {profileFractions.length}/{profiles.length} профилей
      </div>
    </>
  );
}

export const GeminiModelDetails = memo(function GeminiModelDetails({
  profiles,
  models,
  now,
  nowMs,
}: {
  profiles: GeminiProfile[];
  models: GeminiModel[];
  now?: number;
  nowMs: number;
}): ReactElement {
  const nowSec = Number(now || nowMs / 1000);
  return (
    <details>
      <summary>Каталог Gemini · {count(models.length, "модель", "модели", "моделей")}</summary>
      <TableCard>
        <table>
          <thead>
            <tr>
              <th className="left">модель</th>
              <th>доступно профилей</th>
              <th>health</th>
              <th>официальная quota</th>
              <th>ближайший reset</th>
            </tr>
          </thead>
          <tbody>
            {models.length ? (
              models.map((model, index) => {
                const healthy = Number(model.healthy || 0);
                const degraded = Number(model.degraded || 0);
                const unknown = Number(model.unknown || 0);
                const resets = profiles
                  .flatMap((profile) => (profile.quotas ?? []).filter((quota) => quota.model_id === model.id))
                  .map((quota) => Date.parse(quota.reset_time ?? "") / 1000)
                  .filter((reset) => Number.isFinite(reset) && reset > 0);
                const nearestReset = resets.length ? Math.min(...resets) : 0;
                return (
                  <tr key={model.id ?? index}>
                    <td className="left">
                      <b>{model.id ?? "—"}</b>
                    </td>
                    <td>
                      <b>{model.available ?? 0}</b>/{profiles.length}
                      {model.soonest_ready ? (
                        <div className="sub">следующий {duration(Math.max(0, model.soonest_ready - nowSec))}</div>
                      ) : null}
                    </td>
                    <td>
                      {healthy > 0 ? <Pill kind="ok">{healthy} healthy</Pill> : null}{" "}
                      {degraded > 0 ? <Pill kind="warn">{degraded} degraded</Pill> : null}{" "}
                      {unknown > 0 ? <Pill>{unknown} без probe</Pill> : null}
                      {!healthy && !degraded && !unknown ? <Pill>нет health snapshot</Pill> : null}
                    </td>
                    <td>{modelQuotaCell(model, profiles)}</td>
                    <td>
                      {nearestReset ? (
                        <>
                          <b>{duration(Math.max(0, nearestReset - nowSec))}</b>
                          <div className="sub">{formatDate(nearestReset * 1000, true)}</div>
                        </>
                      ) : (
                        "—"
                      )}
                    </td>
                  </tr>
                );
              })
            ) : (
              <EmptyRow columns={5} text="каталог моделей пуст" />
            )}
          </tbody>
        </table>
      </TableCard>
    </details>
  );
});

/* ── Gemini transport fingerprint и cache/affinity ──────── */

export const TransportDetails = memo(function TransportDetails({
  transport,
  affinity,
}: {
  transport: GeminiTransport;
  affinity: GeminiAffinity;
}): ReactElement {
  return (
    <details>
      <summary>Gemini transport fingerprint и cache/affinity</summary>
      <TableCard>
        <table>
          <tbody>
            <tr>
              <th className="left">Antigravity</th>
              <td className="left mono">{transport.antigravity_version || "—"}</td>
              <th className="left">Node</th>
              <td className="left mono">
                {transport.node_version || "—"} · {transport.http_version || "—"}
              </td>
            </tr>
            <tr>
              <th className="left">transport profile</th>
              <td className="left mono">{transport.profile || "—"}</td>
              <th className="left">Node SHA-256</th>
              <td className="left mono">{transport.node_sha256 || "—"}</td>
            </tr>
            <tr>
              <th className="left">expected JA3</th>
              <td className="left mono">{transport.expected_ja3 || "—"}</td>
              <th className="left">expected JA4</th>
              <td className="left mono">{transport.expected_ja4 || "—"}</td>
            </tr>
            <tr>
              <th className="left">userinfo fetch</th>
              <td className="left mono">
                {transport.userinfo_profile || "—"} · {transport.userinfo_http_version || "—"}
              </td>
              <th className="left">Undici JA3 / JA4</th>
              <td className="left mono">
                {transport.userinfo_expected_ja3 || "—"} / {transport.userinfo_expected_ja4 || "—"}
              </td>
            </tr>
            <tr>
              <th className="left">affinity hits</th>
              <td className="left mono">
                local {affinity.local_hits || 0} · redis {affinity.redis_hits || 0} · roots {affinity.cache_root_hits || 0}
              </td>
              <th className="left">affinity health</th>
              <td className="left mono">
                miss {affinity.misses || 0} · redis errors {affinity.redis_errors || 0} · rebinds {affinity.rebinds || 0}
              </td>
            </tr>
          </tbody>
        </table>
      </TableCard>
    </details>
  );
});
