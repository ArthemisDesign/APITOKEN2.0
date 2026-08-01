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
