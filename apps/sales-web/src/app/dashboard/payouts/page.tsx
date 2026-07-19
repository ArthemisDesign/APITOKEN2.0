"use client";

// Периодная модель выплат (USDT BEP-20 / BSC). Ручного запроса вывода нет — платим по
// расписанию на привязанный кошелёк. Партнёр видит текущий период, лок, дату следующей
// выплаты и историю по полумесячным периодам. Сама отправка — отдельная система.

import { useCallback, useEffect, useState, type FormEvent } from "react";
import {
  api,
  ApiError,
  formatUsd,
  type PeriodState,
  type Partner,
} from "@/lib/api";
import { usePartner } from "@/components/partner-context";
import { Badge, Button, Card, EmptyState, Field, Input, Loading, Notice, Table } from "@/components/ui";
import { localeFor, useI18n, type Lang } from "@/components/i18n";
import { PayoutTimeline } from "@/components/payout-timeline";

const BSC_ADDRESS = /^0x[a-fA-F0-9]{40}$/;

function walletFromPartner(partner: Partner): string | null {
  const details = partner.payoutDetails;
  if (details && typeof details === "object" && "address" in details) {
    const address = (details as { address?: unknown }).address;
    if (typeof address === "string" && BSC_ADDRESS.test(address)) return address;
  }
  return null;
}

function shortAddress(address: string): string {
  return `${address.slice(0, 6)}…${address.slice(-4)}`;
}

function dateLabel(iso: string, lang: Lang): string {
  return new Date(iso).toLocaleDateString(localeFor(lang), { month: "short", day: "numeric", year: "numeric" });
}

function periodRange(startIso: string, endIso: string, lang: Lang): string {
  const locale = localeFor(lang);
  const start = new Date(startIso);
  const endInclusive = new Date(new Date(endIso).getTime() - 86_400_000);
  const sameMonth = start.getUTCMonth() === endInclusive.getUTCMonth();
  const s = start.toLocaleDateString(locale, { month: "short", day: "numeric", timeZone: "UTC" });
  const e = endInclusive.toLocaleDateString(locale, {
    month: sameMonth ? undefined : "short",
    day: "numeric",
    year: "numeric",
    timeZone: "UTC",
  });
  return `${s} – ${e}`;
}

function daysUntil(iso: string, nowIso: string): number {
  return Math.max(0, Math.ceil((new Date(iso).getTime() - new Date(nowIso).getTime()) / 86_400_000));
}

const PHASE_BADGE: Record<string, { tone: "neutral" | "green" | "yellow"; en: string; ru: string }> = {
  accruing: { tone: "neutral", en: "Accruing", ru: "Начисляется" },
  locked: { tone: "yellow", en: "Locked", ru: "Заблокировано" },
  payable: { tone: "green", en: "Paying out", ru: "Выплачивается" },
  closed: { tone: "neutral", en: "Closed", ru: "Закрыто" },
};

function WalletCard({ wallet, onBound }: { wallet: string | null; onBound: (address: string) => void }) {
  const { t } = useI18n();
  const [editing, setEditing] = useState(wallet === null);
  const [address, setAddress] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  async function bind(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setSaved(false);
    const clean = address.trim();
    if (!BSC_ADDRESS.test(clean)) {
      setError(
        t(
          "That doesn't look like a BSC address — expected 0x followed by 40 hex characters.",
          "Это не похоже на адрес BSC — ожидается 0x и 40 шестнадцатеричных символов.",
        ),
      );
      return;
    }
    setBusy(true);
    try {
      await api("/v1/partner/wallet", { method: "PATCH", body: { address: clean } });
      onBound(clean);
      setEditing(false);
      setAddress("");
      setSaved(true);
    } catch (err) {
      setError(
        err instanceof ApiError
          ? err.message
          : t("Could not save the wallet. Try again.", "Не удалось сохранить кошелёк. Попробуйте ещё раз."),
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card
      title={t("Payout wallet", "Кошелёк для выплат")}
      sub={t(
        "USDT (BEP-20) on BNB Smart Chain — the only supported network. Every payout is sent to this address.",
        "USDT (BEP-20) в сети BNB Smart Chain — единственная поддерживаемая сеть. Каждая выплата отправляется на этот адрес.",
      )}
    >
      {error ? <Notice kind="error">{error}</Notice> : null}
      {saved && !editing ? <Notice kind="success">{t("Wallet saved.", "Кошелёк сохранён.")}</Notice> : null}
      {!wallet && !editing ? (
        <Notice kind="info">
          {t(
            "Bind your BSC wallet to receive payouts — without it, your balance rolls over.",
            "Привяжите кошелёк BSC, чтобы получать выплаты — без него баланс переносится на следующий период.",
          )}
        </Notice>
      ) : null}

      {wallet && !editing ? (
        <div className="wallet-row">
          <div className="wallet-chip">
            <span className="wallet-net">BSC · USDT BEP-20</span>
            <span className="mono wallet-addr" title={wallet}>
              {shortAddress(wallet)}
            </span>
          </div>
          <Button variant="ghost" size="sm" onClick={() => setEditing(true)}>
            {t("Change wallet", "Изменить кошелёк")}
          </Button>
        </div>
      ) : (
        <form onSubmit={bind}>
          <Field
            label={t("BSC wallet address", "Адрес кошелька BSC")}
            hint={t(
              "Double-check it — on-chain payouts cannot be reversed.",
              "Перепроверьте — выплаты в блокчейне нельзя отменить.",
            )}
          >
            <Input
              className="mono"
              value={address}
              onChange={(e) => setAddress(e.target.value)}
              placeholder="0x0000000000000000000000000000000000000000"
              spellCheck={false}
              autoComplete="off"
              required
            />
          </Field>
          <div className="row-actions">
            <Button type="submit" loading={busy}>
              {wallet ? t("Save new wallet", "Сохранить новый кошелёк") : t("Bind wallet", "Привязать кошелёк")}
            </Button>
            {wallet ? (
              <Button type="button" variant="ghost" onClick={() => setEditing(false)} disabled={busy}>
                {t("Cancel", "Отмена")}
              </Button>
            ) : null}
          </div>
        </form>
      )}
    </Card>
  );
}

export default function PayoutsPage() {
  const { t, lang } = useI18n();
  const partner = usePartner();
  const [wallet, setWallet] = useState<string | null>(walletFromPartner(partner));
  const [state, setState] = useState<PeriodState | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setState(await api<PeriodState>("/v1/partner/periods"));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Failed to load payouts.", "Не удалось загрузить выплаты."));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const lockedTotal = state
    ? state.locked.reduce((acc, l) => acc + BigInt(l.earnedNano), 0n).toString()
    : "0";

  return (
    <>
      <h1 className="page-title">{t("Payouts", "Выплаты")}</h1>
      <p className="page-sub">
        {t(
          "Automatic twice-monthly payouts in USDT (BEP-20) on BNB Smart Chain. No manual withdrawal — just keep your wallet bound.",
          "Автоматические выплаты дважды в месяц в USDT (BEP-20) в сети BNB Smart Chain. Ручной вывод не нужен — просто держите кошелёк привязанным.",
        )}
      </p>
      {error ? <Notice kind="error">{error}</Notice> : null}

      <div className="stack">
        {state ? <PayoutTimeline state={state} /> : null}
        <div className="stat-grid" style={{ gridTemplateColumns: "repeat(4, 1fr)" }}>
          <div className="stat-card">
            <div className="stat-label">{t("This period", "Текущий период")}</div>
            <div className="stat-value green">{state ? formatUsd(state.current.accruedNano) : "…"}</div>
            <div className="stat-foot">
              {state
                ? `${t("accruing", "начисляется")} · ${periodRange(state.current.start, state.current.end, lang)}`
                : ""}
            </div>
          </div>
          <div className="stat-card">
            <div className="stat-label">{t("Locked", "Заблокировано")}</div>
            <div className="stat-value">{state ? formatUsd(lockedTotal) : "…"}</div>
            <div className="stat-foot">
              {state && state.locked[0]
                ? `${t("unlocks", "разблокируется")} ${dateLabel(state.locked[0].unlocksAt, lang)}`
                : t("7-day hold after a period ends", "удержание 7 дней после окончания периода")}
            </div>
          </div>
          <div className="stat-card">
            <div className="stat-label">{t("Next payout", "Следующая выплата")}</div>
            <div className="stat-value">{state ? formatUsd(state.nextPayout.estimatedNano) : "…"}</div>
            <div className="stat-foot">
              {state
                ? `${dateLabel(state.nextPayout.date, lang)} · ${t("in", "через")} ${daysUntil(state.nextPayout.date, state.now)}${t("d", " дн.")}`
                : ""}
            </div>
          </div>
          <div className="stat-card">
            <div className="stat-label">{t("Unpaid total", "Всего не выплачено")}</div>
            <div className="stat-value">{state ? formatUsd(state.unpaidNano) : "…"}</div>
            <div className="stat-foot">
              {state ? `${formatUsd(state.lifetimePaidNano)} ${t("paid to date", "выплачено на данный момент")}` : ""}
            </div>
          </div>
        </div>

        <WalletCard wallet={wallet} onBound={setWallet} />

        <Card title={t("How payouts work", "Как работают выплаты")}>
          <ul className="how-list">
            <li>
              {t("Earnings are counted in two periods each month: the ", "Заработок учитывается в двух периодах каждый месяц: ")}
              <strong>{t("1st–15th", "с 1-го по 15-е")}</strong>
              {t(" and the ", " и ")}
              <strong>{t("16th–last day of the month", "с 16-го по последний день месяца")}</strong>.
            </li>
            <li>
              {t("After a period ends there is a ", "После окончания периода действует ")}
              <strong>{t("7-day lock", "7-дневная блокировка")}</strong>
              {t(
                ", then any earnings are sent to your wallet within the next ",
                ", затем весь заработок отправляется на ваш кошелёк в течение следующих ",
              )}
              <strong>{t("3 days", "3 дней")}</strong>.
            </li>
            <li>
              {t(
                "No wallet bound yet? Nothing is lost — your balance rolls into the next payout (so it may cover two periods at once).",
                "Кошелёк ещё не привязан? Ничего не теряется — баланс переносится на следующую выплату (так что она может покрыть сразу два периода).",
              )}
            </li>
            {!wallet ? (
              <li>
                <strong>
                  {t(
                    "Bind your BSC wallet above to start receiving payouts.",
                    "Привяжите кошелёк BSC выше, чтобы начать получать выплаты.",
                  )}
                </strong>
              </li>
            ) : null}
          </ul>
        </Card>

        <Card
          title={t("Period history", "История периодов")}
          sub={t(
            "What you earned in each half-month period and when it pays out.",
            "Сколько вы заработали в каждом полумесячном периоде и когда это выплачивается.",
          )}
        >
          {!state ? (
            <Loading />
          ) : state.history.length === 0 ? (
            <EmptyState title={t("No earnings yet", "Пока нет заработка")}>
              {t(
                "Your period earnings will appear here once your referrals start spending.",
                "Ваш заработок по периодам появится здесь, как только рефералы начнут тратить.",
              )}
            </EmptyState>
          ) : (
            <Table
              head={
                <>
                  <th>{t("Period", "Период")}</th>
                  <th className="num">{t("Earned", "Заработано")}</th>
                  <th>{t("Status", "Статус")}</th>
                  <th>{t("Payout date", "Дата выплаты")}</th>
                </>
              }
            >
              {state.history.map((row) => {
                const badge = PHASE_BADGE[row.phase] ?? PHASE_BADGE.closed;
                return (
                  <tr key={row.key}>
                    <td>{periodRange(row.start, row.end, lang)}</td>
                    <td className="num" style={{ fontWeight: 700 }}>{formatUsd(row.earnedNano)}</td>
                    <td><Badge tone={badge!.tone}>{t(badge!.en, badge!.ru)}</Badge></td>
                    <td>{dateLabel(row.payoutDate, lang)}</td>
                  </tr>
                );
              })}
            </Table>
          )}
        </Card>
      </div>
    </>
  );
}
