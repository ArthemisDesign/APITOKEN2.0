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
import {
  barFromPercent,
  barFromRemaining,
  barFromUtil,
  deadLabel,
  geminiProfileStatus,
  homeStatus,
  resolveBanner,
  stripProxyPort,
} from "./logic";

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
  claudeCount: 3,
  gptSummary: 2,
  geminiSummary: 1,
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

  it("ClaudeCapacityBoard: различает свежую квоту без reset, stale и аккаунт вне ротации", () => {
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
              reset5h_in: 0,
              reset7d_in: 0,
              cap5h_nano: "216730000000",
              cap7d_nano: "933330000000",
              windows: [
                {
                  window_kind: "5h",
                  snapshot_fresh: false,
                  used_fraction_units: 18_000_000,
                  capacity_nano: "216730000000",
                  remaining_nano: null,
                  missing_reason: "stale_current_quota_snapshot",
                },
                {
                  window_kind: "7d",
                  snapshot_fresh: false,
                  used_fraction_units: 93_000_000,
                  capacity_nano: "933330000000",
                  remaining_nano: null,
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
    expect(text).toContain("обновляем");
    expect(text).not.toContain("18%");
    expect(text).not.toContain("$216.73");
    expect(text).toContain("dead…");
    expect(text).toContain("вне ротации");
    expect(text).toContain("не входит в ёмкость");
    expect(text).not.toContain("$1,222.19");
    expect(text).not.toContain("99%");
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
        models={[{ id: "gemini-3-pro", available: 1, healthy: 1, degraded: 0, unknown: 0 }]}
        profiles={[
          {
            id: "prof-1",
            quotas: [
              {
                model_id: "gemini-3-pro",
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
    expect(html).toContain("gemini-3-pro");
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

describe("resolveBanner (приоритеты баннера флота)", () => {
  it("всё здорово → ok-баннер со сводкой флота", () => {
    expect(resolveBanner(OK_BANNER)).toEqual({
      kind: "ok",
      title: "Все три флота подписок в ротации",
      sub: "Claude 3 · GPT 2 · Gemini 1 · обновлено 31.07.2026, 19:00",
    });
  });

  it("dead имеет высший приоритет и несёт счётчик suspect в подписи", () => {
    const banner = resolveBanner({ ...OK_BANNER, dead: 2, suspect: 1, subsDown: true, gptDown: true });
    expect(banner.kind).toBe("bad");
    expect(banner.title).toBe("2 Claude-подписки с мёртвым токеном");
    expect(banner.sub).toContain("1 под наблюдением");
  });

  it("падения источников идут в порядке Claude → GPT → Gemini", () => {
    expect(resolveBanner({ ...OK_BANNER, subsDown: true, gptDown: true }).title).toBe(
      "Claude lifecycle-источник недоступен",
    );
    expect(resolveBanner({ ...OK_BANNER, gptDown: true, geminiDown: true }).title).toBe(
      "GPT-контур (OpenAI Codex) не отвечает",
    );
    expect(resolveBanner({ ...OK_BANNER, geminiDown: true }).title).toBe("Gemini-контур не отвечает");
    expect(resolveBanner({ ...OK_BANNER, geminiEmpty: true }).title).toBe("В Gemini-пуле нет профилей");
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
