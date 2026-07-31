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

import AuditPage from "./page";
import {
  INITIAL_AUDIT_FILTERS,
  auditActionOptions,
  auditCsvRows,
  auditPageTotal,
  buildAuditQuery,
  clampAuditOffset,
  normalizeAuditActions,
} from "./lib";

describe("Аудит (audit page)", () => {
  it("рендерится без падения: начальное состояние — скелетон загрузки", () => {
    // fetch на всякий случай замокан: при SSR-рендере эффекты не исполняются,
    // но страница не должна трогать сеть до монтирования.
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const html = renderToString(<AuditPage />);
    expect(html).toContain("Аудит");
    expect(html).toContain("loading-grid");

    expect(fetchMock).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });
});

describe("buildAuditQuery", () => {
  it("пустые фильтры не попадают в query, limit/offset — всегда", () => {
    expect(buildAuditQuery(INITIAL_AUDIT_FILTERS)).toBe("limit=50&offset=0");
  });

  it("собирает action/actor_type/q как в легаси", () => {
    const query = buildAuditQuery({ offset: 100, limit: 50, action: "admin.credit", actorType: "user", q: "цель 1" });
    const params = new URLSearchParams(query);
    expect(params.get("limit")).toBe("50");
    expect(params.get("offset")).toBe("100");
    expect(params.get("action")).toBe("admin.credit");
    expect(params.get("actor_type")).toBe("user");
    expect(params.get("q")).toBe("цель 1");
  });
});

describe("normalizeAuditActions", () => {
  it("принимает и {actions:[...]}, и голый массив", () => {
    expect(normalizeAuditActions({ actions: ["admin.credit", "auth.login"] })).toEqual(["admin.credit", "auth.login"]);
    expect(normalizeAuditActions(["admin.credit"])).toEqual(["admin.credit"]);
  });

  it("мусор и отсутствующий ответ → пустой список, нестроки отбрасываются", () => {
    expect(normalizeAuditActions(null)).toEqual([]);
    expect(normalizeAuditActions({})).toEqual([]);
    expect(normalizeAuditActions({ actions: ["a", 1, null] })).toEqual(["a"]);
  });
});

describe("auditPageTotal", () => {
  it("без total деградирует к размеру страницы (старый backend)", () => {
    expect(auditPageTotal({ rows: [{}, {}] })).toBe(2);
    expect(auditPageTotal({ rows: [{}], total: 137 })).toBe(137);
    expect(auditPageTotal(null)).toBe(0);
  });
});

describe("clampAuditOffset", () => {
  it("offset в пределах лога не трогает", () => {
    expect(clampAuditOffset(0, 50, 137)).toBe(0);
    expect(clampAuditOffset(100, 50, 137)).toBe(100);
    expect(clampAuditOffset(50, 50, 0)).toBe(50);
  });

  it("уехавший offset откатывает на последнюю полную страницу", () => {
    expect(clampAuditOffset(150, 50, 137)).toBe(100);
    expect(clampAuditOffset(200, 50, 40)).toBe(0);
  });
});

describe("auditActionOptions", () => {
  it("выбранный action добавляется опцией первой, если его нет в списке", () => {
    expect(auditActionOptions(["a", "b"], "c")).toEqual(["c", "a", "b"]);
    expect(auditActionOptions(["a", "b"], "b")).toEqual(["a", "b"]);
    expect(auditActionOptions(["a"], "")).toEqual(["a"]);
  });
});

describe("auditCsvRows", () => {
  it("собирает колонки как кнопка audit-csv в легаси", () => {
    const rows = auditCsvRows([
      {
        created_at: "2026-07-31T10:00:00Z",
        action: "admin.credit",
        actor_type: "commercial-admin",
        actor_id: null,
        target_type: "user",
        target_id: "u1",
        metadata: { reason: "ok" },
      },
    ]);
    expect(rows).toEqual([
      ["2026-07-31T10:00:00Z", "admin.credit", "commercial-admin", "", "user", "u1", '{"reason":"ok"}'],
    ]);
  });

  it("пустые поля и metadata → дефолты легаси", () => {
    expect(auditCsvRows([{}])).toEqual([["", undefined, undefined, "", undefined, undefined, "{}"]]);
  });
});
