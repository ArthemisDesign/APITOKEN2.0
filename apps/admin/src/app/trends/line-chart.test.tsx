import { describe, expect, it } from "vitest";
import { renderToString } from "react-dom/server";
import { LineChart } from "./line-chart";

// Логика lineChart() из admin-panel.js: оси min/mid/max по Y с fmt,
// время по X, null/нечисловые значения разрывают линию.
describe("LineChart", () => {
  it("без числовых значений — empty-заглушка «данных за окно нет»", () => {
    const html = renderToString(
      <LineChart series={[{ label: "a", points: [{ ts: 1, value: null }, { ts: 2 }] }]} />,
    );
    expect(html).toContain("данных за окно нет");
    expect(html).not.toContain("<svg");
  });

  it("рисует svg с path, легендой и подписями осей через fmt", () => {
    const html = renderToString(
      <LineChart
        series={[{ label: "cap 7д", points: [{ ts: 1000, value: 10 }, { ts: 2000, value: 20 }] }]}
        fmt={(value) => "$" + value.toFixed(2)}
      />,
    );
    expect(html).toContain("<svg");
    expect(html).toContain('aria-label="график"');
    expect(html).toContain("<path");
    expect(html).toContain("cap 7д");
    // ось Y: min (0 по умолчанию), mid, max — отформатированы через fmt
    expect(html).toContain("$0.00");
    expect(html).toContain("$10.00");
    expect(html).toContain("$20.00");
  });

  it("null разрывает линию: path содержит несколько сегментов M", () => {
    const html = renderToString(
      <LineChart
        series={[
          {
            label: "util",
            points: [
              { ts: 1000, value: 0.1 },
              { ts: 2000, value: null },
              { ts: 3000, value: 0.3 },
            ],
          },
        ]}
        min={0}
        max={1}
      />,
    );
    const d = html.match(/d="([^"]*)"/)?.[1] ?? "";
    expect(d.split("M").length - 1).toBe(2);
    expect(d).not.toContain("L");
  });

  it("непрерывный ряд даёт один сегмент M с L-продолжением", () => {
    const html = renderToString(
      <LineChart
        series={[{ label: "gap", points: [{ ts: 1000, value: 1 }, { ts: 2000, value: 2 }, { ts: 3000, value: 3 }] }]}
        fmt={(value) => String(Math.round(value))}
        min={0}
      />,
    );
    const d = html.match(/d="([^"]*)"/)?.[1] ?? "";
    expect(d.startsWith("M")).toBe(true);
    expect(d.split("L").length - 1).toBe(2);
  });

  it("окно больше суток — подписи X с датой, иначе только время", () => {
    const day = 86400;
    const long = renderToString(
      <LineChart series={[{ label: "a", points: [{ ts: 1_700_000_000, value: 1 }, { ts: 1_700_000_000 + 2 * day, value: 2 }] }]} />,
    );
    // формат DD.MM hh:mm — есть точка между днём и месяцем
    expect(long).toMatch(/\d{2}\.\d{2} \d{2}:\d{2}/);

    const short = renderToString(
      <LineChart series={[{ label: "a", points: [{ ts: 1_700_000_000, value: 1 }, { ts: 1_700_000_000 + 3600, value: 2 }] }]} />,
    );
    expect(short).toMatch(/\d{2}:\d{2}/);
    expect(short).not.toMatch(/\d{2}\.\d{2}/);
  });
});
