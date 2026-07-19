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

function dateLabel(iso: string): string {
  return new Date(iso).toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" });
}

function periodRange(startIso: string, endIso: string): string {
  const start = new Date(startIso);
  const endInclusive = new Date(new Date(endIso).getTime() - 86_400_000);
  const sameMonth = start.getUTCMonth() === endInclusive.getUTCMonth();
  const s = start.toLocaleDateString("en-US", { month: "short", day: "numeric", timeZone: "UTC" });
  const e = endInclusive.toLocaleDateString("en-US", {
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

const PHASE_BADGE: Record<string, { tone: "neutral" | "green" | "yellow"; label: string }> = {
  accruing: { tone: "neutral", label: "Accruing" },
  locked: { tone: "yellow", label: "Locked" },
  payable: { tone: "green", label: "Paying out" },
  closed: { tone: "neutral", label: "Closed" },
};

function WalletCard({ wallet, onBound }: { wallet: string | null; onBound: (address: string) => void }) {
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
      setError("That doesn't look like a BSC address — expected 0x followed by 40 hex characters.");
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
      setError(err instanceof ApiError ? err.message : "Could not save the wallet. Try again.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card
      title="Payout wallet"
      sub="USDT (BEP-20) on BNB Smart Chain — the only supported network. Every payout is sent to this address."
    >
      {error ? <Notice kind="error">{error}</Notice> : null}
      {saved && !editing ? <Notice kind="success">Wallet saved.</Notice> : null}
      {!wallet && !editing ? (
        <Notice kind="info">Bind your BSC wallet to receive payouts — without it, your balance rolls over.</Notice>
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
            Change wallet
          </Button>
        </div>
      ) : (
        <form onSubmit={bind}>
          <Field label="BSC wallet address" hint="Double-check it — on-chain payouts cannot be reversed.">
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
              {wallet ? "Save new wallet" : "Bind wallet"}
            </Button>
            {wallet ? (
              <Button type="button" variant="ghost" onClick={() => setEditing(false)} disabled={busy}>
                Cancel
              </Button>
            ) : null}
          </div>
        </form>
      )}
    </Card>
  );
}

export default function PayoutsPage() {
  const partner = usePartner();
  const [wallet, setWallet] = useState<string | null>(walletFromPartner(partner));
  const [state, setState] = useState<PeriodState | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setState(await api<PeriodState>("/v1/partner/periods"));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Failed to load payouts.");
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
      <h1 className="page-title">Payouts</h1>
      <p className="page-sub">
        Automatic twice-monthly payouts in USDT (BEP-20) on BNB Smart Chain. No manual withdrawal —
        just keep your wallet bound.
      </p>
      {error ? <Notice kind="error">{error}</Notice> : null}

      <div className="stack">
        <div className="stat-grid" style={{ gridTemplateColumns: "repeat(4, 1fr)" }}>
          <div className="stat-card">
            <div className="stat-label">This period</div>
            <div className="stat-value green">{state ? formatUsd(state.current.accruedNano) : "…"}</div>
            <div className="stat-foot">
              {state ? `accruing · ${periodRange(state.current.start, state.current.end)}` : ""}
            </div>
          </div>
          <div className="stat-card">
            <div className="stat-label">Locked</div>
            <div className="stat-value">{state ? formatUsd(lockedTotal) : "…"}</div>
            <div className="stat-foot">
              {state && state.locked[0]
                ? `unlocks ${dateLabel(state.locked[0].unlocksAt)}`
                : "7-day hold after a period ends"}
            </div>
          </div>
          <div className="stat-card">
            <div className="stat-label">Next payout</div>
            <div className="stat-value">{state ? formatUsd(state.nextPayout.estimatedNano) : "…"}</div>
            <div className="stat-foot">
              {state
                ? `${dateLabel(state.nextPayout.date)} · in ${daysUntil(state.nextPayout.date, state.now)}d`
                : ""}
            </div>
          </div>
          <div className="stat-card">
            <div className="stat-label">Unpaid total</div>
            <div className="stat-value">{state ? formatUsd(state.unpaidNano) : "…"}</div>
            <div className="stat-foot">{state ? `${formatUsd(state.lifetimePaidNano)} paid to date` : ""}</div>
          </div>
        </div>

        <WalletCard wallet={wallet} onBound={setWallet} />

        <Card title="How payouts work">
          <ul className="how-list">
            <li>
              Earnings are counted in two periods each month: the <strong>1st–15th</strong> and the{" "}
              <strong>16th–end</strong>.
            </li>
            <li>
              After a period ends there is a <strong>7-day lock</strong>, then payouts are sent within
              the next <strong>3 days</strong> — if you have a bound wallet and your balance is at least{" "}
              <strong>{state ? formatUsd(state.minPayoutNano) : "the minimum"}</strong>.
            </li>
            <li>
              Below the minimum or no wallet? Nothing is lost — the amount rolls into your next payout
              (so it may cover two periods at once).
            </li>
            {!wallet ? <li><strong>Bind your BSC wallet above to start receiving payouts.</strong></li> : null}
          </ul>
        </Card>

        <Card title="Period history" sub="What you earned in each half-month period and when it pays out.">
          {!state ? (
            <Loading />
          ) : state.history.length === 0 ? (
            <EmptyState title="No earnings yet">
              Your period earnings will appear here once your referrals start spending.
            </EmptyState>
          ) : (
            <Table
              head={
                <>
                  <th>Period</th>
                  <th className="num">Earned</th>
                  <th>Status</th>
                  <th>Payout date</th>
                </>
              }
            >
              {state.history.map((row) => {
                const badge = PHASE_BADGE[row.phase] ?? PHASE_BADGE.closed;
                return (
                  <tr key={row.key}>
                    <td>{periodRange(row.start, row.end)}</td>
                    <td className="num" style={{ fontWeight: 700 }}>{formatUsd(row.earnedNano)}</td>
                    <td><Badge tone={badge!.tone}>{badge!.label}</Badge></td>
                    <td>{dateLabel(row.payoutDate)}</td>
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
