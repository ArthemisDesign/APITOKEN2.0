"use client";

import { useCallback, useEffect, useState } from "react";
import {
  api,
  ApiError,
  formatDate,
  formatUsd,
  type Overview,
  type ReferralRow,
} from "@/lib/api";
import { Badge, Button, EmptyState, Input, Loading, Notice, StatusBadge, Table } from "@/components/ui";
import { useI18n } from "@/components/i18n";

export default function ReferralsPage() {
  const { t } = useI18n();
  const [items, setItems] = useState<ReferralRow[] | null>(null);
  const [overview, setOverview] = useState<Overview | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Inline-редактор партнёрской ставки: какой реферал редактируем и введённый процент.
  const [editingRef, setEditingRef] = useState<string | null>(null);
  const [pct, setPct] = useState("");
  const [busy, setBusy] = useState(false);
  const [editError, setEditError] = useState<string | null>(null);

  const load = useCallback(async () => {
    const res = await api<{ items: ReferralRow[] }>("/v1/partner/referrals");
    setItems([...res.items].sort((a, b) => (a.attributedAt < b.attributedAt ? 1 : -1)));
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [, ov] = await Promise.all([
          load(),
          api<Overview>("/v1/partner/overview").catch(() => null),
        ]);
        if (!cancelled && ov) setOverview(ov);
      } catch (err) {
        if (!cancelled)
          setError(err instanceof ApiError ? err.message : t("Failed to load referrals.", "Не удалось загрузить рефералов."));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [load]);

  const canSetDiscount = overview?.referralDiscountEnabled === true;
  const maxPct = (overview?.referralDiscountBps ?? 0) / 100;

  function openEditor(r: ReferralRow) {
    setEditingRef(r.userRef ?? null);
    setPct(r.referralFloorBps ? String(r.referralFloorBps / 100) : "");
    setEditError(null);
  }

  async function saveDiscount(userRef: string) {
    const value = Number(pct.trim());
    if (!Number.isFinite(value) || value < 0 || value > maxPct) {
      setEditError(t(`Enter a percent between 0 and ${maxPct} (0 removes the partner rate).`, `Введите процент от 0 до ${maxPct} (0 — снять партнёрскую ставку).`));
      return;
    }
    setBusy(true);
    setEditError(null);
    try {
      await api(`/v1/partner/referrals/${userRef}/discount`, {
        method: "POST",
        body: { discountBps: Math.round(value * 100) },
      });
      setEditingRef(null);
      // Перечитываем список: эффективная скидка зависит от тира и считается на сервере.
      await load();
    } catch (err) {
      setEditError(err instanceof ApiError ? err.message : t("Could not update the rate.", "Не удалось изменить ставку."));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <h1 className="page-title">{t("Referrals", "Рефералы")}</h1>
      <p className="page-sub">
        {t(
          "Users attributed to you. Identities are masked for their privacy. “Spend” is their real-money API usage — the amount charged after their own discount, excluding anything paid from free bonus/promo balance. That is the base your commission is calculated from.",
          "Пользователи, закреплённые за вами. Личности скрыты в целях конфиденциальности. «Расход» — это их реальные траты на API: сумма, списанная после их скидки, без учёта оплаченного из бесплатного бонуса/промо. Именно от неё считается ваша комиссия.",
        )}
      </p>
      {canSetDiscount ? (
        <p className="page-sub">
          {t(
            `You can move any of your B2C referrals to a personal partner rate (up to ${maxPct}%) or change it later — use “Set rate” in the table. 0 removes the rate and returns them to the regular tiers.`,
            `Любого своего B2C-реферала можно перевести на персональную партнёрскую ставку (до ${maxPct}%) или изменить её позже — кнопка «Ставка» в таблице. 0 снимает ставку и возвращает на обычные тиры.`,
          )}
        </p>
      ) : null}
      {error ? <Notice kind="error">{error}</Notice> : null}
      {!items && !error ? <Loading label={t("Loading referrals…", "Загружаем рефералов…")} /> : null}
      {items ? (
        items.length === 0 ? (
          <div className="card">
            <EmptyState title={t("No referrals yet", "Пока нет рефералов")}>
              {t(
                "Share your referral link — every user who registers through it appears here.",
                "Поделитесь реферальной ссылкой — каждый пользователь, зарегистрировавшийся по ней, появится здесь.",
              )}
            </EmptyState>
          </div>
        ) : (
          <Table
            head={
              <>
                <th>{t("User", "Пользователь")}</th>
                <th>{t("Plan", "Тип")}</th>
                <th className="num">{t("Discount", "Скидка")}</th>
                <th className="num">{t("Balance", "Баланс")}</th>
                <th className="num">{t("Top-ups", "Пополнения")}</th>
                <th className="num">{t("Spend", "Траты")}</th>
                <th className="num">{t("You earned", "Вы заработали")}</th>
                {canSetDiscount ? <th /> : null}
              </>
            }
          >
            {items.map((r) => {
              const isPartner = (r.referralFloorBps ?? 0) > 0;
              const editable = canSetDiscount && r.userRef != null && r.customerType === "b2c";
              const editing = editable && editingRef === r.userRef;
              return (
                <tr key={`${r.userMask}-${r.attributedAt}`}>
                  <td className="mono">
                    {r.userMask}
                    <div style={{ color: "var(--text-dim)", fontSize: "12px", marginTop: "2px" }}>
                      {t("joined", "с")} {formatDate(r.attributedAt)}
                    </div>
                  </td>
                  <td>
                    {r.customerType === "b2b" ? (
                      <Badge tone="green">B2B</Badge>
                    ) : r.customerType === "b2c" ? (
                      isPartner ? (
                        <Badge tone="yellow">{t("B2C · partner rate", "B2C · партнёрская")}</Badge>
                      ) : (
                        <Badge tone="neutral">B2C</Badge>
                      )
                    ) : (
                      <span style={{ color: "var(--text-dim)" }}>—</span>
                    )}
                    {r.status && r.status !== "active" ? (
                      <div style={{ marginTop: "4px" }}>
                        <StatusBadge status={r.status} />
                      </div>
                    ) : null}
                  </td>
                  <td className="num">
                    {r.discountPercent != null ? `${r.discountPercent}%` : <span style={{ color: "var(--text-dim)" }}>—</span>}
                  </td>
                  <td className="num">
                    {r.balanceNano != null ? formatUsd(r.balanceNano) : <span style={{ color: "var(--text-dim)" }}>—</span>}
                  </td>
                  <td className="num">{formatUsd(r.topupNano)}</td>
                  <td className="num">{formatUsd(r.spendNano)}</td>
                  <td className="num" style={{ color: "var(--accent-strong)", fontWeight: 700 }}>
                    {formatUsd(r.earnedNano)}
                  </td>
                  {canSetDiscount ? (
                    <td style={{ whiteSpace: "nowrap" }}>
                      {editing ? (
                        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                          <div style={{ width: 72 }}>
                            <Input
                              value={pct}
                              onChange={(e) => setPct(e.target.value.replace(/[^\d.]/g, ""))}
                              inputMode="decimal"
                              placeholder={`≤${maxPct}`}
                              autoFocus
                            />
                          </div>
                          <Button onClick={() => void saveDiscount(r.userRef!)} loading={busy}>
                            {t("Save", "Сохранить")}
                          </Button>
                          <button
                            type="button"
                            onClick={() => setEditingRef(null)}
                            disabled={busy}
                            style={{ background: "none", border: "none", color: "var(--text-dim)", cursor: "pointer", fontSize: 13 }}
                          >
                            {t("Cancel", "Отмена")}
                          </button>
                          {editError ? (
                            <span style={{ color: "var(--danger, #c0392b)", fontSize: 12 }}>{editError}</span>
                          ) : null}
                        </div>
                      ) : editable ? (
                        <button
                          type="button"
                          onClick={() => openEditor(r)}
                          style={{ background: "none", border: "1px solid var(--border)", borderRadius: 6, padding: "4px 10px", color: "var(--accent-strong)", cursor: "pointer", fontSize: 13 }}
                        >
                          {isPartner ? t("Change rate", "Изменить ставку") : t("Set rate", "Ставка")}
                        </button>
                      ) : null}
                    </td>
                  ) : null}
                </tr>
              );
            })}
          </Table>
        )
      ) : null}
    </>
  );
}
