"use client";

import { formatUsd, parseCanonicalSignedNanoUsd, type EarningRow } from "@/lib/api";

/**
 * Pure-SVG bar chart of daily earnings (nanoUSD strings).
 * BigInt-scaled to pixel heights — no float math on money values.
 */
export function EarningsChart({ items }: { items: EarningRow[] }) {
  const W = 720;
  const H = 180;
  const PAD_BOTTOM = 22;
  const PAD_TOP = 12;
  const plotH = H - PAD_BOTTOM - PAD_TOP;

  const rows = [...items].sort((a, b) => (a.date < b.date ? -1 : 1));
  const parsedValues = rows.map((r) => parseCanonicalSignedNanoUsd(r.netNano));
  if (parsedValues.some((value) => value === null)) {
    return (
      <div className="empty" style={{ padding: "36px 16px" }}>
        <div className="empty-title">Earnings chart unavailable</div>
        <div style={{ fontSize: 14 }}>The API returned an invalid money value. Refresh before relying on this chart.</div>
      </div>
    );
  }
  const values = parsedValues as bigint[];
  const magnitudes = values.map((value) => (value < 0n ? -value : value));
  const max = magnitudes.reduce((m, v) => (v > m ? v : m), 0n);

  if (rows.length === 0 || max === 0n) {
    return (
      <div className="empty" style={{ padding: "36px 16px" }}>
        <div className="empty-title">No earnings yet</div>
        <div style={{ fontSize: 14 }}>
          Your last 30 days of commission will chart here once referrals start
          spending.
        </div>
      </div>
    );
  }

  const n = rows.length;
  const gap = 3;
  const barW = (W - gap * (n - 1)) / n;

  const bars = rows.map((row, i) => {
    const v = values[i];
    // height = v / max * plotH, computed in BigInt with 4 digits of precision
    const magnitude = v < 0n ? -v : v;
    const h = Number((magnitude * 10000n) / max) / 10000 * plotH;
    const x = i * (barW + gap);
    const y = PAD_TOP + (plotH - h);
    return { x, y, h: Math.max(h, magnitude > 0n ? 2 : 0), negative: v < 0n, row };
  });

  const first = rows[0].date;
  const last = rows[n - 1].date;

  return (
    <div className="chart-wrap">
      <svg
        className="chart-svg"
        viewBox={`0 0 ${W} ${H}`}
        role="img"
        aria-label="Daily earnings for the last 30 days"
      >
        {/* baseline */}
        <line
          x1={0}
          x2={W}
          y1={H - PAD_BOTTOM}
          y2={H - PAD_BOTTOM}
          stroke="var(--border-strong)"
          strokeWidth={1}
        />
        {bars.map((b, i) => (
          <g key={rows[i].date}>
            <rect
              x={b.x}
              y={b.y}
              width={barW}
              height={b.h}
              rx={2}
              fill={b.negative ? "#d6455a" : "var(--accent)"}
              opacity={0.85}
            >
              <title>{`${b.row.date}: ${formatUsd(b.row.netNano)} net (${formatUsd(b.row.adjustmentNano)} returns; spend ${formatUsd(b.row.spendNano)})`}</title>
            </rect>
          </g>
        ))}
        <text x={0} y={H - 6} fontSize={11} fill="var(--text-faint)">
          {first}
        </text>
        <text x={W} y={H - 6} fontSize={11} fill="var(--text-faint)" textAnchor="end">
          {last}
        </text>
      </svg>
    </div>
  );
}
