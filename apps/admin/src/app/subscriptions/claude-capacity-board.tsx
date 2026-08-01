"use client";

import { type ReactElement } from "react";
import { Pill, TableCard } from "@/components/ui";
import { duration, nanoMoney } from "@/lib/format";
import { providerInteger } from "./provider-calibration";
import { ProviderQuotaMeter, ProviderSection } from "./provider-board-ui";
import type { CapacityResponse, CapacitySub } from "./types";

function moneyOrDash(value: string | null | undefined): string {
  return providerInteger(value) == null ? "—" : nanoMoney(value);
}

function quotaValue(util: number | null | undefined): { value: number | null; label: string } {
  const value = Number(util);
  if (!Number.isFinite(value)) return { value: null, label: "—" };
  const percent = Math.min(100, Math.max(0, value * 100));
  const rounded = Math.round(percent * 10) / 10;
  return { value: rounded, label: `${rounded.toLocaleString("ru-RU", { maximumFractionDigits: 1 })}%` };
}

function subscriptionStatus(item: CapacitySub): { label: string; kind: "ok" | "warn" | "bad" } {
  if (item.auth_state === "dead") {
    return {
      label: item.dead_reason === "permission_error" ? "токен мёртв · бан" : "токен мёртв",
      kind: "bad",
    };
  }
  if (item.auth_state === "suspect") return { label: "auth под наблюдением", kind: "warn" };
  if (item.cooling) return { label: "cooling", kind: "warn" };
  if (item.routable === false) return { label: "вне ротации", kind: "warn" };
  if (item.calibrated === false) return { label: "ждём данные", kind: "warn" };
  return { label: "active", kind: "ok" };
}

function ClaudeSubscriptions({ items }: { items: CapacitySub[] }) {
  return (
    <ProviderSection overline="Подписки" title="Окна по аккаунтам" meta={`${items.filter((item) => item.routable).length}/${items.length} в ротации`}>
      <TableCard>
        <table className="provider-home-capacity-table">
          <thead>
            <tr>
              <th className="left">Почта</th>
              <th className="left">Состояние</th>
              <th>Quota 5ч / reset</th>
              <th className="provider-five-hour-money">Доступно $ · 5ч</th>
              <th>Quota 7д / reset</th>
              <th>Доступно $ · 7д</th>
            </tr>
          </thead>
          <tbody>
            {items.map((item, index) => {
              const five = quotaValue(item.util5h);
              const weekly = quotaValue(item.util7d);
              const health = subscriptionStatus(item);
              return (
                <tr key={`${item.email ?? "claude"}-${index}`}>
                  <td className="left"><b>{item.email || "—"}</b><small>{item.plan ?? "—"}</small></td>
                  <td className="left"><Pill kind={health.kind}>{health.label}</Pill></td>
                  <td><ProviderQuotaMeter usedPercent={five.value} label={five.label} reset={duration(item.reset5h_in)} /></td>
                  <td className="provider-usd-ink provider-five-hour-money">
                    <b>{moneyOrDash(item.rem5h_nano)}</b>
                    <small>{`из ${moneyOrDash(item.cap5h_nano)}`}</small>
                  </td>
                  <td><ProviderQuotaMeter usedPercent={weekly.value} label={weekly.label} reset={duration(item.reset7d_in)} /></td>
                  <td className="provider-usd-ink">
                    <b>{moneyOrDash(item.rem7d_nano)}</b>
                    <small>{`из ${moneyOrDash(item.cap7d_nano)}`}</small>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </TableCard>
    </ProviderSection>
  );
}

export function ClaudeCapacityBoard({ response }: { response: CapacityResponse }): ReactElement {
  return (
    <div className="provider-capacity-board claude-capacity-board">
      <ClaudeSubscriptions items={response.per_sub ?? []} />
    </div>
  );
}
