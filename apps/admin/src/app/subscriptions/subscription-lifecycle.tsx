import type { ReactElement } from "react";
import type { SubscriptionLifecycle } from "./types";

const DAY_SECONDS = 86_400;

export interface SubscriptionExpiryView {
  date: string;
  state: "remaining" | "expired" | "unknown";
  detail: string;
}

function absoluteDate(epochSeconds: number): string {
  return new Date(epochSeconds * 1000).toLocaleDateString("ru-RU", {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
    timeZone: "UTC",
  });
}

export function subscriptionExpiryView(
  lifecycle: SubscriptionLifecycle,
  nowSeconds: number,
): SubscriptionExpiryView {
  const expiry = lifecycle.subscription_expires_at;
  const producerDays = lifecycle.subscription_days_left;
  if (typeof expiry !== "number" || !Number.isFinite(expiry) || expiry <= 0) {
    return { date: "—", state: "unknown", detail: "неизвестно" };
  }

  const days = typeof producerDays === "number" && Number.isFinite(producerDays)
    ? producerDays
    : (expiry - nowSeconds) / DAY_SECONDS;
  if (days < 0) {
    const elapsed = Math.max(1, Math.ceil(Math.abs(days)));
    return { date: absoluteDate(expiry), state: "expired", detail: `истекла ${elapsed}д назад` };
  }

  const remaining = Math.max(0, Math.ceil(days));
  return { date: absoluteDate(expiry), state: "remaining", detail: `осталось ${remaining}д` };
}

export function SubscriptionExpiry({
  lifecycle,
  nowSeconds,
}: {
  lifecycle: SubscriptionLifecycle;
  nowSeconds: number;
}): ReactElement {
  const view = subscriptionExpiryView(lifecycle, nowSeconds);
  return (
    <td className={`subscription-expiry ${view.state}`}>
      <b>{view.date}</b>
      <small>{view.detail}</small>
    </td>
  );
}
