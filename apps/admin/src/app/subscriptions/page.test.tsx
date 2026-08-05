import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";
import type { ReactNode } from "react";

// next/link вне рантайма Next подменяем обычной ссылкой.
vi.mock("next/link", () => ({
  default: (props: { href: string; children: ReactNode; className?: string; style?: unknown }) => (
    <a href={props.href} className={props.className} style={props.style as never}>
      {props.children}
    </a>
  ),
}));

import SubsPage from "./page";
import { ClaudeTable, GeminiModelDetails, GeminiTable, GptTable, TransportDetails } from "./components";
import { ClaudeCapacityBoard } from "./claude-capacity-board";
import { CodexCapacityBoard } from "./codex-capacity-board";
import { FleetCapacityOverview } from "./fleet-capacity-overview";
import { GeminiCapacityBoard } from "./gemini-capacity-board";
import { GlmCapacityBoard } from "./glm-capacity-board";
import { KimiCapacityBoard } from "./kimi-capacity-board";
import {
  barFromPercent,
  barFromRemaining,
  barFromUtil,
  deadLabel,
  geminiProfileStatus,
  glmFleetUsedPercent,
  glmFleetWindowMoney,
  glmMeasuredCoverage,
  glmProfileStatus,
  glmUsedPercent,
  glmWindowLabel,
  homeStatus,
  kimiFleetUsedPercent,
  kimiFleetWindowMoney,
  kimiMeasuredCoverage,
  kimiProfileStatus,
  kimiUsedPercent,
  kimiWindowLabel,
  resolveBanner,
  stripProxyPort,
} from "./logic";
import type { GlmProfile, GlmSubsResponse, KimiProfile, KimiSubsResponse } from "./types";

const OK_BANNER = {
  dead: 0,
  suspect: 0,
  subsDown: false,
  gptDown: false,
  geminiDown: false,
  geminiEmpty: false,
  gptAuthBad: 0,
  gptProcDown: 0,
  geminiAuthBad: 0,
  geminiUnavailable: false,
  geminiMissing: 0,
  kimiDown: false,
  kimiEmpty: false,
  kimiUnavailable: false,
  glmDown: false,
  glmEmpty: false,
  glmUnavailable: false,
  claudeCount: 3,
  gptSummary: 2,
  geminiSummary: 1,
  kimiSummary: 1,
  glmSummary: 1,
  updatedAt: "31.07.2026, 19:00",
};

// React SSR разделяет текстовые узлы комментариями <!-- --> — для проверок
// склеенных строк нормализуем разметку.
const plain = (html: string): string => html.replace(/<!-- -->/g, "");

describe("таблицы флотов (smoke render с данными)", () => {
  it("FleetCapacityOverview: сверху сопоставляет 5ч и 7д API-$ всех трёх пулов", () => {
    const html = renderToString(
      <FleetCapacityOverview
        claude={{
          calibrated: true,
          per_sub: [{ routable: true }],
          window_totals: [
            { window_minutes: 300, capacity_nano: "60000000000", remaining_nano: "45000000000", routable_subs: 1, calibrated_subs: 1 },
            { window_minutes: 10_080, capacity_nano: "200000000000", remaining_nano: "120000000000" },
          ],
        }}
        gpt={{
          enabled: true,
          available: 1,
          homes: [{ process_live: true, admitted: true }],
          window_totals: [
            { window_minutes: 300, capacity_nano: "80000000000", remaining_nano: "40000000000", measured_homes: 1, observed_homes: 1 },
            { window_minutes: 10_080, capacity_nano: "300000000000", remaining_nano: "210000000000" },
          ],
        }}
        gemini={{
          enabled: true,
          available: 1,
          calibration_authority_available: true,
          calibration_delivery: { pending_events: 0, dropped_events: 0, persistence_ok: true },
          profiles: [{ authenticated: true }],
          window_totals: [
            { window_minutes: 300, capacity_nano: "50000000000", remaining_nano: "30000000000", measured_profiles: 1, observed_profiles: 1 },
            { window_minutes: 10_080, capacity_nano: "250000000000", remaining_nano: "150000000000" },
          ],
        }}
      />,
    );
    expect(html).toContain("Ёмкость пулов");
    expect(html).toContain("Claude");
    expect(html).toContain("GPT");
    expect(html).toContain("Gemini");
    expect(plain(html)).toContain("$45.00");
    expect(plain(html)).toContain("$210.00");
    expect(plain(html)).toContain("$150.00");
    expect(html.match(/fleet-window-rail/g)).toHaveLength(6);
    expect(plain(html)).toContain("25%");
    expect(plain(html)).toContain("50%");
  });

  it("FleetCapacityOverview: сразу показывает очередь или потерю Claude-калибровки", () => {
    const pending = renderToString(
      <FleetCapacityOverview
        claude={{
          calibrated: false,
          calibration_delivery: { pending_events: 2, dropped_events: 0, persistence_ok: false },
          per_sub: [{ routable: true }],
          window_totals: [{ window_minutes: 300, capacity_nano: "60000000000", remaining_nano: null }],
        }}
        gpt={null}
        gemini={null}
      />,
    );
    expect(plain(pending)).toContain("2 сохраняется");
    expect(pending).toContain("fleet-claude");

    const dropped = renderToString(
      <FleetCapacityOverview
        claude={{
          calibrated: false,
          calibration_delivery: { pending_events: 0, dropped_events: 1, persistence_ok: false },
          per_sub: [{ routable: true }],
          window_totals: [{ window_minutes: 300, capacity_nano: "60000000000", remaining_nano: null }],
        }}
        gpt={null}
        gemini={null}
      />,
    );
    expect(plain(dropped)).toContain("1 потеряно");
    expect(dropped).toContain("fleet-state bad");
  });

  it("FleetCapacityOverview: ошибка persistence одного Gemini-профиля скрывает fleet API-$", () => {
    const html = renderToString(
      <FleetCapacityOverview
        claude={null}
        gpt={null}
        gemini={{
          enabled: true,
          available: 1,
          calibration_authority_available: true,
          calibration_delivery: { pending_events: 0, dropped_events: 0, persistence_ok: true },
          profiles: [{ authenticated: true, calibration_persistence_ok: false }],
          window_totals: [
            { window_minutes: 300, capacity_nano: "50000000000", remaining_nano: "30000000000", measured_profiles: 1, observed_profiles: 1 },
            { window_minutes: 10_080, capacity_nano: "200000000000", remaining_nano: "150000000000", measured_profiles: 1, observed_profiles: 1 },
          ],
        }}
      />,
    );
    expect(plain(html)).toContain("ошибка authority");
    expect(plain(html)).not.toContain("$30.00");
    expect(plain(html)).not.toContain("$150.00");
    expect(html).toContain("fleet-gemini");
    expect(html).toContain("fleet-state bad");
  });

  it("ClaudeTable: dead-подписка с пилюлей и live-окнами, пустой список → empty-row", () => {
    const html = renderToString(
      <ClaudeTable
        list={[
          {
            email: "cl***@example.com",
            auth_state: "dead",
            dead_reason: "permission_error",
            dead_since_ts: 1_700_000_000,
            sub_days_left: 5,
            added: "2026-01-10T00:00:00Z",
            proxy_host: "gw.example.com:8080",
            proxy_ok: false,
            proxy_expire: "2026-02-01T00:00:00Z",
          },
        ]}
        liveByEmail={{ "cl***@example.com": { util5h: 0.5, util7d: 0.9, rem5h_usd: 12.5, rem7d_usd: 40 } }}
      />,
    );
    expect(html).toContain("cl***@example.com");
    expect(html).toContain("токен мёртв · бан");
    expect(html).toContain("мертва");
    // Порт обрезан в видимом тексте ячейки; полный host остаётся в title (как в легаси).
    expect(html).toContain(">gw.example.com<");
    expect(html).toContain('title="gw.example.com:8080"');
    expect(plain(html)).toContain("50%");
    expect(html).toContain("$12.50");

    const empty = renderToString(<ClaudeTable list={[]} liveByEmail={{}} />);
    expect(empty).toContain("данных нет");
  });

  it("GptTable: home со статусом cooling и бюджетом окон", () => {
    const nowMs = 1_700_000_000_000;
    const html = renderToString(
      <GptTable
        nowMs={nowMs}
        homes={[
          {
            id: "home-1",
            email: "owne…",
            process_live: true,
            reject_reason: "cooling",
            cooling_until: nowMs / 1000 + 600,
            inflight: 2,
            spend_nano_total: "33300000000",
            spend_usd_total: 33.3,
            windows: [
              {
                slot: "primary",
                used_percent: 42,
                used_fraction_units: 42_125_679,
                window_minutes: 300,
                source: "workload_blend",
                samples: 3,
                confidence: 0.8,
                remaining_nano: "10000000000",
                capacity_nano: "20000000000",
                low_nano: "18000000000",
                high_nano: "22000000000",
                remaining_low_nano: "9000000000",
                remaining_high_nano: "11000000000",
                observed_spend_nano: "8425135800",
                observed_fraction_units: 42_125_679,
                remaining_usd: 10,
                cap_usd: 20,
                low_usd: 18,
                high_usd: 22,
              },
            ],
            rate_limits: { primary: { resets_at: nowMs / 1000 + 300 } },
          },
        ]}
      />,
    );
    expect(html).toContain("home-1");
    expect(html).toContain("owne…");
    expect(html).toContain("аккаунт / home");
    expect(html).toContain("cooling 10м");
    expect(html).toContain("42.125679%");
    expect(html).toContain("доверительный интервал");
    expect(html).toContain("Δquota 42.125679%");
    expect(html).toContain("official-price");
  });

  it("CodexCapacityBoard: оставляет основную ёмкость и shared home capacity", () => {
    const html = renderToString(
      <CodexCapacityBoard
        nowMs={1_800_000_000_000}
        response={{
          calibration_evidence_available: true,
          credit_schedule_id: "chatgpt/codex-credits/2026-07-30/v1",
          conversion_models: [
            {
              id: "gpt-5.4",
              api: {
                input_nanousd_per_token: "2500",
                cached_input_nanousd_per_token: "250",
                cache_write_nanousd_per_token: "2500",
                output_nanousd_per_token: "15000",
                fast_multiplier_basis_points: 20000,
                long_context_threshold: "272000",
                long_input_multiplier_basis_points: 20000,
                long_output_multiplier_basis_points: 15000,
              },
              chatgpt_credits: {
                input_nanocredits_per_token: "62500",
                cached_input_nanocredits_per_token: "6250",
                output_nanocredits_per_token: "375000",
                fast_multiplier_basis_points: 20000,
              },
            },
            {
              id: "gpt-5.6-sol",
              api_tariff_schedule_id: "openai/gpt-5.6-sol/2026-07-30/v2",
              credit_schedule_id: "chatgpt/codex-credits/2026-07-30/v1",
              api: {
                input_nanousd_per_token: "5000",
                cached_input_nanousd_per_token: "500",
                cache_write_nanousd_per_token: "6250",
                output_nanousd_per_token: "30000",
                fast_multiplier_basis_points: 20000,
                long_context_threshold: "272000",
                long_input_multiplier_basis_points: 20000,
                long_output_multiplier_basis_points: 15000,
              },
              chatgpt_credits: {
                input_nanocredits_per_token: "125000",
                cached_input_nanocredits_per_token: "12500",
                output_nanocredits_per_token: "750000",
                fast_multiplier_basis_points: 25000,
              },
            },
          ],
          window_totals: [
            {
              window_minutes: 300,
              capacity_nanocredits: "2000000000000",
              remaining_nanocredits: "1200000000000",
            },
          ],
          plan_cohorts: [
            {
              plan: "chatgpt_pro",
              window_minutes: 300,
              homes_total: 1,
              measured_homes: 1,
              capacity_per_home_nanocredits: "2000000000000",
              fleet_capacity_nanocredits: "2000000000000",
              fleet_remaining_nanocredits: "1200000000000",
            },
          ],
          homes: [
            {
              id: "home-1",
              email: "owne…",
              plan: "chatgpt_pro",
              process_live: true,
              admitted: true,
              calibration_persistence_ok: true,
              calibration_pending_events: 0,
              calibration_dropped_events: 0,
              credit_tracking_started_ts: 1_790_000_000,
              windows: [
                {
                  slot: "primary",
                  window_minutes: 300,
                  resets_at: 1_800_000_600,
                  used_fraction_units: 40000000,
                  capacity_nanocredits: "2000000000000",
                  remaining_nanocredits: "1200000000000",
                  low_nanocredits: "1900000000000",
                  high_nanocredits: "2100000000000",
                  observed_spend_nanocredits: "800000000000",
                  observed_fraction_units: 40000000,
                  credit_samples: 4,
                  unattributed_fraction_units: 0,
                },
              ],
              calibration_evidence: [
                {
                  model: "gpt-5.6-sol",
                  service_tier: "fast",
                  provider_reported_tier: "priority",
                  turns: 3,
                  first_completed_at: 1_790_000_000,
                  last_completed_at: 1_790_000_100,
                  input_tokens: "1000",
                  cached_input_tokens: "400",
                  cache_write_input_tokens: "100",
                  output_tokens: "100",
                  reasoning_output_tokens: "80",
                  api_input_nanousd: "5000000",
                  api_cached_input_nanousd: "400000",
                  api_cache_write_nanousd: "1250000",
                  api_output_nanousd: "6000000",
                  api_total_nanousd: "12650000",
                  chatgpt_input_nanocredits: "187500000",
                  chatgpt_cached_input_nanocredits: "12500000",
                  chatgpt_output_nanocredits: "187500000",
                  chatgpt_total_nanocredits: "387500000",
                  api_tariff_schedule_id: "openai/gpt-5.6-sol/2026-07-30/v2",
                  credit_schedule_id: "chatgpt/codex-credits/2026-07-30/v1",
                },
              ],
            },
          ],
        }}
      />,
    );
    expect(html).not.toContain("Сколько токенов доступно");
    expect(html).not.toContain("Выгодность по убыванию");
    expect(html).toContain("Доступная ёмкость по home");
    expect(html).toContain("owne…");
    expect(html).not.toContain("home-1");
    expect(html).not.toContain("owner@example.com");
    expect(plain(html)).toContain("$48.00");
    expect(plain(html)).toContain("$120.00");
    expect(plain(html)).toContain("gpt-5.6-sol · short");
    expect(plain(html)).toContain("gpt-5.6-sol · standard/long/write");
    expect(plain(html)).not.toContain("gpt-5.4");
    expect(html).toContain("codex-quota-meter");
    expect(plain(html)).toContain("сброс 10м");
    expect(html).not.toContain("Immutable evidence ledger");
    expect(html).not.toContain("api_tariff_schedule_id");
  });

  it("CodexCapacityBoard: пустая калибровка остаётся коротким направляющим состоянием", () => {
    const html = renderToString(
      <CodexCapacityBoard
        nowMs={1_800_000_000_000}
        response={{ calibration_evidence_available: true, homes: [{ id: "home-1", email: "owne…" }] }}
      />,
    );
    expect(html).toContain("ждём Δquota");
    expect(html).not.toContain("Тарифный каталог недоступен");
    expect(html).toContain("owne…");
    expect(html).not.toContain("Первый точный расход появится здесь сразу");
  });

  it("CodexCapacityBoard: суммирует весь пул, но применяет capacity своего тарифа к каждой почте", () => {
    const html = renderToString(
      <CodexCapacityBoard
        nowMs={1_800_000_000_000}
        response={{
          plan_cohorts: [
            {
              plan: "chatgpt_pro",
              window_minutes: 10080,
              homes_total: 1,
              measured_homes: 1,
              capacity_per_home_nanocredits: "2000000000000",
              fleet_capacity_nanocredits: "2000000000000",
              fleet_remaining_nanocredits: "1200000000000",
            },
            {
              plan: "chatgpt_plus",
              window_minutes: 10080,
              homes_total: 1,
              measured_homes: 1,
              capacity_per_home_nanocredits: "1000000000000",
              fleet_capacity_nanocredits: "1000000000000",
              fleet_remaining_nanocredits: "750000000000",
            },
          ],
          homes: [
            {
              id: "pro",
              email: "pro…",
              plan: "chatgpt_pro",
              process_live: true,
              windows: [{ window_minutes: 10080, used_fraction_units: 40000000 }],
            },
            {
              id: "plus",
              email: "plus…",
              plan: "chatgpt_plus",
              process_live: true,
              windows: [{ window_minutes: 10080, used_fraction_units: 25000000 }],
            },
          ],
        }}
      />,
    );
    expect(html).toContain("весь пул");
    expect(plain(html)).toContain("1,950 credits");
    expect(plain(html)).toContain("3,000 credits");
    expect(plain(html)).toContain("1,200 credits");
    expect(plain(html)).toContain("750 credits");
  });

  it("ClaudeCapacityBoard: оставляет только компактные окна по аккаунтам", () => {
    const html = renderToString(
      <ClaudeCapacityBoard
        response={{
          calibrated: true,
          window_totals: [
            {
              window_minutes: 300,
              capacity_nano: "60000000000",
              remaining_nano: "45000000000",
              routable_subs: 1,
              calibrated_subs: 1,
            },
            {
              window_minutes: 10080,
              capacity_nano: "200000000000",
              remaining_nano: "120000000000",
              routable_subs: 1,
              calibrated_subs: 1,
            },
          ],
          conversion_models: [
            {
              id: "claude-opus-4-8",
              web_search_nanousd_per_request: "10000000",
              tiers: [
                {
                  id: "standard",
                  input_nanousd_per_token: "5000",
                  cache_read_nanousd_per_token: "500",
                  cache_write_5m_nanousd_per_token: "6250",
                  cache_write_1h_nanousd_per_token: "10000",
                  output_nanousd_per_token: "25000",
                },
                {
                  id: "fast",
                  input_nanousd_per_token: "10000",
                  cache_read_nanousd_per_token: "1000",
                  cache_write_5m_nanousd_per_token: "12500",
                  cache_write_1h_nanousd_per_token: "20000",
                  output_nanousd_per_token: "50000",
                },
              ],
            },
          ],
          calibration_evidence: [
            {
              email: "owne…",
              model: "claude-opus-4-8",
              service_tier: "standard",
              inference_geo: "global",
              turns: 3,
              input_tokens: "1000000",
              cache_read_tokens: "2000000",
              cache_write_5m_tokens: "300000",
              cache_write_1h_tokens: "100000",
              output_tokens: "500000",
              search_queries: "2",
              api_input_nanousd: "5000000000",
              api_cache_read_nanousd: "1000000000",
              api_cache_write_5m_nanousd: "1875000000",
              api_cache_write_1h_nanousd: "1000000000",
              api_output_nanousd: "12500000000",
              api_search_nanousd: "20000000",
              api_total_nanousd: "21395000000",
            },
          ],
          per_sub: [
            {
              email: "owne…",
              plan: "max20",
              routable: true,
              calibrated: true,
              auth_state: "healthy",
              util5h: 0.25,
              util7d: 0.4,
              reset5h_in: 600,
              reset7d_in: 3600,
              cap5h_nano: "60000000000",
              cap7d_nano: "200000000000",
              rem5h_nano: "45000000000",
              rem7d_nano: "120000000000",
            },
          ],
        }}
      />,
    );
    expect(html).toContain("Окна по аккаунтам");
    expect(html).toContain("owne…");
    expect(html).not.toContain("owner@example.com");
    expect(html).toContain("Доступно $ · 5ч");
    expect(html).toContain('provider-usd-ink provider-five-hour-money"><b>$45.00</b><small>из $60.00</small>');
    expect(plain(html)).toContain("$120.00");
    expect(plain(html)).toContain("40%");
    expect(plain(html)).toContain("сброс 1ч 0м");
    expect(html).toContain("provider-quota-meter");
    expect(html).not.toContain("Сколько токенов доступно");
    expect(html).not.toContain("Выгодность по убыванию");
    expect(html).not.toContain("Фактическая смесь калибровки");
    expect(html).not.toContain("claude-opus-4-8");
    expect(html).not.toContain("5ч · доступно");
  });

  it("ClaudeCapacityBoard: держит последнее точное значение до reset и отличает его от current", () => {
    const html = renderToString(
      <ClaudeCapacityBoard
        response={{
          now: 2_000,
          per_sub: [
            {
              email: "fres…",
              plan: "max20",
              routable: true,
              auth_state: "healthy",
              util5h: 0.99,
              util7d: 0.99,
              reset5h_in: null,
              reset7d_in: null,
              cap5h_nano: "60000000000",
              cap7d_nano: "200000000000",
              rem5h_nano: "45000000000",
              rem7d_nano: "120000000000",
              windows: [
                {
                  window_kind: "5h",
                  snapshot_fresh: true,
                  used_fraction_units: 25_000_000,
                  resets_at: null,
                  capacity_nano: "60000000000",
                  remaining_nano: "45000000000",
                  current_quota_source: "runtime_quota_snapshot",
                },
                {
                  window_kind: "7d",
                  snapshot_fresh: true,
                  used_fraction_units: 40_000_000,
                  resets_at: null,
                  capacity_nano: "200000000000",
                  remaining_nano: "120000000000",
                  current_quota_source: "runtime_quota_snapshot",
                },
              ],
            },
            {
              email: "stal…",
              plan: "max20",
              routable: true,
              auth_state: "healthy",
              util5h: 0.18,
              util7d: 0.93,
              reset5h_in: 1_800,
              reset7d_in: 86_400,
              cap5h_nano: "216730000000",
              cap7d_nano: "933330000000",
              windows: [
                {
                  window_kind: "5h",
                  snapshot_fresh: false,
                  used_fraction_units: 18_000_000,
                  resets_at: 3_800,
                  capacity_nano: "216730000000",
                  remaining_nano: null,
                  last_known_remaining_nano: "177718600000",
                  last_known_quota_source: "runtime_quota_snapshot",
                  missing_reason: "stale_current_quota_snapshot",
                },
                {
                  window_kind: "7d",
                  snapshot_fresh: false,
                  used_fraction_units: 93_000_000,
                  resets_at: 88_400,
                  capacity_nano: "933330000000",
                  remaining_nano: null,
                  last_known_remaining_nano: "65333100000",
                  last_known_quota_source: "runtime_quota_snapshot",
                  missing_reason: "stale_current_quota_snapshot",
                },
              ],
            },
            {
              email: "dead…",
              plan: "max20",
              routable: false,
              auth_state: "dead",
              dead_reason: "permission_error",
              util5h: 0,
              util7d: 0,
              cap5h_nano: "1222190000000",
              cap7d_nano: "4444440000000",
            },
          ],
        }}
      />,
    );
    const text = plain(html);
    expect(text).toContain("fres…");
    expect(text).toContain("25%");
    expect(text).toContain("$45.00");
    expect(text).toContain("сброс уточняется");
    expect(text).toContain("stal…");
    expect(text).toContain("18%");
    expect(text).toContain("93%");
    expect(text).toContain("сброс 30м");
    expect(text).toContain("сброс 1д 0ч");
    expect(text).toContain("$177.71");
    expect(text).toContain("$65.33");
    expect(text).toContain("последнее · из $216.73");
    expect(text).toContain("последнее · из $933.33");
    expect(text).not.toContain("обновляем");
    expect(text).toContain("dead…");
    expect(text).toContain("вне ротации");
    expect(text).toContain("не входит в ёмкость");
    expect(text).not.toContain("$1,222.19");
    expect(text).not.toContain("99%");
  });

  it("ClaudeCapacityBoard: после reset больше не переносит старое значение в новое окно", () => {
    const html = renderToString(
      <ClaudeCapacityBoard
        response={{
          now: 3_801,
          per_sub: [
            {
              email: "expi…",
              plan: "max20",
              routable: true,
              auth_state: "healthy",
              util5h: 0.18,
              util7d: 0.93,
              reset5h_in: null,
              reset7d_in: null,
              windows: [
                {
                  window_kind: "5h",
                  snapshot_fresh: false,
                  used_fraction_units: null,
                  resets_at: null,
                  capacity_nano: "216730000000",
                  remaining_nano: null,
                  last_known_remaining_nano: null,
                  missing_reason: "stale_current_quota_snapshot",
                },
                {
                  window_kind: "7d",
                  snapshot_fresh: false,
                  used_fraction_units: null,
                  resets_at: null,
                  capacity_nano: "933330000000",
                  remaining_nano: null,
                  last_known_remaining_nano: null,
                  missing_reason: "stale_current_quota_snapshot",
                },
              ],
            },
          ],
        }}
      />,
    );
    const text = plain(html);
    expect(text).toContain("expi…");
    expect(text).toContain("обновляем");
    expect(text).not.toContain("18%");
    expect(text).not.toContain("93%");
    expect(text).not.toContain("$177.71");
    expect(text).not.toContain("последнее");
  });

  it("ClaudeCapacityBoard: stale исчерпанное окно показывает 100% и reset без cooling", () => {
    const html = renderToString(
      <ClaudeCapacityBoard
        response={{
          now: 2_000,
          per_sub: [
            {
              email: "full…",
              plan: "max20",
              routable: false,
              cooling: true,
              auth_state: "healthy",
              util5h: 1,
              util7d: 0.42,
              reset5h_in: 1_800,
              reset7d_in: 86_400,
              cap5h_nano: "60000000000",
              cap7d_nano: "200000000000",
              rem5h_nano: "0",
              rem7d_nano: "116000000000",
              windows: [
                {
                  window_kind: "5h",
                  snapshot_fresh: false,
                  used_fraction_units: 100_000_000,
                  resets_at: 3_800,
                  capacity_nano: "60000000000",
                  remaining_nano: null,
                  last_known_remaining_nano: "0",
                  last_known_quota_source: "runtime_quota_snapshot",
                },
                {
                  window_kind: "7d",
                  snapshot_fresh: true,
                  used_fraction_units: 42_000_000,
                  resets_at: 88_400,
                  capacity_nano: "200000000000",
                  remaining_nano: "116000000000",
                },
              ],
            },
          ],
        }}
      />,
    );
    const text = plain(html);
    expect(text).toContain("лимит 5ч исчерпан");
    expect(text).not.toContain("cooling");
    expect(text).toContain("100%");
    expect(text).toContain("42%");
    expect(text).toContain("сброс 30м");
    expect(text).toContain("сброс 1д 0ч");
    expect(text).toContain("вне ротации");
    expect(text).toContain("не входит в ёмкость");
    expect(text).not.toContain("$60.00");
    expect(text).not.toContain("$116.00");
  });

  it("GeminiCapacityBoard: оставляет основные окна, модели и masked email", () => {
    const nowMs = 1_800_000_000_000;
    const html = renderToString(
      <GeminiCapacityBoard
        nowMs={nowMs}
        response={{
          now: nowMs / 1000,
          available: 1,
          calibration_authority_available: true,
          calibration_delivery: { pending_events: 0, dropped_events: 0, persistence_ok: true },
          window_totals: [
            {
              window_minutes: 300,
              capacity_nano: "50000000000",
              remaining_nano: "30000000000",
              measured_profiles: 1,
              observed_profiles: 1,
            },
            {
              window_minutes: 10080,
              capacity_nano: "200000000000",
              remaining_nano: "150000000000",
              measured_profiles: 1,
              observed_profiles: 1,
            },
          ],
          models: [
            { id: "gemini-3.1-flash-image", quota_model_ids: ["gemini-3.1-flash-image"] },
            { id: "gemini-3.6-flash", quota_model_ids: ["gemini-3.6-flash-medium"] },
          ],
          conversion_models: [
            {
              id: "gemini-3.1-flash-image",
              quota_model_ids: ["gemini-3.1-flash-image"],
              rates: {
                input_nanousd_per_token: "500",
                audio_input_nanousd_per_token: "500",
                cached_input_nanousd_per_token: "500",
                cached_audio_input_nanousd_per_token: "500",
                output_nanousd_per_token: "3000",
                image_output_nanousd_per_token: "60000",
                long_input_nanousd_per_token: "500",
                long_audio_input_nanousd_per_token: "500",
                long_cached_input_nanousd_per_token: "500",
                long_cached_audio_input_nanousd_per_token: "500",
                long_output_nanousd_per_token: "3000",
              },
              search: { billing_unit: "query", nanousd_per_unit: "14000000" },
            },
            {
              id: "gemini-3.6-flash",
              quota_model_ids: ["gemini-3.6-flash-medium"],
              rates: {
                input_nanousd_per_token: "1500",
                audio_input_nanousd_per_token: "1500",
                cached_input_nanousd_per_token: "150",
                cached_audio_input_nanousd_per_token: "150",
                output_nanousd_per_token: "7500",
                image_output_nanousd_per_token: "0",
                long_input_nanousd_per_token: "1500",
                long_audio_input_nanousd_per_token: "1500",
                long_cached_input_nanousd_per_token: "150",
                long_cached_audio_input_nanousd_per_token: "150",
                long_output_nanousd_per_token: "7500",
              },
              search: { billing_unit: "query", nanousd_per_unit: "14000000" },
            },
          ],
          profiles: [
            {
              id: "opaque-profile-id",
              email: "gemi…",
              plan: "google_ai_pro",
              authenticated: true,
              model_cooling: [{ model_id: "gemini-3.1-flash-image", cooling_until: 0 }],
              quotas: [
                {
                  model_id: "gemini-3.1-flash-image",
                  remaining_amount: "1250000",
                  remaining_fraction: 0.5,
                  token_type: "tokens",
                  reset_time: new Date(nowMs + 3600_000).toISOString(),
                },
                {
                  model_id: "gemini-3.6-flash-medium",
                  remaining_amount: null,
                  remaining_fraction: 0.75,
                  token_type: "antigravity_model",
                  reset_time: new Date(nowMs + 7200_000).toISOString(),
                },
              ],
              windows: [
                {
                  window_kind: "5h",
                  used_fraction_units: 40_000_000,
                  capacity_nano: "50000000000",
                  remaining_nano: "30000000000",
                  resets_at: nowMs / 1000 + 600,
                },
                {
                  window_kind: "weekly",
                  used_fraction_units: 25_000_000,
                  capacity_nano: "200000000000",
                  remaining_nano: "150000000000",
                  resets_at: nowMs / 1000 + 3600,
                },
              ],
            },
          ],
        }}
      />,
    );
    expect(html).not.toContain("Доступная квота по моделям");
    expect(html).not.toContain("Выгодность по убыванию");
    expect(html).toContain("gemi…");
    expect(html).not.toContain("opaque-profile-id");
    expect(html).not.toContain("Google даёт только %");
    expect(html).not.toContain("Тарифный каталог Gemini недоступен");
    expect(html).toContain("Доступно $ · 5ч");
    expect(html).toContain('provider-usd-ink provider-five-hour-money"><b>$30.00</b><small>из $50.00</small>');
    expect(plain(html).indexOf("5ч · доступно")).toBeLessThan(plain(html).indexOf("7д · доступно"));
    expect(plain(html)).toContain("25%");
    expect(html).toContain("provider-quota-meter");
  });

  it("GeminiCapacityBoard: pending authority скрывает stale API-$, а не показывает их как доступные", () => {
    const html = renderToString(
      <GeminiCapacityBoard
        nowMs={1_800_000_000_000}
        response={{
          calibration_authority_available: true,
          calibration_delivery: { pending_events: 1, dropped_events: 0, persistence_ok: false },
          window_totals: [{ window_minutes: 300, capacity_nano: "50000000000", remaining_nano: "30000000000" }],
          profiles: [{
            email: "gemi…",
            authenticated: true,
            windows: [{
              window_kind: "5h",
              used_fraction_units: 40_000_000,
              resets_at: 1_800_000_600,
              capacity_nano: "50000000000",
              remaining_nano: "30000000000",
            }],
          }],
        }}
      />,
    );
    expect(plain(html)).not.toContain("$30.00");
    expect(plain(html)).not.toContain("$50.00");
    expect(plain(html)).toContain("40%");
    expect(plain(html)).toContain("обновляем");
    expect(plain(html)).toContain("quota уже доступна");
    expect(html).toContain("gemi…");
  });

  it("GeminiCapacityBoard: локальная ошибка persistence скрывает stale API-$ при здоровой FIFO", () => {
    const html = renderToString(
      <GeminiCapacityBoard
        nowMs={1_800_000_000_000}
        response={{
          calibration_authority_available: true,
          calibration_delivery: { pending_events: 0, dropped_events: 0, persistence_ok: true },
          window_totals: [{ window_minutes: 300, capacity_nano: "50000000000", remaining_nano: "30000000000" }],
          profiles: [{
            email: "gemi…",
            authenticated: true,
            calibration_persistence_ok: false,
            windows: [{ window_kind: "5h", capacity_nano: "50000000000", remaining_nano: "30000000000" }],
          }],
        }}
      />,
    );
    expect(plain(html)).not.toContain("$30.00");
    expect(plain(html)).not.toContain("$50.00");
    expect(plain(html)).toContain("calibration storage");
    expect(html).toContain("gemi…");
  });

  it("GeminiCapacityBoard: профиль вне ротации не показывает saleable API-$", () => {
    const html = renderToString(
      <GeminiCapacityBoard
        nowMs={1_800_000_000_000}
        response={{
          calibration_authority_available: true,
          calibration_delivery: { pending_events: 0, dropped_events: 0, persistence_ok: true },
          profiles: [{
            email: "dead…",
            authenticated: false,
            windows: [{
              window_kind: "5h",
              used_fraction_units: 70_000_000,
              capacity_nano: "50000000000",
              remaining_nano: "15000000000",
            }],
          }],
        }}
      />,
    );
    expect(plain(html)).toContain("70%");
    expect(plain(html)).toContain("вне ротации");
    expect(plain(html)).toContain("не входит в ёмкость");
    expect(plain(html)).not.toContain("$15.00");
    expect(plain(html)).not.toContain("$50.00");
  });

  it("GeminiCapacityBoard: отключённый оператором профиль виден, но не считается ёмкостью", () => {
    const html = renderToString(
      <GeminiCapacityBoard
        nowMs={1_800_000_000_000}
        response={{
          calibration_authority_available: true,
          calibration_delivery: { pending_events: 0, dropped_events: 0, persistence_ok: true },
          profiles: [{
            id: "gemini_oauth_000002",
            email: "pull…",
            // Отключённый профиль не аутентифицируется — его перестают пробить. Диагноз
            // «ошибка auth» здесь ввёл бы в заблуждение, поэтому ручное состояние важнее.
            authenticated: false,
            disabled: true,
            windows: [{
              window_kind: "5h",
              used_fraction_units: 40_000_000,
              capacity_nano: "50000000000",
              remaining_nano: "15000000000",
            }],
          }],
        }}
      />,
    );
    // Остаётся в списке — иначе его нельзя было бы вернуть, — и предлагает вернуть.
    expect(plain(html)).toContain("отключён оператором");
    expect(plain(html)).toContain("Вернуть");
    expect(plain(html)).not.toContain("ошибка auth");
    // Но деньги отключённого профиля продавать нельзя.
    expect(plain(html)).not.toContain("$15.00");
    expect(plain(html)).not.toContain("$50.00");
    // И он не входит в счётчик auth.
    expect(plain(html)).toContain("0/1 auth");
  });

  it("GeminiTable: одна строка на профиль, а не на каждую модель", () => {
    const nowMs = 1_700_000_000_000;
    const models = Array.from({ length: 7 }, (_, index) => ({
      id: `gemini-model-${index + 1}`,
      available: 2,
      healthy: 1,
      unknown: 1,
    }));
    const modelHealth = models.map((model) => ({
      model_id: model.id,
      cooling_until: 0,
      last_success_at: nowMs / 1000 - 60,
    }));
    const html = renderToString(
      <GeminiTable
        nowMs={nowMs}
        now={nowMs / 1000}
        models={models}
        profiles={[
          {
            id: "prof-1",
            authenticated: true,
            inflight: 2,
            spend_usd_total: 7.5,
            model_cooling: modelHealth,
            windows: [
              {
                window_kind: "5h",
                source: "workload_blend",
                remaining_fraction: 0.6,
                remaining_usd: 30,
                cap_usd: 50,
                low_usd: 45,
                high_usd: 55,
                observed_fraction_units: 400000,
                observed_spend_usd: 20,
                samples: 2,
                confidence: 0.5,
                resets_at: nowMs / 1000 + 1800,
              },
            ],
            last_probe_at: nowMs / 1000 - 120,
          },
          {
            id: "prof-2",
            authenticated: true,
            spend_usd_total: 2.5,
            model_cooling: modelHealth,
          },
        ]}
      />,
    );
    expect(html).toContain("prof-1");
    expect(html).toContain("prof-2");
    expect(html).toContain("active");
    expect(html).toContain("workload envelope");
    expect(html).toContain("0.40000%"); // Δquota
    expect(plain(html)).toContain("40%"); // использовано: 1 − remaining_fraction 0.6
    expect(html).toContain("<b>7/7</b> доступны");
    expect(html).not.toContain("gemini-model-1");
    // header + две подписки: семь моделей не создают ещё 14 строк.
    expect(html.match(/<tr/g)).toHaveLength(3);
  });

  it("GeminiModelDetails: каталог моделей хранит health, quota и reset отдельно от профилей", () => {
    const nowMs = 1_700_000_000_000;
    const html = renderToString(
      <GeminiModelDetails
        nowMs={nowMs}
        now={nowMs / 1000}
        models={[{
          id: "gemini-3-flash-preview",
          quota_model_ids: ["gemini-3-flash", "gemini-3-flash-agent"],
          available: 1,
          healthy: 1,
          degraded: 0,
          unknown: 0,
        }]}
        profiles={[
          {
            id: "prof-1",
            quotas: [
              {
                model_id: "gemini-3-flash-agent",
                token_type: "requests",
                remaining_fraction: 0.25,
                remaining_amount: "250",
                reset_time: new Date(nowMs + 3600_000).toISOString(),
              },
            ],
          },
        ]}
      />,
    );
    expect(plain(html)).toContain("Каталог Gemini · 1 модель");
    expect(html).toContain("gemini-3-flash-preview");
    expect(plain(html)).toContain("1 healthy");
    expect(html).toContain("официальная quota");
    expect(plain(html)).toContain("75%"); // использовано при 25% остатка
    expect(plain(html)).toContain("осталось 25%");
    expect(html).toContain("1ч 0м");
  });

  it("TransportDetails: fingerprint и affinity-счётчики", () => {
    const html = renderToString(
      <TransportDetails
        transport={{
          antigravity_version: "1.2.3",
          expected_ja3: "ja3-hash",
          expected_ja4: "ja4-hash",
        }}
        affinity={{ local_hits: 5, redis_hits: 2, rebinds: 1 }}
      />,
    );
    expect(html).toContain("Gemini transport fingerprint и cache/affinity");
    expect(html).toContain("1.2.3");
    expect(html).toContain("ja3-hash");
    expect(plain(html)).toContain("local 5");
    expect(plain(html)).toContain("rebinds 1");
  });
});

// Детерминированные фикстуры KIMI — точная форма wire contract GET /kimi-subs.
const KIMI_NOW = 1_785_820_912;

const KIMI_PROFILE: KimiProfile = {
  id: "kimi-1beecf16c84925f0",
  plan: "unreviewed",
  live: true,
  inflight: 0,
  cooling: { auth_until: null, transport_until: null, quota_until: null },
  quota_observed_at: KIMI_NOW - 29,
  quota: [
    {
      duration_secs: 18_000,
      used_units: "25",
      limit_units: "100",
      used_fraction_units: 25_000_000,
      measurement_resolution_fraction_units: 1_000_000,
      resets_at: KIMI_NOW + 5_593,
      observed_at: KIMI_NOW - 29,
    },
    {
      duration_secs: 604_800,
      used_units: "120",
      limit_units: "1000",
      used_fraction_units: 12_000_000,
      measurement_resolution_fraction_units: 1_000_000,
      resets_at: KIMI_NOW + 600_000,
      observed_at: KIMI_NOW - 29,
    },
  ],
  calibration: [
    {
      duration_secs: 18_000,
      samples: 4,
      confidence_bp: 8_000,
      capacity: { current_nano: "60000000000", low_nano: "55000000000", high_nano: "65000000000" },
      remaining: { native_units: 75, api_nano: "45000000000" },
      observed_spend_nano: "15000000000",
      unattributed_fraction_units: 0,
      last_measured_at: KIMI_NOW - 60,
      estimator_version: 1,
    },
    {
      duration_secs: 604_800,
      samples: 9,
      confidence_bp: 9_000,
      capacity: { current_nano: "200000000000", low_nano: "190000000000", high_nano: "210000000000" },
      remaining: { native_units: 880, api_nano: "176000000000" },
      observed_spend_nano: "24000000000",
      unattributed_fraction_units: 0,
      last_measured_at: KIMI_NOW - 60,
      estimator_version: 1,
    },
  ],
};

const kimiResponse = (overrides: Partial<KimiSubsResponse> = {}): KimiSubsResponse => ({
  now: KIMI_NOW,
  enabled: true,
  delivery: { pending_events: 0, dropped_events: 0, persistence_ok: true },
  fleet: {
    profiles: 1,
    live_profiles: 1,
    available_profiles: 1,
    inflight_requests: 0,
    auth_quarantined_profiles: 0,
    transport_cooling_profiles: 0,
    quota_cooling_profiles: 0,
  },
  profiles: [KIMI_PROFILE],
  ...overrides,
});

describe("KIMI capacity board (wire contract /kimi-subs)", () => {
  it("KimiCapacityBoard: реальные окна из duration_secs, exact деньги и проценты, одна строка на профиль", () => {
    const html = renderToString(<KimiCapacityBoard nowMs={KIMI_NOW * 1000} response={kimiResponse()} />);
    const text = plain(html);
    expect(text).toContain("Окна по аккаунтам");
    expect(text).toContain("kimi-1beecf16c84925f0");
    expect(text).toContain("unreviewed");
    expect(text).toContain(">active</span>");
    // Окна подписаны реальной длительностью: 18000 → 5ч, 604800 → 7д.
    expect(text).toContain("Quota 5ч / reset");
    expect(text).toContain("Доступно $ · 5ч");
    expect(text).toContain("Quota 7д / reset");
    expect(text).toContain("Доступно $ · 7д");
    // Exact remaining/full API-$ из decimal nano strings.
    expect(text).toContain('provider-usd-ink provider-five-hour-money"><b>$45.00</b><small>из $60.00</small>');
    expect(text).toContain("$176.00");
    expect(text).toContain("из $200.00");
    // Used share из used_fraction_units (1e-8): 25_000_000 → 25%, 12_000_000 → 12%.
    expect(text).toContain("25%");
    expect(text).toContain("12%");
    expect(text).toContain("сброс 1ч 33м");
    expect(text).toContain("сброс 6д 22ч");
    // Summary strip: те же exact значения и measured coverage.
    expect(text).toContain("5ч · доступно");
    expect(text).toContain("7д · доступно");
    expect(text).toContain("Профили в ротации");
    expect(text).toContain("1/1");
    expect(text).toContain("1/1 измерено");
    expect(html.match(/provider-quota-meter/g)).toHaveLength(2);
    // Одна identity = одна строка независимо от количества окон: header + 1 профиль.
    expect(html.match(/<tr/g)).toHaveLength(2);
    // Privacy: никакого subject/email-shaped контента — только opaque roster id.
    expect(html).not.toMatch(/@/);
    expect(html).not.toContain("subject");
    expect(html).not.toContain("email");
    expect(html).not.toContain("Почта");
    // Удалённой аналитики нет.
    expect(html).not.toContain("Выгодность по убыванию");
    expect(html).not.toContain("Сколько токенов доступно");
    expect(html).not.toContain("estimator_version");
  });

  it("KimiCapacityBoard: null-деньги → «ждём данные», никогда не $0", () => {
    const html = renderToString(
      <KimiCapacityBoard
        nowMs={KIMI_NOW * 1000}
        response={kimiResponse({
          profiles: [{
            ...KIMI_PROFILE,
            calibration: [{
              duration_secs: 18_000,
              samples: 0,
              confidence_bp: 0,
              capacity: { current_nano: null, low_nano: null, high_nano: null },
              remaining: { native_units: 100, api_nano: null },
              observed_spend_nano: "0",
              unattributed_fraction_units: 0,
              last_measured_at: null,
              estimator_version: 1,
            }],
          }],
        })}
      />,
    );
    const text = plain(html);
    expect(text).toContain("ждём данные");
    expect(text).toContain("ещё не измерено");
    expect(text).toContain("0/1 измерено");
    // Свежая provider quota остаётся видна даже без calibrated денег.
    expect(text).toContain("25%");
    expect(text).not.toContain("$0.00");
    expect(text).not.toContain("$45.00");
  });

  it("KimiCapacityBoard: pending delivery скрывает saleable деньги за «сохраняется»", () => {
    const html = renderToString(
      <KimiCapacityBoard
        nowMs={KIMI_NOW * 1000}
        response={kimiResponse({ delivery: { pending_events: 2, dropped_events: 0, persistence_ok: true } })}
      />,
    );
    const text = plain(html);
    expect(text).toContain("сохраняется");
    expect(text).not.toContain("$45.00");
    expect(text).not.toContain("$176.00");
    // Quota — live provider fact и не скрывается.
    expect(text).toContain("25%");
    expect(text).toContain("quota уже доступна");
  });

  it("KimiCapacityBoard: dropped/persistence-сбой скрывает stale API-$ за «обновляем»", () => {
    const html = renderToString(
      <KimiCapacityBoard
        nowMs={KIMI_NOW * 1000}
        response={kimiResponse({ delivery: { pending_events: 0, dropped_events: 1, persistence_ok: false } })}
      />,
    );
    const text = plain(html);
    expect(text).toContain("обновляем");
    expect(text).not.toContain("$45.00");
    expect(text).not.toContain("$176.00");
    expect(text).toContain("25%");
  });

  it("KimiCapacityBoard: dead и cooling профили — «вне ротации» без saleable денег", () => {
    const dead = renderToString(
      <KimiCapacityBoard
        nowMs={KIMI_NOW * 1000}
        response={kimiResponse({
          fleet: { ...kimiResponse().fleet, live_profiles: 0, available_profiles: 0 },
          profiles: [{ ...KIMI_PROFILE, live: false }],
        })}
      />,
    );
    const deadText = plain(dead);
    expect(deadText).toContain("вне ротации");
    expect(deadText).toContain("не входит в ёмкость");
    expect(deadText).not.toContain("$45.00");
    // Quota остаётся диагностикой, как у Gemini.
    expect(deadText).toContain("25%");

    const cooling = renderToString(
      <KimiCapacityBoard
        nowMs={KIMI_NOW * 1000}
        response={kimiResponse({
          fleet: { ...kimiResponse().fleet, available_profiles: 0, quota_cooling_profiles: 1 },
          profiles: [{
            ...KIMI_PROFILE,
            cooling: { auth_until: null, transport_until: null, quota_until: KIMI_NOW + 300 },
          }],
        })}
      />,
    );
    const coolingText = plain(cooling);
    expect(coolingText).toContain("cooling quota 5м");
    expect(coolingText).toContain("вне ротации");
    expect(coolingText).not.toContain("$45.00");
  });

  it("KimiCapacityBoard: протухший snapshot → «обновляем» и без денег", () => {
    const stale = KIMI_NOW - 1_200;
    const html = renderToString(
      <KimiCapacityBoard
        nowMs={KIMI_NOW * 1000}
        response={kimiResponse({
          profiles: [{
            ...KIMI_PROFILE,
            quota_observed_at: stale,
            quota: KIMI_PROFILE.quota?.map((window) => ({ ...window, observed_at: stale })),
            calibration: KIMI_PROFILE.calibration?.map((row) => ({ ...row, last_measured_at: stale })),
          }],
        })}
      />,
    );
    const text = plain(html);
    expect(text).toContain(">обновляем</span>");
    expect(text).toContain("ждём свежую квоту");
    expect(text).not.toContain("$45.00");
    expect(text).toContain("25%");
  });

  it("KimiCapacityBoard: несколько окон не плодят строки; нестандартное окно подписано честно", () => {
    const html = renderToString(
      <KimiCapacityBoard
        nowMs={KIMI_NOW * 1000}
        response={kimiResponse({
          profiles: [{
            ...KIMI_PROFILE,
            quota: [
              ...(KIMI_PROFILE.quota ?? []),
              {
                duration_secs: 3_600,
                used_units: "5",
                limit_units: "20",
                used_fraction_units: 25_000_000,
                measurement_resolution_fraction_units: 1_000_000,
                resets_at: KIMI_NOW + 900,
                observed_at: KIMI_NOW - 29,
              },
            ],
          }],
        })}
      />,
    );
    const text = plain(html);
    // 3600 секунд — это «1ч», а не фиктивный 5ч-эквивалент.
    expect(text).toContain("Quota 1ч / reset");
    expect(text).toContain("Доступно $ · 1ч");
    expect(text).toContain("сброс 15м");
    // header + ровно одна строка профиля при трёх окнах.
    expect(html.match(/<tr/g)).toHaveLength(2);
    expect(html.match(/provider-quota-meter/g)).toHaveLength(3);
    // У окна без calibration — «ждём данные», у измеренных — exact деньги.
    expect(text).toContain("ждём данные");
    expect(text).toContain("$45.00");
  });

  it("KimiCapacityBoard: BigInt-суммы пула и limit-взвешенная used-доля в strip", () => {
    const second: KimiProfile = {
      ...KIMI_PROFILE,
      id: "kimi-aa20ff0011223344",
      plan: "kimi_for_coding",
      quota: [{
        duration_secs: 18_000,
        used_units: "150",
        limit_units: "300",
        used_fraction_units: 50_000_000,
        measurement_resolution_fraction_units: 1_000_000,
        resets_at: KIMI_NOW + 3_000,
        observed_at: KIMI_NOW - 10,
      }],
      calibration: [{
        duration_secs: 18_000,
        samples: 2,
        confidence_bp: 7_000,
        capacity: { current_nano: "40000000000", low_nano: null, high_nano: null },
        remaining: { native_units: 150, api_nano: "15000000000" },
        observed_spend_nano: "25000000000",
        unattributed_fraction_units: 0,
        last_measured_at: KIMI_NOW - 30,
        estimator_version: 1,
      }],
    };
    const first: KimiProfile = { ...KIMI_PROFILE, quota: [KIMI_PROFILE.quota![0]], calibration: [KIMI_PROFILE.calibration![0]] };
    const html = renderToString(
      <KimiCapacityBoard
        nowMs={KIMI_NOW * 1000}
        response={kimiResponse({
          fleet: { ...kimiResponse().fleet, profiles: 2, live_profiles: 2, available_profiles: 2 },
          profiles: [first, second],
        })}
      />,
    );
    const text = plain(html);
    // 45e9 + 15e9 = $60.00 из 60e9 + 40e9 = $100.00 — никакой float-математики.
    expect(text).toContain("$60.00");
    expect(text).toContain("из $100.00");
    // (25e6·100 + 50e6·300) / 400 = 43_750_000 → 43.8%.
    expect(text).toContain("43.8%");
    expect(text).toContain("2/2");
    expect(text).toContain("2/2 измерено");
    // header + две identity.
    expect(html.match(/<tr/g)).toHaveLength(3);
  });

  it("FleetCapacityOverview: KIMI-карточка с реальными окнами рядом с остальными флотами", () => {
    const html = renderToString(
      <FleetCapacityOverview
        claude={{
          calibrated: true,
          per_sub: [{ routable: true }],
          window_totals: [
            { window_minutes: 300, capacity_nano: "60000000000", remaining_nano: "45000000000", routable_subs: 1, calibrated_subs: 1 },
            { window_minutes: 10_080, capacity_nano: "200000000000", remaining_nano: "120000000000" },
          ],
        }}
        gpt={{
          enabled: true,
          available: 1,
          homes: [{ process_live: true, admitted: true }],
          window_totals: [
            { window_minutes: 300, capacity_nano: "80000000000", remaining_nano: "40000000000", measured_homes: 1, observed_homes: 1 },
            { window_minutes: 10_080, capacity_nano: "300000000000", remaining_nano: "210000000000" },
          ],
        }}
        gemini={{
          enabled: true,
          available: 1,
          calibration_authority_available: true,
          calibration_delivery: { pending_events: 0, dropped_events: 0, persistence_ok: true },
          profiles: [{ authenticated: true }],
          window_totals: [
            { window_minutes: 300, capacity_nano: "50000000000", remaining_nano: "30000000000", measured_profiles: 1, observed_profiles: 1 },
            { window_minutes: 10_080, capacity_nano: "250000000000", remaining_nano: "150000000000" },
          ],
        }}
        kimi={kimiResponse()}
      />,
    );
    const text = plain(html);
    expect(text).toContain("KIMI");
    expect(html).toContain("fleet-kimi");
    // 4 флота × 2 rail: KIMI показывает свои реальные 5ч/7д из duration_secs.
    expect(html.match(/fleet-window-rail/g)).toHaveLength(8);
    expect(text).toContain("/ $60.00");
    expect(text).toContain("/ $200.00");
    expect(text).toContain("1/1 измерено");
    expect(html).toContain("fleet-state ok");
  });

  it("FleetCapacityOverview: KIMI без калибровки — «ждём данные» вместо $0, quota-доля видна", () => {
    const html = renderToString(
      <FleetCapacityOverview
        claude={null}
        gpt={null}
        gemini={null}
        kimi={kimiResponse({
          profiles: [{
            ...KIMI_PROFILE,
            calibration: [{
              duration_secs: 18_000,
              samples: 0,
              confidence_bp: 0,
              capacity: { current_nano: null, low_nano: null, high_nano: null },
              remaining: { native_units: 100, api_nano: null },
              observed_spend_nano: "0",
              unattributed_fraction_units: 0,
              last_measured_at: null,
              estimator_version: 1,
            }],
          }],
        })}
      />,
    );
    const text = plain(html);
    expect(text).toContain("ждём данные");
    expect(text).not.toContain("$0.00");
    expect(text).not.toContain("$45.00");
    expect(text).toContain("25%");
    expect(text).toContain("0/1 измерено");
    expect(html).toContain("fleet-state warn");
  });

  it("FleetCapacityOverview: очередь и потеря KIMI delivery скрывают деньги флота", () => {
    const pending = renderToString(
      <FleetCapacityOverview
        claude={null}
        gpt={null}
        gemini={null}
        kimi={kimiResponse({ delivery: { pending_events: 2, dropped_events: 0, persistence_ok: true } })}
      />,
    );
    expect(plain(pending)).toContain("2 сохраняется");
    expect(plain(pending)).not.toContain("$45.00");

    const dropped = renderToString(
      <FleetCapacityOverview
        claude={null}
        gpt={null}
        gemini={null}
        kimi={kimiResponse({ delivery: { pending_events: 0, dropped_events: 1, persistence_ok: false } })}
      />,
    );
    expect(plain(dropped)).toContain("1 потеряно");
    expect(plain(dropped)).not.toContain("$45.00");
    expect(dropped).toContain("fleet-state bad");
  });

  it("FleetCapacityOverview: disabled envelope и недоступный KIMI — честные состояния", () => {
    const off = renderToString(
      <FleetCapacityOverview
        claude={null}
        gpt={null}
        gemini={null}
        kimi={{ now: KIMI_NOW, enabled: false, profiles: [] }}
      />,
    );
    expect(plain(off)).toContain("выключен");
    expect(off).toContain("fleet-kimi");
    expect(off).toContain("fleet-state bad");

    const down = renderToString(<FleetCapacityOverview claude={null} gpt={null} gemini={null} kimi={null} />);
    expect(plain(down)).toContain("нет связи");
    expect(down).toContain("fleet-kimi");
  });
});

// Детерминированные фикстуры GLM — точная форма wire contract GET /glm-subs.
const GLM_NOW = 1_785_820_912;

const GLM_PROFILE: GlmProfile = {
  id: "glm-7f3a91c04b2d8e65",
  plan: "Pro",
  live: true,
  account_dead: false,
  account_suspect: false,
  cooling: { transport_until: null, quota_until: null },
  inflight: 0,
  quota_observed_at: GLM_NOW - 29,
  quota: [
    {
      duration_secs: 18_000,
      used_units: 3_000,
      limit_units: 12_000,
      remaining_units: 9_000,
      used_fraction_units: 25_000_000,
      measurement_resolution_fraction_units: 100_000,
      resets_at: GLM_NOW + 5_593,
      observed_at: GLM_NOW - 29,
    },
    {
      duration_secs: 604_800,
      used_units: 7_200,
      limit_units: 60_000,
      remaining_units: 52_800,
      used_fraction_units: 12_000_000,
      measurement_resolution_fraction_units: 100_000,
      resets_at: GLM_NOW + 600_000,
      observed_at: GLM_NOW - 29,
    },
  ],
  calibration: [
    {
      duration_secs: 18_000,
      samples: 4,
      confidence_bp: 8_000,
      capacity: { current_nano: "60000000000", low_nano: "55000000000", high_nano: "65000000000" },
      remaining: { native_units: 9_000_000_000, api_nano: "45000000000" },
      observed_spend_nano: "15000000000",
      observed_spend_native_units: 3_000_000_000,
      unattributed_fraction_units: 0,
      last_measured_at: GLM_NOW - 60,
      estimator_version: 1,
    },
    {
      duration_secs: 604_800,
      samples: 9,
      confidence_bp: 9_000,
      capacity: { current_nano: "200000000000", low_nano: "190000000000", high_nano: "210000000000" },
      remaining: { native_units: 52_800_000_000, api_nano: "176000000000" },
      observed_spend_nano: "24000000000",
      observed_spend_native_units: 7_200_000_000,
      unattributed_fraction_units: 0,
      last_measured_at: GLM_NOW - 60,
      estimator_version: 1,
    },
  ],
};

const glmResponse = (overrides: Partial<GlmSubsResponse> = {}): GlmSubsResponse => ({
  now: GLM_NOW,
  enabled: true,
  delivery: { pending_events: 0, dropped_events: 0, persistence_ok: true },
  fleet: {
    profiles: 1,
    live_profiles: 1,
    available_profiles: 1,
    inflight_requests: 0,
    account_dead_profiles: 0,
    account_suspect_profiles: 0,
    transport_cooling_profiles: 0,
    quota_cooling_profiles: 0,
  },
  window_totals: [
    { window_minutes: 300, duration_secs: 18_000, capacity_nano: "60000000000", remaining_nano: "45000000000" },
    { window_minutes: 10_080, duration_secs: 604_800, capacity_nano: "200000000000", remaining_nano: "176000000000" },
  ],
  profiles: [GLM_PROFILE],
  ...overrides,
});

describe("GLM capacity board (wire contract /glm-subs)", () => {
  it("GlmCapacityBoard: реальные окна из duration_secs, exact деньги и проценты, одна строка на профиль", () => {
    const html = renderToString(<GlmCapacityBoard nowMs={GLM_NOW * 1000} response={glmResponse()} />);
    const text = plain(html);
    expect(text).toContain("Окна по аккаунтам");
    expect(text).toContain("glm-7f3a91c04b2d8e65");
    expect(text).toContain("Pro");
    expect(text).toContain(">active</span>");
    // Окна подписаны реальной длительностью: 18000 → 5ч, 604800 → 7д.
    expect(text).toContain("Quota 5ч / reset");
    expect(text).toContain("Доступно $ · 5ч");
    expect(text).toContain("Quota 7д / reset");
    expect(text).toContain("Доступно $ · 7д");
    // Exact remaining/full API-$ из decimal nano strings.
    expect(text).toContain('provider-usd-ink provider-five-hour-money"><b>$45.00</b><small>из $60.00</small>');
    expect(text).toContain("$176.00");
    expect(text).toContain("из $200.00");
    // Used share из used_fraction_units (1e-8): 25_000_000 → 25%, 12_000_000 → 12%.
    expect(text).toContain("25%");
    expect(text).toContain("12%");
    expect(text).toContain("сброс 1ч 33м");
    expect(text).toContain("сброс 6д 22ч");
    // Native остаток (microcredits) — отдельная компактная колонка, exact integer.
    expect(text).toContain("Native · остаток");
    expect(text).toContain("9,000,000,000");
    expect(text).toContain("52,800,000,000");
    expect(text).toContain("микрокредиты");
    // Summary strip: те же exact значения и measured coverage.
    expect(text).toContain("5ч · доступно");
    expect(text).toContain("7д · доступно");
    expect(text).toContain("Профили в ротации");
    expect(text).toContain("1/1");
    expect(text).toContain("1/1 измерено");
    expect(html.match(/provider-quota-meter/g)).toHaveLength(2);
    // Одна identity = одна строка независимо от количества окон: header + 1 профиль.
    expect(html.match(/<tr/g)).toHaveLength(2);
    // Wide-table контракт: sticky identity колонка + горизонтальный скролл карточки.
    expect(html).toContain("provider-home-capacity-table provider-glm-home-table");
    expect(html).toContain("tscroll");
    // Privacy: никакого subject/key/proxy-shaped контента — только opaque roster id.
    expect(html).not.toMatch(/@/);
    expect(html).not.toContain("subject");
    expect(html).not.toContain("email");
    expect(html).not.toContain("Почта");
    expect(html).not.toContain("api_key");
    expect(html).not.toContain("proxy");
    expect(html).not.toContain("credential");
    expect(html).not.toContain("base_url");
    // Удалённой аналитики нет.
    expect(html).not.toContain("Выгодность по убыванию");
    expect(html).not.toContain("Сколько токенов доступно");
    expect(html).not.toContain("estimator_version");
  });

  it("GlmCapacityBoard: null-деньги → «ждём данные», никогда не $0", () => {
    const html = renderToString(
      <GlmCapacityBoard
        nowMs={GLM_NOW * 1000}
        response={glmResponse({
          profiles: [{
            ...GLM_PROFILE,
            calibration: [{
              duration_secs: 18_000,
              samples: 0,
              confidence_bp: 0,
              capacity: { current_nano: null, low_nano: null, high_nano: null },
              remaining: null,
              observed_spend_nano: "0",
              observed_spend_native_units: 0,
              unattributed_fraction_units: 0,
              last_measured_at: null,
              estimator_version: 1,
            }],
          }],
        })}
      />,
    );
    const text = plain(html);
    expect(text).toContain("ждём данные");
    expect(text).toContain("ещё не измерено");
    expect(text).toContain("0/1 измерено");
    // Свежая provider quota остаётся видна даже без calibrated денег.
    expect(text).toContain("25%");
    expect(text).not.toContain("$0.00");
    expect(text).not.toContain("$45.00");
    // Native-колонка без замера — «—», а не 0.
    expect(text).not.toContain("9,000,000,000");
  });

  it("GlmCapacityBoard: pending delivery скрывает saleable деньги за «сохраняется»", () => {
    const html = renderToString(
      <GlmCapacityBoard
        nowMs={GLM_NOW * 1000}
        response={glmResponse({ delivery: { pending_events: 2, dropped_events: 0, persistence_ok: true } })}
      />,
    );
    const text = plain(html);
    expect(text).toContain("сохраняется");
    expect(text).not.toContain("$45.00");
    expect(text).not.toContain("$176.00");
    // Quota — live provider fact и не скрывается.
    expect(text).toContain("25%");
    expect(text).toContain("quota уже доступна");
  });

  it("GlmCapacityBoard: dropped/persistence-сбой скрывает stale API-$ за «обновляем»", () => {
    const html = renderToString(
      <GlmCapacityBoard
        nowMs={GLM_NOW * 1000}
        response={glmResponse({ delivery: { pending_events: 0, dropped_events: 1, persistence_ok: false } })}
      />,
    );
    const text = plain(html);
    expect(text).toContain("обновляем");
    expect(text).not.toContain("$45.00");
    expect(text).not.toContain("$176.00");
    expect(text).toContain("25%");
  });

  it("GlmCapacityBoard: dead/suspect/cooling профили — без saleable денег", () => {
    const dead = renderToString(
      <GlmCapacityBoard
        nowMs={GLM_NOW * 1000}
        response={glmResponse({
          fleet: { ...glmResponse().fleet, live_profiles: 0, available_profiles: 0, account_dead_profiles: 1 },
          profiles: [{ ...GLM_PROFILE, live: false, account_dead: true }],
        })}
      />,
    );
    const deadText = plain(dead);
    expect(deadText).toContain("вне ротации");
    expect(deadText).toContain("не входит в ёмкость");
    expect(deadText).not.toContain("$45.00");
    // Quota остаётся диагностикой, как у Gemini и KIMI.
    expect(deadText).toContain("25%");

    const suspect = renderToString(
      <GlmCapacityBoard
        nowMs={GLM_NOW * 1000}
        response={glmResponse({
          fleet: { ...glmResponse().fleet, available_profiles: 0, account_suspect_profiles: 1 },
          profiles: [{ ...GLM_PROFILE, account_suspect: true }],
        })}
      />,
    );
    const suspectText = plain(suspect);
    expect(suspectText).toContain(">под наблюдением</span>");
    expect(suspectText).toContain("вне ротации");
    expect(suspectText).not.toContain("$45.00");

    const cooling = renderToString(
      <GlmCapacityBoard
        nowMs={GLM_NOW * 1000}
        response={glmResponse({
          fleet: { ...glmResponse().fleet, available_profiles: 0, quota_cooling_profiles: 1 },
          profiles: [{
            ...GLM_PROFILE,
            cooling: { transport_until: null, quota_until: GLM_NOW + 300 },
          }],
        })}
      />,
    );
    const coolingText = plain(cooling);
    expect(coolingText).toContain("cooling quota 5м");
    expect(coolingText).toContain("вне ротации");
    expect(coolingText).not.toContain("$45.00");
  });

  it("GlmCapacityBoard: протухший snapshot → «обновляем» и без денег", () => {
    const stale = GLM_NOW - 1_200;
    const html = renderToString(
      <GlmCapacityBoard
        nowMs={GLM_NOW * 1000}
        response={glmResponse({
          profiles: [{
            ...GLM_PROFILE,
            quota_observed_at: stale,
            quota: GLM_PROFILE.quota?.map((window) => ({ ...window, observed_at: stale })),
            calibration: GLM_PROFILE.calibration?.map((row) => ({ ...row, last_measured_at: stale })),
          }],
        })}
      />,
    );
    const text = plain(html);
    expect(text).toContain(">обновляем</span>");
    expect(text).toContain("ждём свежую квоту");
    expect(text).not.toContain("$45.00");
    expect(text).toContain("25%");
  });

  it("GlmCapacityBoard: несколько окон не плодят строки; нестандартное окно подписано честно", () => {
    const html = renderToString(
      <GlmCapacityBoard
        nowMs={GLM_NOW * 1000}
        response={glmResponse({
          profiles: [{
            ...GLM_PROFILE,
            quota: [
              ...(GLM_PROFILE.quota ?? []),
              {
                duration_secs: 3_600,
                used_units: 500,
                limit_units: 2_000,
                remaining_units: 1_500,
                used_fraction_units: 25_000_000,
                measurement_resolution_fraction_units: 100_000,
                resets_at: GLM_NOW + 900,
                observed_at: GLM_NOW - 29,
              },
            ],
          }],
        })}
      />,
    );
    const text = plain(html);
    // 3600 секунд — это «1ч», а не фиктивный 5ч-эквивалент.
    expect(text).toContain("Quota 1ч / reset");
    expect(text).toContain("Доступно $ · 1ч");
    expect(text).toContain("сброс 15м");
    // header + ровно одна строка профиля при трёх окнах.
    expect(html.match(/<tr/g)).toHaveLength(2);
    expect(html.match(/provider-quota-meter/g)).toHaveLength(3);
    // У окна без calibration — «ждём данные», у измеренных — exact деньги.
    expect(text).toContain("ждём данные");
    expect(text).toContain("$45.00");
  });

  it("GlmCapacityBoard: BigInt-суммы пула и limit-взвешенная used-доля в strip", () => {
    const second: GlmProfile = {
      ...GLM_PROFILE,
      id: "glm-0be48d1a726c5f39",
      plan: "Lite",
      quota: [{
        duration_secs: 18_000,
        used_units: 1_000,
        limit_units: 2_000,
        remaining_units: 1_000,
        used_fraction_units: 50_000_000,
        measurement_resolution_fraction_units: 100_000,
        resets_at: GLM_NOW + 3_000,
        observed_at: GLM_NOW - 10,
      }],
      calibration: [{
        duration_secs: 18_000,
        samples: 2,
        confidence_bp: 7_000,
        capacity: { current_nano: "40000000000", low_nano: null, high_nano: null },
        remaining: { native_units: 1_000_000_000, api_nano: "15000000000" },
        observed_spend_nano: "25000000000",
        observed_spend_native_units: 1_000_000_000,
        unattributed_fraction_units: 0,
        last_measured_at: GLM_NOW - 30,
        estimator_version: 1,
      }],
    };
    const first: GlmProfile = { ...GLM_PROFILE, quota: [GLM_PROFILE.quota![0]], calibration: [GLM_PROFILE.calibration![0]] };
    const html = renderToString(
      <GlmCapacityBoard
        nowMs={GLM_NOW * 1000}
        response={glmResponse({
          fleet: { ...glmResponse().fleet, profiles: 2, live_profiles: 2, available_profiles: 2 },
          profiles: [first, second],
        })}
      />,
    );
    const text = plain(html);
    // 45e9 + 15e9 = $60.00 из 60e9 + 40e9 = $100.00 — никакой float-математики.
    expect(text).toContain("$60.00");
    expect(text).toContain("из $100.00");
    // (25e6·12000 + 50e6·2000) / 14000 = 28_571_428.57… → 28.6%.
    expect(text).toContain("28.6%");
    expect(text).toContain("2/2");
    expect(text).toContain("2/2 измерено");
    // header + две identity.
    expect(html.match(/<tr/g)).toHaveLength(3);
  });

  it("FleetCapacityOverview: GLM-карточка с реальными окнами рядом с остальными флотами", () => {
    const html = renderToString(
      <FleetCapacityOverview
        claude={{
          calibrated: true,
          per_sub: [{ routable: true }],
          window_totals: [
            { window_minutes: 300, capacity_nano: "60000000000", remaining_nano: "45000000000", routable_subs: 1, calibrated_subs: 1 },
            { window_minutes: 10_080, capacity_nano: "200000000000", remaining_nano: "120000000000" },
          ],
        }}
        gpt={{
          enabled: true,
          available: 1,
          homes: [{ process_live: true, admitted: true }],
          window_totals: [
            { window_minutes: 300, capacity_nano: "80000000000", remaining_nano: "40000000000", measured_homes: 1, observed_homes: 1 },
            { window_minutes: 10_080, capacity_nano: "300000000000", remaining_nano: "210000000000" },
          ],
        }}
        gemini={{
          enabled: true,
          available: 1,
          calibration_authority_available: true,
          calibration_delivery: { pending_events: 0, dropped_events: 0, persistence_ok: true },
          profiles: [{ authenticated: true }],
          window_totals: [
            { window_minutes: 300, capacity_nano: "50000000000", remaining_nano: "30000000000", measured_profiles: 1, observed_profiles: 1 },
            { window_minutes: 10_080, capacity_nano: "250000000000", remaining_nano: "150000000000" },
          ],
        }}
        kimi={kimiResponse()}
        glm={glmResponse()}
      />,
    );
    const text = plain(html);
    expect(text).toContain("GLM");
    expect(html).toContain("fleet-glm");
    // 5 флотов × 2 rail: GLM показывает свои реальные 5ч/7д из window_totals.
    expect(html.match(/fleet-window-rail/g)).toHaveLength(10);
    expect(text).toContain("/ $60.00");
    expect(text).toContain("/ $200.00");
    expect(text).toContain("1/1 измерено");
    expect(html).toContain("fleet-state ok");
  });

  it("FleetCapacityOverview: GLM window_totals без денег — «ждём данные» вместо $0", () => {
    const html = renderToString(
      <FleetCapacityOverview
        claude={null}
        gpt={null}
        gemini={null}
        glm={glmResponse({
          window_totals: [
            { window_minutes: 300, duration_secs: 18_000, capacity_nano: null, remaining_nano: null },
            { window_minutes: 10_080, duration_secs: 604_800, capacity_nano: null, remaining_nano: null },
          ],
          profiles: [{
            ...GLM_PROFILE,
            calibration: [{
              duration_secs: 18_000,
              samples: 0,
              confidence_bp: 0,
              capacity: { current_nano: null, low_nano: null, high_nano: null },
              remaining: null,
              observed_spend_nano: "0",
              observed_spend_native_units: 0,
              unattributed_fraction_units: 0,
              last_measured_at: null,
              estimator_version: 1,
            }],
          }],
        })}
      />,
    );
    const text = plain(html);
    expect(text).toContain("ждём данные");
    expect(text).not.toContain("$0.00");
    expect(text).not.toContain("$45.00");
    expect(text).toContain("0/1 измерено");
    expect(html).toContain("fleet-state warn");
  });

  it("FleetCapacityOverview: GLM без window_totals деградирует в coverage-only, как KIMI", () => {
    const response = glmResponse();
    delete response.window_totals;
    const html = renderToString(
      <FleetCapacityOverview claude={null} gpt={null} gemini={null} kimi={null} glm={response} />,
    );
    const text = plain(html);
    // Ни одного rail без окон, но measured coverage из per-profile calibration остаётся.
    expect(html).not.toContain("fleet-window-rail");
    expect(html).toContain("fleet-glm");
    expect(text).toContain("1/1 измерено");
    expect(text).not.toContain("$45.00");
  });

  it("FleetCapacityOverview: очередь и потеря GLM delivery скрывают деньги флота", () => {
    const pending = renderToString(
      <FleetCapacityOverview
        claude={null}
        gpt={null}
        gemini={null}
        glm={glmResponse({ delivery: { pending_events: 2, dropped_events: 0, persistence_ok: true } })}
      />,
    );
    expect(plain(pending)).toContain("2 сохраняется");
    expect(plain(pending)).not.toContain("$45.00");

    const dropped = renderToString(
      <FleetCapacityOverview
        claude={null}
        gpt={null}
        gemini={null}
        glm={glmResponse({ delivery: { pending_events: 0, dropped_events: 1, persistence_ok: false } })}
      />,
    );
    expect(plain(dropped)).toContain("1 потеряно");
    expect(plain(dropped)).not.toContain("$45.00");
    expect(dropped).toContain("fleet-state bad");
  });

  it("FleetCapacityOverview: disabled envelope и недоступный GLM — честные состояния", () => {
    const off = renderToString(
      <FleetCapacityOverview
        claude={null}
        gpt={null}
        gemini={null}
        glm={{ now: GLM_NOW, enabled: false, profiles: [] }}
      />,
    );
    expect(plain(off)).toContain("выключен");
    expect(off).toContain("fleet-glm");
    expect(off).toContain("fleet-state bad");

    const down = renderToString(<FleetCapacityOverview claude={null} gpt={null} gemini={null} glm={null} />);
    expect(plain(down)).toContain("нет связи");
    expect(down).toContain("fleet-glm");
  });
});

describe("Подписки (subs page)", () => {
  it("рендерится без падения: начальное состояние — скелетон загрузки", () => {
    // fetch на всякий случай замокан: при SSR-рендере эффекты не исполняются,
    // но страница не должна трогать сеть до монтирования.
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const html = renderToString(<SubsPage />);
    expect(html).toContain("Подписки");
    expect(html).toContain("loading-grid");

    expect(fetchMock).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });
});

describe("deadLabel", () => {
  it("маппит причины смерти токена на русские подписи", () => {
    expect(deadLabel("permission_error")).toBe("токен мёртв · бан");
    expect(deadLabel("authentication_error")).toBe("токен мёртв · нужен re-auth");
    expect(deadLabel("other")).toBe("токен мёртв");
    expect(deadLabel(undefined)).toBe("токен мёртв");
  });
});

describe("бары окон", () => {
  it("barFromUtil: доля 0..1 → процент с тоном по порогам 70/95", () => {
    expect(barFromUtil(0.5)).toEqual({ percent: 50, kind: "" });
    expect(barFromUtil(0.7)).toEqual({ percent: 70, kind: "warn" });
    expect(barFromUtil(0.95)).toEqual({ percent: 95, kind: "bad" });
    expect(barFromUtil(undefined)).toEqual({ percent: 0, kind: "" });
    expect(barFromUtil(1.4)).toEqual({ percent: 100, kind: "bad" });
  });

  it("barFromPercent: готовый процент clampится к 0..100", () => {
    expect(barFromPercent(120)).toEqual({ percent: 100, kind: "bad" });
    expect(barFromPercent(-5)).toEqual({ percent: 0, kind: "" });
    expect(barFromPercent(null)).toEqual({ percent: 0, kind: "" });
  });

  it("barFromRemaining: остаток инвертируется в расход с общими порогами 70/95", () => {
    expect(barFromRemaining(1)).toEqual({ percent: 0, kind: "" });
    expect(barFromRemaining(0.8)).toEqual({ percent: 20, kind: "" });
    expect(barFromRemaining(0.3)).toEqual({ percent: 70, kind: "warn" });
    expect(barFromRemaining(0.05)).toEqual({ percent: 95, kind: "bad" });
    expect(barFromRemaining(0)).toEqual({ percent: 100, kind: "bad" });
    expect(barFromRemaining(undefined)).toEqual({ percent: 0, kind: "" });
  });
});

describe("stripProxyPort", () => {
  it("обрезает порт и подставляет тире для пустого host", () => {
    expect(stripProxyPort("gw.example.com:8080")).toBe("gw.example.com");
    expect(stripProxyPort("gw.example.com")).toBe("gw.example.com");
    expect(stripProxyPort(undefined)).toBe("—");
    expect(stripProxyPort("")).toBe("—");
  });
});

describe("homeStatus (вердикт допуска gateway)", () => {
  const now = 1_000_000;

  it("остановленный процесс — bad", () => {
    expect(homeStatus({ process_live: false }, now)).toEqual({ label: "процесс остановлен", kind: "bad" });
  });

  it("reject_reason маппится на конкретные пилюли", () => {
    expect(homeStatus({ process_live: true, reject_reason: "account_dead" }, now).label).toBe("подписка мертва");
    expect(homeStatus({ process_live: true, reject_reason: "transport_wedged" }, now).label).toBe(
      "не отвечает · транспорт",
    );
    expect(homeStatus({ process_live: true, reject_reason: "provider_limit" }, now)).toEqual({
      label: "лимит достигнут",
      kind: "warn",
    });
    expect(homeStatus({ process_live: true, admitted: false }, now).label).toBe("вне ротации");
  });

  it("cooling показывает оставшееся время (неотрицательное)", () => {
    const status = homeStatus({ process_live: true, reject_reason: "cooling", cooling_until: now + 7200 }, now);
    expect(status).toEqual({ label: "cooling 2ч 0м", kind: "warn" });
    expect(homeStatus({ process_live: true, reject_reason: "cooling", cooling_until: now - 5 }, now).label).toBe(
      "cooling 0м",
    );
  });

  it("warn-состояния admitted home и здоровый active", () => {
    expect(homeStatus({ process_live: true, account_state: "suspect" }, now).label).toBe("active · auth под вопросом");
    expect(homeStatus({ process_live: true, snapshot_age_secs: 601 }, now).label).toBe("active · данные устарели");
    expect(homeStatus({ process_live: true, calibration_persistence_ok: false }, now).label).toBe(
      "active · calibration storage",
    );
    expect(homeStatus({ process_live: true }, now)).toEqual({ label: "active", kind: "ok" });
  });
});

describe("geminiProfileStatus (профиль подписки)", () => {
  const now = 1_000_000;

  it("цепочка приоритетов: auth → profile/model cooling → degradation → calibration", () => {
    expect(geminiProfileStatus({ authenticated: false }, now)).toEqual({ label: "ошибка auth", kind: "bad" });
    expect(geminiProfileStatus({ authenticated: true, cooling_until: now + 300 }, now)).toEqual({
      label: "cooling 5м",
      kind: "warn",
    });
    expect(
      geminiProfileStatus(
        { authenticated: true, model_cooling: [{ cooling_until: now + 300 }, { cooling_until: now + 600 }] },
        now,
      ),
    ).toEqual({ label: "модели cooling 5м", kind: "warn" });
    expect(
      geminiProfileStatus(
        { authenticated: true, model_cooling: [{ failure_streak: 3 }, { last_success_at: now - 10 }] },
        now,
      ),
    ).toEqual({
      label: "active · 1 модель degraded",
      kind: "warn",
    });
    expect(geminiProfileStatus({ authenticated: true, calibration_persistence_ok: false }, now).label).toBe(
      "active · calibration storage",
    );
  });

  it("authenticated профиль остаётся active без probe отдельных моделей", () => {
    expect(geminiProfileStatus({ authenticated: true, model_cooling: [{}, {}] }, now)).toEqual({
      label: "active",
      kind: "ok",
    });
  });
});

describe("kimiWindowLabel (окна подписаны реальной длительностью)", () => {
  it("18000 → 5ч, 604800 → 7д, остальные — по фактической длительности", () => {
    expect(kimiWindowLabel(18_000)).toBe("5ч");
    expect(kimiWindowLabel(604_800)).toBe("7д");
    expect(kimiWindowLabel(3_600)).toBe("1ч");
    expect(kimiWindowLabel(900)).toBe("15м");
    expect(kimiWindowLabel(45)).toBe("45с");
    expect(kimiWindowLabel(0)).toBe("окно");
    expect(kimiWindowLabel(undefined)).toBe("окно");
  });
});

describe("kimiProfileStatus (dead / cooling-оси / stale / пусто / active)", () => {
  const now = KIMI_NOW;

  it("dead профиль — «вне ротации» bad", () => {
    expect(kimiProfileStatus({ live: false }, now)).toEqual({ label: "вне ротации", kind: "bad" });
  });

  it("cooling-оси показывают имя оси и отсчёт до последнего until", () => {
    expect(
      kimiProfileStatus({ live: true, cooling: { quota_until: now + 300 } }, now),
    ).toEqual({ label: "cooling quota 5м", kind: "warn" });
    expect(
      kimiProfileStatus({ live: true, cooling: { auth_until: now + 600, quota_until: now + 300 } }, now).label,
    ).toBe("cooling auth+quota 10м");
    expect(
      kimiProfileStatus({ live: true, cooling: { transport_until: now + 90 } }, now).label,
    ).toBe("cooling транспорт 1м");
  });

  it("без наблюдений — «ждём данные», протухшие — «обновляем», свежие — active", () => {
    expect(kimiProfileStatus({ live: true }, now)).toEqual({ label: "ждём данные", kind: "warn" });
    expect(kimiProfileStatus({ live: true, quota_observed_at: now - 601 }, now)).toEqual({
      label: "обновляем",
      kind: "warn",
    });
    expect(kimiProfileStatus({ live: true, quota_observed_at: now - 30 }, now)).toEqual({ label: "active", kind: "ok" });
  });
});

describe("kimiUsedPercent / kimiFleetUsedPercent (BigInt, без float)", () => {
  it("exact процент с шагом 0.1 и clamp к 0..100", () => {
    expect(kimiUsedPercent(25_000_000)).toEqual({ value: 25, label: "25%" });
    expect(kimiUsedPercent(33_333_333)).toEqual({ value: 33.3, label: "33.3%" });
    expect(kimiUsedPercent(100_000_000)).toEqual({ value: 100, label: "100%" });
    expect(kimiUsedPercent(150_000_000)).toEqual({ value: 100, label: "100%" });
    expect(kimiUsedPercent(null)).toEqual({ value: null, label: "—" });
  });

  it("fleet-доля взвешивается по limit_units окон", () => {
    const profiles: KimiProfile[] = [
      { live: true, quota: [{ duration_secs: 18_000, used_fraction_units: 25_000_000, limit_units: "100" }] },
      { live: true, quota: [{ duration_secs: 18_000, used_fraction_units: 50_000_000, limit_units: "300" }] },
    ];
    // (25e6·100 + 50e6·300) / 400 = 43_750_000 → 43.8%.
    expect(kimiFleetUsedPercent(profiles, 18_000)).toEqual({ value: 43.8, label: "43.8%" });
    expect(kimiFleetUsedPercent(profiles, 604_800)).toEqual({ value: null, label: "—" });
  });
});

describe("kimiFleetWindowMoney (fail-closed суммы)", () => {
  it("суммирует decimal strings по live-профилям", () => {
    const profiles: KimiProfile[] = [
      { live: true, calibration: [{ duration_secs: 18_000, capacity: { current_nano: "60000000000" }, remaining: { api_nano: "45000000000" } }] },
      { live: true, calibration: [{ duration_secs: 18_000, capacity: { current_nano: "40000000000" }, remaining: { api_nano: "15000000000" } }] },
    ];
    expect(kimiFleetWindowMoney(profiles, 18_000, KIMI_NOW)).toEqual({ capacity: "100000000000", remaining: "60000000000" });
  });

  it("null у любого live-профиля делает итог неизвестным, dead не участвуют", () => {
    const profiles: KimiProfile[] = [
      { live: true, calibration: [{ duration_secs: 18_000, capacity: { current_nano: "60000000000" }, remaining: { api_nano: "45000000000" } }] },
      { live: true, calibration: [{ duration_secs: 18_000, capacity: { current_nano: null }, remaining: { api_nano: null } }] },
      { live: false, calibration: [{ duration_secs: 18_000, capacity: { current_nano: "99000000000" }, remaining: { api_nano: "99000000000" } }] },
    ];
    expect(kimiFleetWindowMoney(profiles, 18_000, KIMI_NOW)).toEqual({ capacity: null, remaining: null });
    expect(kimiFleetWindowMoney([], 18_000, KIMI_NOW)).toEqual({ capacity: null, remaining: null });
  });

  it("cooling и stale профили не продаются: их деньги не входят во fleet-итог", () => {
    const profiles: KimiProfile[] = [
      { live: true, cooling: { quota_until: KIMI_NOW + 300 }, quota_observed_at: KIMI_NOW - 10, calibration: [{ duration_secs: 18_000, capacity: { current_nano: "60000000000" }, remaining: { api_nano: "45000000000" } }] },
      { live: true, quota_observed_at: KIMI_NOW - 1_200, calibration: [{ duration_secs: 18_000, capacity: { current_nano: "40000000000" }, remaining: { api_nano: "15000000000" } }] },
    ];
    expect(kimiFleetWindowMoney(profiles, 18_000, KIMI_NOW)).toEqual({ capacity: null, remaining: null });
  });
});

describe("kimiMeasuredCoverage", () => {
  it("считает только профили с samples > 0 в конкретном окне", () => {
    const profiles: KimiProfile[] = [
      { live: true, calibration: [{ duration_secs: 18_000, samples: 4 }] },
      { live: true, calibration: [{ duration_secs: 18_000, samples: 0 }, { duration_secs: 604_800, samples: 1 }] },
    ];
    expect(kimiMeasuredCoverage(profiles, 18_000)).toEqual({ measured: 1, observed: 2 });
    expect(kimiMeasuredCoverage(profiles, 604_800)).toEqual({ measured: 1, observed: 2 });
    expect(kimiMeasuredCoverage([], 18_000)).toEqual({ measured: 0, observed: 0 });
  });
});

describe("glmWindowLabel (окна подписаны реальной длительностью)", () => {
  it("18000 → 5ч, 604800 → 7д, остальные — по фактической длительности", () => {
    expect(glmWindowLabel(18_000)).toBe("5ч");
    expect(glmWindowLabel(604_800)).toBe("7д");
    expect(glmWindowLabel(3_600)).toBe("1ч");
    expect(glmWindowLabel(900)).toBe("15м");
    expect(glmWindowLabel(45)).toBe("45с");
    expect(glmWindowLabel(0)).toBe("окно");
    expect(glmWindowLabel(undefined)).toBe("окно");
  });
});

describe("glmProfileStatus (dead / suspect / cooling-оси / stale / пусто / active)", () => {
  const now = GLM_NOW;

  it("account_dead — «вне ротации» bad (durable, до замены ключа)", () => {
    expect(glmProfileStatus({ live: false, account_dead: true }, now)).toEqual({
      label: "вне ротации",
      kind: "bad",
    });
  });

  it("account_suspect — «под наблюдением» warn до свежего probe", () => {
    expect(glmProfileStatus({ live: true, account_suspect: true }, now)).toEqual({
      label: "под наблюдением",
      kind: "warn",
    });
  });

  it("cooling-оси показывают имя оси и отсчёт до последнего until", () => {
    expect(
      glmProfileStatus({ live: true, cooling: { quota_until: now + 300 } }, now),
    ).toEqual({ label: "cooling quota 5м", kind: "warn" });
    expect(
      glmProfileStatus({ live: true, cooling: { transport_until: now + 600, quota_until: now + 300 } }, now).label,
    ).toBe("cooling транспорт+quota 10м");
    expect(
      glmProfileStatus({ live: true, cooling: { transport_until: now + 90 } }, now).label,
    ).toBe("cooling транспорт 1м");
  });

  it("ключ без прошедшего probe (live:false) — «ждём данные», не «вне ротации»", () => {
    expect(glmProfileStatus({ live: false, quota_observed_at: now - 30 }, now)).toEqual({
      label: "ждём данные",
      kind: "warn",
    });
  });

  it("без наблюдений — «ждём данные», протухшие — «обновляем», свежие — active", () => {
    expect(glmProfileStatus({ live: true }, now)).toEqual({ label: "ждём данные", kind: "warn" });
    expect(glmProfileStatus({ live: true, quota_observed_at: now - 601 }, now)).toEqual({
      label: "обновляем",
      kind: "warn",
    });
    expect(glmProfileStatus({ live: true, quota_observed_at: now - 30 }, now)).toEqual({ label: "active", kind: "ok" });
  });
});

describe("glmUsedPercent / glmFleetUsedPercent (BigInt, без float)", () => {
  it("exact процент с шагом 0.1 и clamp к 0..100", () => {
    expect(glmUsedPercent(25_000_000)).toEqual({ value: 25, label: "25%" });
    expect(glmUsedPercent(33_333_333)).toEqual({ value: 33.3, label: "33.3%" });
    expect(glmUsedPercent(100_000_000)).toEqual({ value: 100, label: "100%" });
    expect(glmUsedPercent(150_000_000)).toEqual({ value: 100, label: "100%" });
    expect(glmUsedPercent(null)).toEqual({ value: null, label: "—" });
  });

  it("fleet-доля взвешивается по limit_units окон", () => {
    const profiles: GlmProfile[] = [
      { live: true, quota: [{ duration_secs: 18_000, used_fraction_units: 25_000_000, limit_units: 12_000 }] },
      { live: true, quota: [{ duration_secs: 18_000, used_fraction_units: 50_000_000, limit_units: 2_000 }] },
    ];
    // (25e6·12000 + 50e6·2000) / 14000 = 28_571_428.57… → 28.6%.
    expect(glmFleetUsedPercent(profiles, 18_000)).toEqual({ value: 28.6, label: "28.6%" });
    expect(glmFleetUsedPercent(profiles, 604_800)).toEqual({ value: null, label: "—" });
  });
});

describe("glmFleetWindowMoney (fail-closed суммы)", () => {
  it("суммирует decimal strings по продаваемым профилям", () => {
    const profiles: GlmProfile[] = [
      { live: true, calibration: [{ duration_secs: 18_000, capacity: { current_nano: "60000000000" }, remaining: { api_nano: "45000000000" } }] },
      { live: true, calibration: [{ duration_secs: 18_000, capacity: { current_nano: "40000000000" }, remaining: { api_nano: "15000000000" } }] },
    ];
    expect(glmFleetWindowMoney(profiles, 18_000, GLM_NOW)).toEqual({ capacity: "100000000000", remaining: "60000000000" });
  });

  it("null у любого продаваемого профиля делает итог неизвестным", () => {
    const profiles: GlmProfile[] = [
      { live: true, calibration: [{ duration_secs: 18_000, capacity: { current_nano: "60000000000" }, remaining: { api_nano: "45000000000" } }] },
      { live: true, calibration: [{ duration_secs: 18_000, capacity: { current_nano: null }, remaining: { api_nano: null } }] },
    ];
    expect(glmFleetWindowMoney(profiles, 18_000, GLM_NOW)).toEqual({ capacity: null, remaining: null });
    expect(glmFleetWindowMoney([], 18_000, GLM_NOW)).toEqual({ capacity: null, remaining: null });
  });

  it("dead, suspect, cooling и stale профили не продаются: их деньги не входят во fleet-итог", () => {
    const good: GlmProfile = {
      live: true,
      calibration: [{ duration_secs: 18_000, capacity: { current_nano: "60000000000" }, remaining: { api_nano: "45000000000" } }],
    };
    const rich = { duration_secs: 18_000, capacity: { current_nano: "99000000000" }, remaining: { api_nano: "99000000000" } };
    const dead: GlmProfile = { live: false, account_dead: true, calibration: [rich] };
    const suspect: GlmProfile = { live: true, account_suspect: true, calibration: [rich] };
    const cooling: GlmProfile = { live: true, cooling: { quota_until: GLM_NOW + 300 }, calibration: [rich] };
    const stale: GlmProfile = { live: true, quota_observed_at: GLM_NOW - 1_200, calibration: [rich] };
    for (const excluded of [dead, suspect, cooling, stale]) {
      expect(glmFleetWindowMoney([good, excluded], 18_000, GLM_NOW)).toEqual({
        capacity: "60000000000",
        remaining: "45000000000",
      });
    }
  });
});

describe("glmMeasuredCoverage", () => {
  it("считает только профили с samples > 0 в конкретном окне", () => {
    const profiles: GlmProfile[] = [
      { live: true, calibration: [{ duration_secs: 18_000, samples: 4 }] },
      { live: true, calibration: [{ duration_secs: 18_000, samples: 0 }, { duration_secs: 604_800, samples: 1 }] },
    ];
    expect(glmMeasuredCoverage(profiles, 18_000)).toEqual({ measured: 1, observed: 2 });
    expect(glmMeasuredCoverage(profiles, 604_800)).toEqual({ measured: 1, observed: 2 });
    expect(glmMeasuredCoverage([], 18_000)).toEqual({ measured: 0, observed: 0 });
  });
});

describe("resolveBanner (приоритеты баннера флота)", () => {
  it("всё здорово → ok-баннер со сводкой флота", () => {
    expect(resolveBanner(OK_BANNER)).toEqual({
      kind: "ok",
      title: "Все пять флотов подписок в ротации",
      sub: "Claude 3 · GPT 2 · Gemini 1 · KIMI 1 · GLM 1 · обновлено 31.07.2026, 19:00",
    });
  });

  it("dead имеет высший приоритет и несёт счётчик suspect в подписи", () => {
    const banner = resolveBanner({ ...OK_BANNER, dead: 2, suspect: 1, subsDown: true, gptDown: true });
    expect(banner.kind).toBe("bad");
    expect(banner.title).toBe("2 Claude-подписки с мёртвым токеном");
    expect(banner.sub).toContain("1 под наблюдением");
  });

  it("падения источников идут в порядке Claude → GPT → Gemini → KIMI → GLM", () => {
    expect(resolveBanner({ ...OK_BANNER, subsDown: true, gptDown: true }).title).toBe(
      "Claude lifecycle-источник недоступен",
    );
    expect(resolveBanner({ ...OK_BANNER, gptDown: true, geminiDown: true }).title).toBe(
      "GPT-контур (OpenAI Codex) не отвечает",
    );
    expect(resolveBanner({ ...OK_BANNER, geminiDown: true, kimiDown: true }).title).toBe("Gemini-контур не отвечает");
    expect(resolveBanner({ ...OK_BANNER, geminiEmpty: true }).title).toBe("В Gemini-пуле нет профилей");
    expect(resolveBanner({ ...OK_BANNER, kimiDown: true, glmDown: true }).title).toBe("KIMI-контур не отвечает");
    expect(resolveBanner({ ...OK_BANNER, kimiEmpty: true }).title).toBe("В KIMI-пуле нет профилей");
    expect(resolveBanner({ ...OK_BANNER, glmDown: true }).title).toBe("GLM-контур не отвечает");
    expect(resolveBanner({ ...OK_BANNER, glmEmpty: true }).title).toBe("В GLM-пуле нет профилей");
  });

  it("KIMI-сбои: недоступность пула идёт после диагностики Gemini и до suspect", () => {
    const banner = resolveBanner({ ...OK_BANNER, kimiUnavailable: true });
    expect(banner.kind).toBe("warn");
    expect(banner.title).toBe("KIMI: нет доступных профилей");
    expect(resolveBanner({ ...OK_BANNER, kimiUnavailable: true, suspect: 1 }).title).toBe(
      "KIMI: нет доступных профилей",
    );
  });

  it("GLM-сбои: недоступность пула идёт после диагностики KIMI и до suspect", () => {
    const banner = resolveBanner({ ...OK_BANNER, glmUnavailable: true });
    expect(banner.kind).toBe("warn");
    expect(banner.title).toBe("GLM: нет доступных профилей");
    expect(resolveBanner({ ...OK_BANNER, glmUnavailable: true, suspect: 1 }).title).toBe(
      "GLM: нет доступных профилей",
    );
  });

  it("GPT-сбои: auth и процессы склеиваются через « · »", () => {
    expect(resolveBanner({ ...OK_BANNER, gptAuthBad: 2, gptProcDown: 1 }).title).toBe(
      "2 GPT-подписки с ошибкой auth · 1 процесс остановлен",
    );
    expect(resolveBanner({ ...OK_BANNER, gptProcDown: 5 }).title).toBe("5 процессов остановлен");
  });

  it("Gemini-сбои: auth, недоступность и missing usage", () => {
    expect(resolveBanner({ ...OK_BANNER, geminiAuthBad: 1, geminiUnavailable: true }).title).toBe(
      "1 Gemini-профиль с ошибкой auth · нет доступных профилей",
    );
    expect(resolveBanner({ ...OK_BANNER, geminiMissing: 4 }).title).toBe("нет usage metadata: 4");
  });

  it("suspect — последний warn перед ok", () => {
    const banner = resolveBanner({ ...OK_BANNER, suspect: 3 });
    expect(banner.kind).toBe("warn");
    expect(banner.title).toBe("3 подписки под наблюдением (auth падает)");
  });
});
