"use client";

// SVG-график без библиотек — порт 1:1 lineChart/trendsChart из admin-panel.js
// (CSP легаси запрещал внешние chart-библиотеки; здесь та же ручная генерация).
// null/нечисловые value разрывают линию. Оси: min/mid/max по Y с подписями fmt,
// время начала/середины/конца по X.
import type { ReactNode } from "react";
import type { ChartSeries } from "./finance-lib";

const W = 720;
const H = 190;
const PAD_L = 56;
const PAD_R = 10;
const PAD_T = 10;
const PAD_B = 22;
const PALETTE = ["var(--accent)", "var(--ok)", "var(--warn)", "var(--bad)"];

export function LineChart(props: {
  series: ChartSeries[];
  fmt?: (value: number) => string;
  min?: number;
  max?: number;
}) {
  const fmt = props.fmt ?? ((value: number) => String(value));
  const values: number[] = [];
  const times: number[] = [];
  for (const item of props.series) {
    for (const point of item.points) {
      times.push(point.ts);
      if (point.value != null && isFinite(point.value)) values.push(point.value);
    }
  }
  if (!values.length) {
    return (
      <div className="empty" style={{ padding: 26 }}>
        данных за окно нет
      </div>
    );
  }
  const x0 = Math.min(...times);
  const x1 = Math.max(...times);
  const y0 = props.min != null ? props.min : Math.min(0, ...values);
  const yMax = props.max != null ? props.max : Math.max(...values);
  const y1 = yMax > y0 ? yMax : y0 + 1;
  const X = (ts: number) => PAD_L + ((ts - x0) / Math.max(1, x1 - x0)) * (W - PAD_L - PAD_R);
  const Y = (value: number) => PAD_T + (1 - (value - y0) / (y1 - y0)) * (H - PAD_T - PAD_B);
  const timeLabel = (ts: number) => {
    const at = new Date(ts * 1000);
    const hh = String(at.getHours()).padStart(2, "0") + ":" + String(at.getMinutes()).padStart(2, "0");
    return x1 - x0 > 86400
      ? String(at.getDate()).padStart(2, "0") + "." + String(at.getMonth() + 1).padStart(2, "0") + " " + hh
      : hh;
  };

  return (
    <>
      <div style={{ marginBottom: 2 }}>
        {props.series.map((item, index) => (
          <span key={item.label} style={{ marginRight: 12, whiteSpace: "nowrap" }}>
            <span style={{ color: item.color || PALETTE[index % PALETTE.length] }}>●</span> {item.label}
          </span>
        ))}
      </div>
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" role="img" aria-label="график">
        {[y0, (y0 + y1) / 2, y1].map((value) => (
          <g key={value}>
            <line x1={PAD_L} y1={Y(value)} x2={W - PAD_R} y2={Y(value)} stroke="var(--line)" strokeWidth={1} />
            <text x={PAD_L - 6} y={Y(value) + 3} textAnchor="end" fontSize={10} fill="var(--faint)">
              {fmt(value)}
            </text>
          </g>
        ))}
        {[x0, (x0 + x1) / 2, x1].map((ts) => (
          <text
            key={ts}
            x={Math.min(Math.max(X(ts), PAD_L + 14), W - PAD_R - 14)}
            y={H - 6}
            textAnchor="middle"
            fontSize={10}
            fill="var(--faint)"
          >
            {timeLabel(ts)}
          </text>
        ))}
        {props.series.map((item, index) => {
          let d = "";
          let pen = false;
          for (const point of item.points) {
            if (point.value == null || !isFinite(point.value)) {
              pen = false;
              continue;
            }
            d += (pen ? "L" : "M") + X(point.ts).toFixed(1) + " " + Y(point.value).toFixed(1);
            pen = true;
          }
          return (
            <path
              key={item.label}
              d={d}
              fill="none"
              stroke={item.color || PALETTE[index % PALETTE.length]}
              strokeWidth={1.6}
              strokeLinejoin="round"
              strokeLinecap="round"
            />
          );
        })}
      </svg>
    </>
  );
}

// Карточка-обёртка графика — порт trendsChart(title, sub, chart).
export function ChartCard(props: { title: string; sub: string; children: ReactNode }) {
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
