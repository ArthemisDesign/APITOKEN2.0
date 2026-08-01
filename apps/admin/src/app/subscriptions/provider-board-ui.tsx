import { type ReactElement, type ReactNode } from "react";
import { barFromPercent } from "./logic";

export interface ProviderStripItem {
  label: string;
  value: ReactNode;
  caption: ReactNode;
  usd?: boolean;
}

export function ProviderCapacityStrip({
  ariaLabel,
  items,
}: {
  ariaLabel: string;
  items: ProviderStripItem[];
}): ReactElement {
  return (
    <section className="provider-capacity-strip" aria-label={ariaLabel}>
      {items.map((item, index) => (
        <div className={index === 0 ? "provider-capacity-primary" : ""} key={item.label}>
          <span>{item.label}</span>
          <strong className={item.usd ? "provider-usd-ink" : undefined}>{item.value}</strong>
          <small>{item.caption}</small>
        </div>
      ))}
    </section>
  );
}

export function ProviderSection({
  overline,
  title,
  meta,
  children,
}: {
  overline: string;
  title: string;
  meta: ReactNode;
  children: ReactNode;
}): ReactElement {
  return (
    <section className="provider-compact-section">
      <header>
        <div>
          <span className="provider-overline">{overline}</span>
          <h3>{title}</h3>
        </div>
        <b>{meta}</b>
      </header>
      {children}
    </section>
  );
}

export function ProviderQuotaMeter({
  usedPercent,
  label,
  reset,
}: {
  usedPercent: number | null;
  label: string;
  reset: string;
}): ReactElement {
  const bar = barFromPercent(usedPercent);
  return (
    <div className="provider-quota-meter" title={usedPercent == null ? undefined : `Использовано ${label}`}>
      <div>
        <span className="bar" aria-hidden="true">
          <i className={bar.kind} style={{ width: `${bar.percent}%` }} />
        </span>
        <b>{label}</b>
      </div>
      <small>сброс {reset}</small>
    </div>
  );
}
