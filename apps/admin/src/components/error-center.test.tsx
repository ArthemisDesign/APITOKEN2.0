import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";

vi.mock("@/lib/resources", () => ({
  dismissError: vi.fn(),
  getErrorRecoveryVersion: () => 0,
  getErrors: () => [],
  refreshResource: vi.fn(),
  subscribeErrors: () => () => undefined,
}));

vi.mock("@/lib/toast", () => ({ toast: vi.fn() }));

import { ErrorNotes } from "./error-center";
import type { ResourceError } from "@/lib/resources";

const plain = (html: string): string => html.replace(/<!-- -->/g, "");

const failure = (index: number, hasData = false): ResourceError => ({
  key: index === 0 ? "/subs" : `/source-${index}`,
  message: `HTTP 50${index}`,
  dismissed: false,
  hasData,
});

describe("ErrorNotes", () => {
  it("честно различает initial failure и неудачную ревалидацию last-good", () => {
    const initial = renderToString(<ErrorNotes errors={[failure(0)]} />);
    expect(initial).toContain("Данных ещё нет");
    expect(initial).not.toContain("Последние успешные данные");

    const refresh = renderToString(<ErrorNotes errors={[failure(0, true)]} />);
    expect(refresh).toContain("Последние успешные данные остаются на экране");
  });

  it("схлопывает четыре и более ошибок в один раскрываемый блок", () => {
    const html = renderToString(<ErrorNotes errors={[failure(0), failure(1), failure(2), failure(3)]} />);
    expect(html.match(/role="alert"/g)).toHaveLength(1);
    expect(plain(html)).toContain("4 источников временно недоступны");
    expect(html).toContain("<details>");
    expect(html).toContain("Показать источники");
    expect(html).toContain("Claude-подписки");
  });
});
