"use client";

import { useMemo, useState, type ReactElement, type ReactNode } from "react";
import { Pill, TableCard } from "@/components/ui";
import { duration, formatDate, nanoCredits, nanoMoney, windowLabel } from "@/lib/format";
import {
  CODEX_WORKLOAD_PRESETS,
  creditsToApiNanousd,
  formatCodexTokenCount,
  priceCodexWorkload,
  sumCodexIntegers,
  type CodexServiceTier,
  type CodexWorkloadInput,
  type CodexWorkloadPrice,
} from "./codex-calibration";
import { homeStatus } from "./logic";
import type {
  CodexCalibrationEvidence,
  CodexConversionModel,
  CodexHome,
  CodexHomeWindow,
  CodexSubsResponse,
  CodexWindowTotal,
} from "./types";

type PresetId = keyof typeof CODEX_WORKLOAD_PRESETS | "custom";

interface EvidenceRow {
  home: CodexHome;
  evidence: CodexCalibrationEvidence;
}

const WORKLOAD_FIELDS: Array<{ key: keyof CodexWorkloadInput; label: string; hint: string }> = [
  { key: "freshInputTokens", label: "Fresh input", hint: "не cache-read и не cache-write" },
  { key: "cachedInputTokens", label: "Cached input", hint: "дешёвый cache read" },
  { key: "cacheWriteInputTokens", label: "Cache write", hint: "API: отдельная ставка · credits: fresh input" },
  { key: "outputTokens", label: "Output", hint: "включает reasoning" },
  { key: "reasoningOutputTokens", label: "Reasoning", hint: "диагностика · subset output" },
];

const fractionPercent = (fractionUnits: number | string | null | undefined, digits = 6): string => {
  const value = Number(fractionUnits ?? 0) / 1_000_000;
  return `${value.toFixed(digits).replace(/0+$/, "").replace(/\.$/, "")}%`;
};

const integerOrZero = (value: string | null | undefined): bigint => {
  try {
    return BigInt(value ?? "0");
  } catch {
    return 0n;
  }
};

function calibrationState(home: CodexHome, evidenceAvailable: boolean): { label: string; kind: "ok" | "warn" | "bad" } {
  const dropped = Number(home.calibration_dropped_events ?? 0);
  const pending = Number(home.calibration_pending_events ?? 0);
  const evidenceTurns = (home.calibration_evidence ?? []).reduce((sum, row) => sum + Number(row.turns ?? 0), 0);
  const windows = home.windows ?? [];
  const measured = windows.filter((window) => window.capacity_nanocredits != null);
  const creditSamples = measured.reduce((sum, window) => sum + Number(window.credit_samples ?? 0), 0);
  const unattributed = windows.reduce((sum, window) => sum + Number(window.unattributed_fraction_units ?? 0), 0);

  if (dropped > 0) return { label: `ошибка integrity · dropped ${dropped}`, kind: "bad" };
  if (pending > 0) return { label: `persistence queue · ${pending}`, kind: "warn" };
  if (home.calibration_persistence_ok === false) return { label: "calibration storage", kind: "bad" };
  if (!evidenceAvailable) return { label: "ledger недоступен", kind: "bad" };
  if (evidenceTurns === 0) return { label: "нет turn evidence", kind: "warn" };
  if (measured.length === 0) return { label: "exact spend · ждём Δquota", kind: "warn" };
  if (unattributed > 0) return { label: "возможно неатрибутировано", kind: "warn" };
  if (creditSamples < measured.length * 3) return { label: "калибровка созревает", kind: "warn" };
  return { label: "representative", kind: "ok" };
}

function exactHomeTotals(home: CodexHome): { turns: number; credits: bigint | null; api: bigint | null } {
  const evidence = home.calibration_evidence ?? [];
  return {
    turns: evidence.reduce((sum, row) => sum + Number(row.turns ?? 0), 0),
    credits: sumCodexIntegers(evidence.map((row) => row.chatgpt_total_nanocredits)),
    api: sumCodexIntegers(evidence.map((row) => row.api_total_nanousd)),
  };
}

function NativeApiValue({
  credits,
  workload,
}: {
  credits: string | null | undefined;
  workload: CodexWorkloadPrice | null;
}): ReactElement {
  const api = creditsToApiNanousd(credits, workload);
  if (credits == null) return <span className="codex-missing">ждём Δquota</span>;
  return (
    <span className="codex-pair-value">
      <span className="credit-ink">{nanoCredits(credits)}</span>
      <i aria-hidden="true">↔</i>
      <span className="usd-ink">{api == null ? "—" : nanoMoney(api)}</span>
    </span>
  );
}

function WindowEvidence({
  window,
  workload,
  nowSec,
}: {
  window: CodexHomeWindow | undefined;
  workload: CodexWorkloadPrice | null;
  nowSec: number;
}): ReactElement {
  if (!window) return <span className="codex-missing">окно не опубликовано</span>;
  const capacityRange =
    window.low_nanocredits == null
      ? "границы ещё не измерены"
      : `${nanoCredits(window.low_nanocredits)} – ${
          window.high_nanocredits == null ? "∞" : nanoCredits(window.high_nanocredits)
        }`;
  const reset = window.resets_at ? duration(window.resets_at - nowSec) : "—";
  const unattributed = Number(window.unattributed_fraction_units ?? 0);
  return (
    <div className="codex-window-evidence">
      <div className="codex-window-heading">
        <b>{windowLabel(window.window_minutes)}</b>
        <span>{fractionPercent(window.used_fraction_units ?? (window.used_percent ?? 0) * 1_000_000, 4)} used</span>
        <span>reset {reset}</span>
      </div>
      <NativeApiValue credits={window.remaining_nanocredits} workload={workload} />
      <div className="codex-window-detail">
        {window.capacity_nanocredits == null ? (
          <>Native capacity появится после положительного движения quota с уже записанным turn.</>
        ) : (
          <>
            capacity <b className="credit-ink">{nanoCredits(window.capacity_nanocredits)}</b> · диапазон {capacityRange}
          </>
        )}
      </div>
      <div className="codex-window-detail">
        exact tracked {nanoCredits(window.observed_spend_nanocredits)} · Δquota {fractionPercent(window.observed_fraction_units)} · samples{" "}
        {Number(window.credit_samples ?? 0)}
        {window.confidence == null ? "" : ` · confidence ${Math.round(Number(window.confidence) * 100)}%`}
      </div>
      {unattributed > 0 ? (
        <div className="codex-uncertainty">возможно неатрибутировано: Δquota {fractionPercent(unattributed)}</div>
      ) : null}
    </div>
  );
}

function SummaryDatum({ label, value, hint, tone }: { label: string; value: ReactNode; hint: string; tone?: string }) {
  return (
    <div className={`codex-summary-card ${tone ?? ""}`}>
      <span>{label}</span>
      <strong>{value}</strong>
      <small>{hint}</small>
    </div>
  );
}

function WorkloadBridge({
  models,
  totals,
  selectedModelId,
  onModelChange,
  tier,
  onTierChange,
  preset,
  onPresetChange,
  workload,
  onWorkloadChange,
}: {
  models: CodexConversionModel[];
  totals: CodexWindowTotal[];
  selectedModelId: string;
  onModelChange: (modelId: string) => void;
  tier: CodexServiceTier;
  onTierChange: (tier: CodexServiceTier) => void;
  preset: PresetId;
  onPresetChange: (preset: PresetId) => void;
  workload: CodexWorkloadInput;
  onWorkloadChange: (key: keyof CodexWorkloadInput, value: string) => void;
}): ReactElement {
  const model = models.find((item) => item.id === selectedModelId) ?? models[0];
  const priced = useMemo(
    () => (model ? priceCodexWorkload(model, workload, tier) : { ok: false as const, error: "Каталог моделей пуст" }),
    [model, tier, workload],
  );
  const price = priced.ok ? priced.value : null;
  const priceError = priced.ok ? null : priced.error;
  const fiveHour = totals.find((item) => Number(item.window_minutes) === 300);
  const weekly = totals.find((item) => Number(item.window_minutes) === 10_080);

  return (
    <section className="codex-workbench" aria-label="Калибровочный мост ChatGPT credits и API USD">
      <div className="codex-workbench-head">
        <div>
          <span className="codex-overline">Selected workload</span>
          <h3>ChatGPT credits ↔ API USD</h3>
          <p>Native capacity не меняется. Фиолетовый USD пересчитывается только под выбранную модель, режим и token mix.</p>
        </div>
        <div className="codex-schedule-stamp">
          <span>credit card</span>
          <b>{model?.credit_schedule_id ?? "—"}</b>
          <span>API tariff</span>
          <b>{model?.api_tariff_schedule_id ?? "—"}</b>
        </div>
      </div>

      <div className="codex-workbench-grid">
        <div className="codex-controls">
          <label>
            <span>Модель</span>
            <select value={model?.id ?? ""} onChange={(event) => onModelChange(event.target.value)}>
              {models.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.id}
                </option>
              ))}
            </select>
          </label>
          <fieldset>
            <legend>Режим</legend>
            <div className="codex-tier-switch">
              {(["standard", "fast"] as const).map((value) => (
                <button
                  className={tier === value ? "on" : ""}
                  key={value}
                  type="button"
                  aria-pressed={tier === value}
                  onClick={() => onTierChange(value)}
                >
                  {value === "standard" ? "Standard" : "Fast"}
                </button>
              ))}
            </div>
          </fieldset>
          <label>
            <span>Профиль нагрузки</span>
            <select value={preset} onChange={(event) => onPresetChange(event.target.value as PresetId)}>
              <option value="review">Code review</option>
              <option value="agent">Agent turn</option>
              <option value="long">Long context</option>
              <option value="custom">Свой token mix</option>
            </select>
          </label>
          <div className="codex-token-grid">
            {WORKLOAD_FIELDS.map((field) => (
              <label key={field.key}>
                <span>{field.label}</span>
                <input
                  inputMode="numeric"
                  value={workload[field.key]}
                  aria-invalid={!/^(0|[1-9][0-9]*)$/.test(workload[field.key])}
                  onChange={(event) => onWorkloadChange(field.key, event.target.value)}
                />
                <small>{field.hint}</small>
              </label>
            ))}
          </div>
          {!priced.ok ? <div className="codex-form-error">{priced.error}</div> : null}
        </div>

        <div className="codex-bridge-output">
          {price ? (
            <>
              <div className="codex-dual-rail">
                <div className="credit-rail">
                  <span>На один выбранный turn</span>
                  <strong>{nanoCredits(price.credits.totalNanocredits)}</strong>
                  <small>
                    fresh+write {nanoCredits(price.credits.freshAndWriteNanocredits)} · cached{" "}
                    {nanoCredits(price.credits.cachedInputNanocredits)} · output {nanoCredits(price.credits.outputNanocredits)}
                  </small>
                </div>
                <div className="rail-junction" aria-hidden="true">
                  <i />
                  <b>↔</b>
                  <i />
                </div>
                <div className="usd-rail">
                  <span>Публичный API equivalent</span>
                  <strong>{nanoMoney(price.api.totalNanousd)}</strong>
                  <small>
                    fresh {nanoMoney(price.api.freshInputNanousd)} · cached {nanoMoney(price.api.cachedInputNanousd)} · write{" "}
                    {nanoMoney(price.api.cacheWriteNanousd)} · output {nanoMoney(price.api.outputNanousd)}
                  </small>
                </div>
              </div>
              <div className="codex-context-line">
                <span>{formatCodexTokenCount(price.totalInputTokens)} total input</span>
                <Pill kind={price.longContext ? "warn" : "info"}>
                  {price.longContext ? "long-context API multipliers active" : "standard context"}
                </Pill>
                <span>
                  API Fast ×{((model?.api.fast_multiplier_basis_points ?? 10_000) / 10_000).toFixed(1)} · subscription Fast ×
                  {((model?.chatgpt_credits.fast_multiplier_basis_points ?? 10_000) / 10_000).toFixed(1)}
                </span>
              </div>
              <div className="codex-window-bridge-grid">
                {[
                  ["5 часов", fiveHour],
                  ["7 дней", weekly],
                ].map(([label, item]) => {
                  const total = item as CodexWindowTotal | undefined;
                  return (
                    <div className="codex-window-bridge" key={label as string}>
                      <span>{label as string} · весь пул</span>
                      <NativeApiValue credits={total?.remaining_nanocredits} workload={price} />
                      <small>
                        remaining · capacity{" "}
                        {total?.capacity_nanocredits == null ? (
                          "ждёт Δquota"
                        ) : (
                          <>
                            <b className="credit-ink">{nanoCredits(total.capacity_nanocredits)}</b> ↔{" "}
                            <b className="usd-ink">
                              {(() => {
                                const equivalent = creditsToApiNanousd(total.capacity_nanocredits, price);
                                return equivalent == null ? "—" : nanoMoney(equivalent);
                              })()}
                            </b>
                          </>
                        )}
                      </small>
                    </div>
                  );
                })}
              </div>
            </>
          ) : (
            <div className="codex-no-catalog">Невозможно рассчитать эквивалент: {priceError}</div>
          )}
        </div>
      </div>
    </section>
  );
}

function HomeTable({
  homes,
  evidenceAvailable,
  nowMs,
  workload,
}: {
  homes: CodexHome[];
  evidenceAvailable: boolean;
  nowMs: number;
  workload: CodexWorkloadPrice | null;
}): ReactElement {
  const nowSec = Math.floor(nowMs / 1000);
  return (
    <TableCard>
      <table className="codex-operator-table codex-sticky-table">
        <thead>
          <tr>
            <th className="left">почта / home</th>
            <th className="left">состояние evidence</th>
            <th className="left">runtime</th>
            <th className="left">primary · credits ↔ API USD</th>
            <th className="left">secondary · credits ↔ API USD</th>
            <th className="left">immutable turns</th>
            <th className="left">integrity</th>
          </tr>
        </thead>
        <tbody>
          {homes.length ? (
            homes.map((home, index) => {
              const state = calibrationState(home, evidenceAvailable);
              const runtime = homeStatus(home, nowSec);
              const totals = exactHomeTotals(home);
              const bySlot = (slot: string) => (home.windows ?? []).find((window) => window.slot === slot);
              return (
                <tr key={home.id ?? index}>
                  <td className="left codex-sticky-identity">
                    <b>{home.email?.trim() || "masked email unavailable"}</b>
                    <div className="sub mono">{home.id ?? "—"}</div>
                  </td>
                  <td className="left">
                    <Pill kind={state.kind}>{state.label}</Pill>
                  </td>
                  <td className="left">
                    <Pill kind={runtime.kind}>{runtime.label}</Pill>
                    <div className="sub">{home.plan ?? "plan unknown"} · inflight {home.inflight ?? 0}</div>
                  </td>
                  <td className="left">
                    <WindowEvidence window={bySlot("primary")} workload={workload} nowSec={nowSec} />
                  </td>
                  <td className="left">
                    <WindowEvidence window={bySlot("secondary")} workload={workload} nowSec={nowSec} />
                  </td>
                  <td className="left">
                    <b>{totals.turns} turns</b>
                    <div className="sub credit-ink">{totals.credits == null ? "—" : nanoCredits(totals.credits)}</div>
                    <div className="sub usd-ink">{totals.api == null ? "—" : nanoMoney(totals.api)}</div>
                  </td>
                  <td className="left">
                    <b>pending {home.calibration_pending_events ?? 0} · dropped {home.calibration_dropped_events ?? 0}</b>
                    <div className="sub">
                      tracking {home.credit_tracking_started_ts ? formatDate(home.credit_tracking_started_ts * 1000, true) : "не начат"}
                    </div>
                  </td>
                </tr>
              );
            })
          ) : (
            <tr>
              <td className="empty" colSpan={7}>Homes ещё не опубликованы.</td>
            </tr>
          )}
        </tbody>
      </table>
    </TableCard>
  );
}

function EvidenceLedger({ rows, available }: { rows: EvidenceRow[]; available: boolean }): ReactElement {
  return (
    <section className="codex-ledger">
      <div className="codex-ledger-head">
        <div>
          <span className="codex-overline">Immutable evidence ledger</span>
          <h3>Каждый успешный turn виден сразу</h3>
        </div>
        <Pill kind={!available ? "bad" : rows.length ? "ok" : "warn"}>
          {!available ? "read path unavailable" : rows.length ? `${rows.length} aggregates` : "waiting for first turn"}
        </Pill>
      </div>
      <TableCard>
        <table className="codex-ledger-table codex-sticky-table">
          <thead>
            <tr>
              <th className="left">почта / home</th>
              <th className="left">модель / tier</th>
              <th className="left">turns / период</th>
              <th className="left">input: fresh / cached / write</th>
              <th className="left">output / reasoning</th>
              <th className="left">ChatGPT credits</th>
              <th className="left">API USD</th>
              <th className="left">schedules</th>
            </tr>
          </thead>
          <tbody>
            {rows.length ? (
              rows.map(({ home, evidence }, index) => {
                const input = integerOrZero(evidence.input_tokens);
                const cached = integerOrZero(evidence.cached_input_tokens);
                const write = integerOrZero(evidence.cache_write_input_tokens);
                const fresh = input > cached + write ? input - cached - write : 0n;
                return (
                  <tr key={`${home.id ?? "home"}-${evidence.model ?? "model"}-${evidence.service_tier ?? "tier"}-${index}`}>
                    <td className="left codex-sticky-identity">
                      <b>{home.email?.trim() || "masked email unavailable"}</b>
                      <div className="sub mono">{home.id ?? "—"}</div>
                    </td>
                    <td className="left">
                      <b>{evidence.model ?? "—"}</b>
                      <div className="sub">
                        {evidence.service_tier ?? "standard"} · provider {evidence.provider_reported_tier ?? "not reported"}
                      </div>
                    </td>
                    <td className="left">
                      <b>{Number(evidence.turns ?? 0)}</b>
                      <div className="sub">
                        {evidence.first_completed_at ? formatDate(evidence.first_completed_at * 1000, true) : "—"} →{" "}
                        {evidence.last_completed_at ? formatDate(evidence.last_completed_at * 1000, true) : "—"}
                      </div>
                    </td>
                    <td className="left">
                      <b>
                        {formatCodexTokenCount(fresh)} / {formatCodexTokenCount(cached)} / {formatCodexTokenCount(write)}
                      </b>
                      <div className="sub">total input {formatCodexTokenCount(input)}</div>
                    </td>
                    <td className="left">
                      <b>
                        {formatCodexTokenCount(evidence.output_tokens)} / {formatCodexTokenCount(evidence.reasoning_output_tokens)}
                      </b>
                      <div className="sub">reasoning уже входит в output</div>
                    </td>
                    <td className="left credit-ink">
                      <b>{nanoCredits(evidence.chatgpt_total_nanocredits)}</b>
                      <div className="sub">
                        fresh {nanoCredits(evidence.chatgpt_input_nanocredits)} · cached{" "}
                        {nanoCredits(evidence.chatgpt_cached_input_nanocredits)} · output{" "}
                        {nanoCredits(evidence.chatgpt_output_nanocredits)}
                      </div>
                    </td>
                    <td className="left usd-ink">
                      <b>{nanoMoney(evidence.api_total_nanousd)}</b>
                      <div className="sub">
                        in {nanoMoney(evidence.api_input_nanousd)} · cache {nanoMoney(evidence.api_cached_input_nanousd)} · write{" "}
                        {nanoMoney(evidence.api_cache_write_nanousd)} · out {nanoMoney(evidence.api_output_nanousd)}
                      </div>
                    </td>
                    <td className="left">
                      <div className="mono">{evidence.credit_schedule_id ?? "—"}</div>
                      <div className="sub mono">{evidence.api_tariff_schedule_id ?? "—"}</div>
                    </td>
                  </tr>
                );
              })
            ) : (
              <tr>
                <td className="empty" colSpan={8}>
                  {available
                    ? "Нет успешного turn после включения immutable ledger. Первый точный расход появится здесь сразу, capacity — позже после Δquota."
                    : "Runtime не смог прочитать immutable evidence ledger. Проверьте calibration storage; отсутствие строк сейчас не считается нулевым расходом."}
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </TableCard>
    </section>
  );
}

export function CodexCalibrationLab({ response, nowMs }: { response: CodexSubsResponse; nowMs: number }): ReactElement {
  const homes = response.homes ?? [];
  const totals = response.window_totals ?? [];
  const models = response.conversion_models ?? [];
  const [selectedModelId, setSelectedModelId] = useState(models[0]?.id ?? "");
  const [tier, setTier] = useState<CodexServiceTier>("standard");
  const [preset, setPreset] = useState<PresetId>("agent");
  const [workload, setWorkload] = useState<CodexWorkloadInput>(CODEX_WORKLOAD_PRESETS.agent);
  const evidenceAvailable = response.calibration_evidence_available === true;
  const evidenceRows: EvidenceRow[] = homes.flatMap((home) =>
    (home.calibration_evidence ?? []).map((evidence) => ({ home, evidence })),
  );
  const exactTurns = evidenceRows.reduce((sum, row) => sum + Number(row.evidence.turns ?? 0), 0);
  const exactCredits = sumCodexIntegers(evidenceRows.map((row) => row.evidence.chatgpt_total_nanocredits));
  const exactApi = sumCodexIntegers(evidenceRows.map((row) => row.evidence.api_total_nanousd));
  const modelsObserved = new Set(evidenceRows.map((row) => row.evidence.model).filter(Boolean)).size;
  const pending = homes.reduce((sum, home) => sum + Number(home.calibration_pending_events ?? 0), 0);
  const dropped = homes.reduce((sum, home) => sum + Number(home.calibration_dropped_events ?? 0), 0);
  const primary = totals.find((item) => Number(item.window_minutes) === 300);
  const weekly = totals.find((item) => Number(item.window_minutes) === 10_080);
  const selectedModel = models.find((item) => item.id === selectedModelId) ?? models[0];
  const selectedPriceResult = selectedModel ? priceCodexWorkload(selectedModel, workload, tier) : null;
  const selectedPrice = selectedPriceResult?.ok ? selectedPriceResult.value : null;
  const choosePreset = (next: PresetId) => {
    setPreset(next);
    if (next !== "custom") setWorkload(CODEX_WORKLOAD_PRESETS[next]);
  };
  const editWorkload = (key: keyof CodexWorkloadInput, value: string) => {
    setPreset("custom");
    setWorkload((current) => ({ ...current, [key]: value }));
  };

  return (
    <div className="codex-lab">
      <div className="codex-summary">
        <SummaryDatum
          label="Native remaining credits"
          value={
            <>
              5ч <span className="credit-ink">{primary?.remaining_nanocredits == null ? "—" : nanoCredits(primary.remaining_nanocredits)}</span>
            </>
          }
          hint={`7д ${weekly?.remaining_nanocredits == null ? "ждёт Δquota" : nanoCredits(weekly.remaining_nanocredits)}`}
          tone="credit"
        />
        <SummaryDatum
          label="Exact tracked credits"
          value={exactCredits == null ? "—" : nanoCredits(exactCredits)}
          hint={`${exactTurns} immutable turns · tracking after runtime cutover`}
          tone="credit"
        />
        <SummaryDatum
          label="Immutable evidence"
          value={`${exactTurns} turns`}
          hint={`${modelsObserved} моделей · ${evidenceRows.length} model/tier aggregates`}
        />
        <SummaryDatum
          label="Exact API equivalent"
          value={exactApi == null ? "—" : nanoMoney(exactApi)}
          hint="фактические модели и token classes, не выбранный workload"
          tone="usd"
        />
        <SummaryDatum
          label="Persistence integrity"
          value={`${pending} / ${dropped}`}
          hint="pending / dropped · норма 0 / 0"
          tone={dropped ? "bad" : pending ? "uncertain" : ""}
        />
      </div>

      {models.length ? (
        <WorkloadBridge
          models={models}
          totals={totals}
          selectedModelId={selectedModelId}
          onModelChange={setSelectedModelId}
          tier={tier}
          onTierChange={setTier}
          preset={preset}
          onPresetChange={choosePreset}
          workload={workload}
          onWorkloadChange={editWorkload}
        />
      ) : (
        <div className="codex-no-catalog">
          Conversion catalogue отсутствует. Exact turn ledger всё равно остаётся видимым, но workload-dependent API equivalent не рассчитывается.
        </div>
      )}

      <div className="codex-table-head">
        <div>
          <span className="codex-overline">Home calibration matrix</span>
          <h3>Почему одинаковые подписки расходятся</h3>
        </div>
        <p>
          Сравнивайте native credits. USD меняется из-за модели, Standard/Fast, fresh/cache/write/output mix и long context;
          quota может также двигаться вне gateway — это помечается как «возможно неатрибутировано».
        </p>
      </div>
      <HomeTable homes={homes} evidenceAvailable={evidenceAvailable} nowMs={nowMs} workload={selectedPrice} />
      <EvidenceLedger rows={evidenceRows} available={evidenceAvailable} />
    </div>
  );
}
