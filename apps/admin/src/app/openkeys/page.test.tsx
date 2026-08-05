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

import OpenKeysPage from "./page";
import {
  buildKeysPath,
  clampOffset,
  okTypeLabel,
  PAGE_LIMIT,
  SELLER_ACTION_COPY,
  sellerActionToast,
} from "./lib";

describe("OpenKeys (page)", () => {
  it("рендерится без падения: начальное состояние — скелетон загрузки", () => {
    // fetch на всякий случай замокан: при SSR-рендере эффекты не исполняются,
    // но страница не должна трогать сеть до монтирования.
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const html = renderToString(<OpenKeysPage />);
    expect(html).toContain("OpenKeys");
    expect(html).toContain("loading-grid");

    expect(fetchMock).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });
});

describe("buildKeysPath", () => {
  it("всегда задаёт limit/offset, пустые фильтры опускает (admin-panel.js:607-609)", () => {
    expect(buildKeysPath({ offset: 0, q: "", batch: "", status: "", usage: "" })).toBe(
      "/openkeys-admin/keys?limit=50&offset=0",
    );
  });

  it("добавляет только заполненные фильтры", () => {
    expect(
      buildKeysPath({ offset: 100, q: "acct 1", batch: "b1", status: "active", usage: "used" }),
    ).toBe("/openkeys-admin/keys?limit=50&offset=100&q=acct+1&batch=b1&status=active&usage=used");
  });
});

describe("clampOffset", () => {
  it("не трогает offset внутри диапазона и при пустом каталоге", () => {
    expect(clampOffset(0, 100)).toBe(0);
    expect(clampOffset(50, 100)).toBe(50);
    expect(clampOffset(50, 0)).toBe(50);
  });

  it("откатывает уехавший offset на последнюю страницу (admin-panel.js:612)", () => {
    expect(clampOffset(100, 100)).toBe(50);
    expect(clampOffset(150, 120)).toBe(100);
    expect(clampOffset(50, 30)).toBe(0);
    expect(clampOffset(PAGE_LIMIT * 3, 1)).toBe(0);
  });
});

describe("okTypeLabel", () => {
  it("openai → OpenAI, всё остальное → Claude (admin-panel.js:407)", () => {
    expect(okTypeLabel("openai")).toBe("OpenAI");
    expect(okTypeLabel("claude")).toBe("Claude");
    expect(okTypeLabel(undefined)).toBe("Claude");
  });
});

describe("sellerActionToast", () => {
  it("сообщает счётчики, а не «готово»: сервер режет пачку по потолку", () => {
    expect(sellerActionToast({ action: "revoke", changed: 500, failed: 0, remaining: 120 })).toBe(
      "Аннулировано ключей: 500 · осталось: 120 — нажмите ещё раз.",
    );
    expect(sellerActionToast({ action: "pause", changed: 8, failed: 2, remaining: 2 })).toBe(
      "Поставлено на паузу ключей: 8 · не удалось: 2 · осталось: 2 — нажмите ещё раз.",
    );
    expect(sellerActionToast({ action: "resume", changed: 4, failed: 0, remaining: 0 })).toBe(
      "Возвращено в строй ключей: 4.",
    );
  });

  it("пустой результат не выдаёт себя за выполненное действие", () => {
    expect(sellerActionToast({ action: "pause", matched: 0, changed: 0, failed: 0, remaining: 0 })).toBe(
      "Подходящих ключей нет — ничего не изменилось.",
    );
  });
});

describe("SELLER_ACTION_COPY", () => {
  it("только аннулирование красное и заявлено как необратимое", () => {
    expect(SELLER_ACTION_COPY.revoke.danger).toBe(true);
    expect(SELLER_ACTION_COPY.revoke.message).toContain("Необратимо");
    expect(SELLER_ACTION_COPY.pause.danger).toBe(false);
    expect(SELLER_ACTION_COPY.resume.danger).toBe(false);
  });
});
