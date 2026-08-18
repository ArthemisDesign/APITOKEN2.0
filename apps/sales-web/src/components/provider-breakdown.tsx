"use client";

import { formatUsd, type ProviderEarningsRow } from "@/lib/api";
import { EmptyState, Table } from "@/components/ui";
import { useI18n } from "@/components/i18n";

// Человекочитаемые имена провайдеров. Неизвестный id показываем как есть, а не прячем:
// в пуле появляются новые провайдеры, и заработок по ним должен быть виден сразу,
// ещё до того как кто-то добавит сюда красивое название.
const PROVIDER_LABELS: Record<string, string> = {
  anthropic: "Claude (Anthropic)",
  openai: "GPT (OpenAI)",
  google: "Gemini (Google)",
  kimi: "Kimi (Moonshot)",
};

export function providerLabel(providerId: string | null, unattributed: string): string {
  if (providerId === null) return unattributed;
  return PROVIDER_LABELS[providerId] ?? providerId;
}

/**
 * Share of total earnings, in tenths of a percent, computed on BigInt nano values only.
 * Percentages of money must not go through float: `Number(nano)` loses precision above 2^53.
 */
export function earningsShareTenths(earnedNano: string, totalNano: bigint): number {
  if (totalNano === 0n) return 0;
  return Number((BigInt(earnedNano) * 1000n) / totalNano);
}

export function ProviderBreakdown({ items }: { items: ProviderEarningsRow[] }) {
  const { t } = useI18n();
  const withMoney = items.filter((row) => row.spendNano !== "0" || row.earnedNano !== "0");

  if (withMoney.length === 0) {
    return (
      <EmptyState title={t("No usage yet", "Пока нет расхода")}>
        {t(
          "Once your referrals start using the API, this splits their spend by provider.",
          "Как только ваши рефералы начнут пользоваться API, здесь появится разбивка их расхода по провайдерам.",
        )}
      </EmptyState>
    );
  }

  const totalEarned = withMoney.reduce((sum, row) => sum + BigInt(row.earnedNano), 0n);

  return (
    <Table
      head={
        <>
          <th>{t("Provider", "Провайдер")}</th>
          <th>{t("Referral spend", "Расход рефералов")}</th>
          <th>{t("Your earnings", "Ваш заработок")}</th>
          <th>{t("Share", "Доля")}</th>
        </>
      }
    >
      {withMoney.map((row) => {
        const shareTenths = earningsShareTenths(row.earnedNano, totalEarned);
        return (
          <tr key={row.providerId ?? "(unattributed)"}>
            <td>
              {providerLabel(
                row.providerId,
                t("Before provider tracking", "До учёта провайдера"),
              )}
            </td>
            <td>{formatUsd(row.spendNano)}</td>
            <td style={{ color: "var(--accent-strong)" }}>{formatUsd(row.earnedNano)}</td>
            <td className="mono">{(shareTenths / 10).toFixed(1)}%</td>
          </tr>
        );
      })}
    </Table>
  );
}
