import { NANO_PER_USD } from "./shared";

/** Money stays integer; only bounded display coordinates become numbers. */
export function chartRatio(value: bigint, maximum: bigint): number {
  if (maximum <= 0n || value <= 0n) return 0;
  return Number((value > maximum ? maximum : value) * 1_000_000n / maximum) / 1_000_000;
}

export function usageChartGeometry(points: { official: bigint; charged: bigint }[]) {
  const maximum = points.reduce((max, point) => [max, point.official, point.charged].reduce((a, b) => a > b ? a : b), 0n);
  const rough = (maximum + 3n) / 4n;
  const magnitude = 10n ** BigInt(Math.max(0, rough.toString().length - 1));
  const step = maximum === 0n ? NANO_PER_USD / 4n : [1n, 2n, 5n, 10n].map(n => n * magnitude).find(n => n >= rough)!;
  const ceiling = step * 4n;
  const x = (index: number) => points.length <= 1 ? 500 : index / (points.length - 1) * 1000;
  const y = (amount: bigint) => 240 - chartRatio(amount, ceiling) * 220;
  const line = (key: "official" | "charged") => points.map((point, index) => `${index ? "L" : "M"}${x(index).toFixed(2)},${y(point[key]).toFixed(2)}`).join(" ");
  const chargedLine = line("charged");
  return {
    maximum, x, y,
    ticks: Array.from({ length: 5 }, (_, index) => ({ value: ceiling - BigInt(index) * step, y: 20 + index * 55 })),
    officialLine: line("official"), chargedLine,
    chargedArea: points.length ? `${chargedLine} L${x(points.length - 1)},240 L${x(0)},240 Z` : "",
  };
}
