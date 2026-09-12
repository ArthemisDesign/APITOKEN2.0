"use client";

import { useLayoutEffect, useRef, type ReactNode } from "react";

/** Keep the measured tooltip inside its chart card, including edge and peak days. */
export function ChartTooltip({ leftPercent, bottomPercent, children }: {
  leftPercent: number;
  bottomPercent: number;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const tooltip = ref.current;
    const plot = tooltip?.parentElement;
    const card = plot?.closest<HTMLElement>(".uchart");
    if (!tooltip || !plot || !card) return;

    const position = () => {
      const bounds = card.getBoundingClientRect();
      const anchor = plot.getBoundingClientRect();
      const tip = tooltip.getBoundingClientRect();
      const minLeft = bounds.left + 12 - anchor.left;
      const maxLeft = bounds.right - 12 - anchor.left - tip.width;
      const maxTop = bounds.bottom - 12 - anchor.top - tip.height;
      // Prefer the plot area so ordinary tooltips do not cover the heading or legend.
      const minTop = Math.max(bounds.top + 12 - anchor.top, Math.min(0, maxTop));
      tooltip.style.left = `${Math.max(minLeft, Math.min(maxLeft, anchor.width * leftPercent / 100 - tip.width / 2))}px`;
      tooltip.style.top = `${Math.max(minTop, Math.min(maxTop, anchor.height * (1 - bottomPercent / 100) - tip.height - 8))}px`;
      tooltip.style.visibility = "visible";
    };

    position();
    const observer = new ResizeObserver(position);
    observer.observe(tooltip);
    observer.observe(plot);
    observer.observe(card);
    return () => observer.disconnect();
  }, [leftPercent, bottomPercent, children]);

  return <div ref={ref} className="chart-tip chart-tip-contained" role="tooltip">{children}</div>;
}
