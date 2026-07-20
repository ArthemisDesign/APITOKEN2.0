"use client";

import { useCallback, useEffect, useState } from "react";
import { api, ApiError, formatBps } from "@/lib/api";
import { useI18n } from "./i18n";
import { Badge, Button, Card, CopyButton, Field, Input, Notice } from "./ui";

type DiscountLink = {
  id: string;
  code: string;
  url: string;
  discountBps: number;
  note: string | null;
  consumed: boolean;
  consumedAt: string | null;
  createdAt: string;
};

// Персональные ОДНОРАЗОВЫЕ ссылки со скидкой (Part B). Обычная реф-ссылка ведёт по обычным b2c-тирам
// без скидки; здесь партнёр (с правом) выпускает ссылку под конкретного клиента со спец-скидкой,
// которая гаснет первым же привязанным пользователем. Показывается только если право выдано.
export function ReferralDiscountCard({ enabled, currentBps }: { enabled: boolean; currentBps: number }) {
  const { t } = useI18n();
  const [pct, setPct] = useState("");
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [links, setLinks] = useState<DiscountLink[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const maxPct = currentBps / 100;

  const load = useCallback(async () => {
    try {
      const res = await api<{ items: DiscountLink[] }>("/v1/partner/discount-links");
      setLinks(res.items);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Could not load links.", "Не удалось загрузить ссылки."));
    }
  }, [t]);

  useEffect(() => { if (enabled) void load(); }, [enabled, load]);
  if (!enabled) return null;

  async function issue() {
    const value = Number(pct.trim());
    if (!Number.isFinite(value) || value <= 0 || value > maxPct) {
      setError(t(`Enter a discount between 1 and ${maxPct}%.`, `Введите скидку от 1 до ${maxPct}%.`));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await api("/v1/partner/discount-links", {
        method: "POST",
        body: { discountBps: Math.round(value * 100), ...(note.trim() ? { note: note.trim() } : {}) },
      });
      setPct("");
      setNote("");
      await load();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Could not issue link.", "Не удалось выпустить ссылку."));
    } finally {
      setBusy(false);
    }
  }

  async function remove(id: string) {
    setBusy(true);
    setError(null);
    try {
      await api(`/v1/partner/discount-links/${id}`, { method: "DELETE" });
      await load();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Could not delete.", "Не удалось удалить."));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card
      title={t("Personal discount links", "Персональные ссылки со скидкой")}
      sub={t(
        `Your normal referral link gives no discount — those referrals go on the usual tiers (you still earn commission on their real spend). Here you issue a one-time personal link with a discount (up to ${maxPct}%) for a specific client. The discount goes to the first person who signs up with it, as a price floor.`,
        `Ваша обычная реф-ссылка скидку не даёт — такие рефералы идут по обычным тирам (комиссию с их реальных трат вы всё равно получаете). Здесь вы выпускаете одноразовую персональную ссылку со скидкой (до ${maxPct}%) под конкретного клиента. Скидка достаётся первому, кто по ней зарегистрируется, и работает как «пол» цены.`,
      )}
    >
      {error ? <Notice kind="error">{error}</Notice> : null}
      <div style={{ display: "grid", gridTemplateColumns: "120px 1fr", gap: 12 }}>
        <Field label={t("Discount %", "Скидка %")} hint={t(`1–${maxPct}%`, `1–${maxPct}%`)}>
          <Input value={pct} onChange={(e) => setPct(e.target.value.replace(/[^\d.]/g, ""))} inputMode="decimal" placeholder="—" />
        </Field>
        <Field label={t("For whom (optional)", "Для кого (необязательно)")} hint={t("A note so you remember", "Заметка, чтобы не путаться")}>
          <Input value={note} onChange={(e) => setNote(e.target.value)} placeholder={t("e.g. client Acme", "напр. клиент Acme")} maxLength={120} />
        </Field>
      </div>
      <div style={{ marginTop: 12 }}>
        <Button onClick={issue} loading={busy}>
          {t("Issue link", "Выпустить ссылку")}
        </Button>
      </div>

      {links && links.length > 0 ? (
        <div className="stack" style={{ gap: 10, marginTop: 16 }}>
          {links.map((l) => (
            <div key={l.id} style={{ border: "1px solid var(--border)", borderRadius: 8, padding: "10px 12px" }}>
              <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 6, flexWrap: "wrap" }}>
                <strong>{formatBps(l.discountBps)}</strong>
                {l.note ? <span style={{ fontSize: 13, color: "var(--text-faint)" }}>· {l.note}</span> : null}
                {l.consumed
                  ? <Badge tone="green">{t("Used", "Использована")}</Badge>
                  : <Badge tone="yellow">{t("Unused", "Свободна")}</Badge>}
                <button
                  type="button"
                  onClick={() => remove(l.id)}
                  disabled={busy}
                  style={{ marginLeft: "auto", background: "none", border: "none", color: "var(--danger, #c0392b)", cursor: "pointer", fontSize: 13 }}
                >
                  {t("Delete", "Удалить")}
                </button>
              </div>
              <div className="reflink-row">
                <Input readOnly value={l.url} onFocus={(e) => e.currentTarget.select()} />
                {!l.consumed ? <CopyButton value={l.url} label={t("Copy", "Копировать")} /> : null}
              </div>
            </div>
          ))}
        </div>
      ) : (
        <p className="field-hint" style={{ marginTop: 12 }}>{t("No personal links yet.", "Пока нет персональных ссылок.")}</p>
      )}
    </Card>
  );
}
