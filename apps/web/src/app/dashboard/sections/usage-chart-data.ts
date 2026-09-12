import { NANO_PER_USD } from "./shared";

/** Money stays integer; only bounded display coordinates become numbers. */
export function chartRatio(value: bigint, maximum: bigint): number {
  if (maximum <= 0n || value <= 0n) return 0;
  return Number((value > maximum ? maximum : value) * 1_000_000n / maximum) / 1_000_000;
}

interface ChartPoint {
  official: bigint;
  providers: { provider: string; officialNano: string }[];
  unattributedOfficialNano: string;
}

/** The original chart metric is official value, stacked by provider; charges are detail only. */
export function usageChartGeometry(points: ChartPoint[], providerIds: (string | null)[]) {
  const maximum = points.reduce((max, point) => point.official > max ? point.official : max, 0n);
  const rough = (maximum + 3n) / 4n;
  const magnitude = 10n ** BigInt(Math.max(0, rough.toString().length - 1));
  const step = maximum === 0n ? NANO_PER_USD / 4n : [1n, 2n, 5n, 10n].map(n => n * magnitude).find(n => n >= rough)!;
  const ceiling = step * 4n;
  const x = (index: number) => points.length <= 1 ? 500 : index / (points.length - 1) * 1000;
  const y = (amount: bigint) => 240 - chartRatio(amount, ceiling) * 220;
  const position = (index: number, value: bigint) => `${x(index).toFixed(2)},${y(value).toFixed(2)}`;
  const totals = points.map(() => 0n);
  const layers = providerIds.map(id => {
    const lower = [...totals];
    const values = points.map(point => {
      const amount = BigInt(id === null ? point.unattributedOfficialNano : point.providers.find(provider => provider.provider === id)?.officialNano ?? "0");
      return amount > 0n ? amount : 0n;
    });
    const upper = values.map((value, index) => totals[index] = totals[index]! + value);
    const line = upper.map((value, index) => `${index ? "L" : "M"}${position(index, value)}`).join(" ");
    const baseline = lower.map((value, index) => `L${position(index, value)}`).reverse().join(" ");
    return { id, values, upper, lower, line, area: points.length ? `${line} ${baseline} Z` : "" };
  });
  return {
    maximum, x, y,
    ticks: Array.from({ length: 5 }, (_, index) => ({ value: ceiling - BigInt(index) * step, y: 20 + index * 55 })),
    layers,
  };
}
