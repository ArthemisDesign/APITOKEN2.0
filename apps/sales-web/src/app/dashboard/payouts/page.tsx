"use client";

// Выплаты: единственная сеть — BSC (BNB Smart Chain), актив USDT (BEP-20).
// Партнёр привязывает кошелёк, все выплаты уходят только на него.

import { useCallback, useEffect, useState, type FormEvent } from "react";
import {
  api,
  ApiError,
  formatDate,
  formatUsd,
  usdToNano,
  type Overview,
  type Partner,
  type PayoutRow,
} from "@/lib/api";
import { usePartner } from "@/components/partner-context";
import {
  Button,
  Card,
  EmptyState,
  Field,
  Input,
  Loading,
  Notice,
  StatusBadge,
  Table,
} from "@/components/ui";

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
      sub="USDT (BEP-20) on BNB Smart Chain — the only supported network. Every payout goes to this address."
    >
      {error ? <Notice kind="error">{error}</Notice> : null}
      {saved && !editing ? <Notice kind="success">Wallet saved.</Notice> : null}

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
          <Field
            label="BSC wallet address"
            hint="Double-check the address — payouts sent on-chain cannot be reversed."
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
  const [overview, setOverview] = useState<Overview | null>(null);
  const [payouts, setPayouts] = useState<PayoutRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [amount, setAmount] = useState("");
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [ov, po] = await Promise.all([
        api<Overview>("/v1/partner/overview"),
        api<{ items: PayoutRow[] }>("/v1/partner/payouts"),
      ]);
      setOverview(ov);
      setPayouts([...po.items].sort((a, b) => (a.requestedAt < b.requestedAt ? 1 : -1)));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Failed to load payouts.");
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setFormError(null);
    setSuccess(null);
    const amountNano = usdToNano(amount);
    if (!amountNano) {
      setFormError("Enter a valid positive USD amount, e.g. 100 or 49.50.");
      return;
    }
    setBusy(true);
    try {
      await api("/v1/partner/payouts", { method: "POST", body: { amountNano } });
      setSuccess("Payout requested. The program team will review it shortly.");
      setAmount("");
      void load();
    } catch (err) {
      setFormError(err instanceof ApiError ? err.message : "Could not request the payout.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <h1 className="page-title">Payouts</h1>
      <p className="page-sub">Withdraw your available balance in USDT (BEP-20) on BNB Smart Chain.</p>
      {error ? <Notice kind="error">{error}</Notice> : null}

      <div className="stack">
        <div className="stat-grid" style={{ gridTemplateColumns: "repeat(3, 1fr)" }}>
          <div className="stat-card">
            <div className="stat-label">Available</div>
            <div className="stat-value green">
              {overview ? formatUsd(overview.totals.availableNano) : "…"}
            </div>
          </div>
          <div className="stat-card">
            <div className="stat-label">Pending payout</div>
            <div className="stat-value">
              {overview ? formatUsd(overview.totals.pendingPayoutNano) : "…"}
            </div>
          </div>
          <div className="stat-card">
            <div className="stat-label">Paid out to date</div>
            <div className="stat-value">
              {overview ? formatUsd(overview.totals.paidNano) : "…"}
            </div>
          </div>
        </div>

        <WalletCard wallet={wallet} onBound={setWallet} />

        <Card title="Request a payout" sub="Requests are reviewed manually by the program team.">
          {formError ? <Notice kind="error">{formError}</Notice> : null}
          {success ? <Notice kind="success">{success}</Notice> : null}
          {!wallet ? (
            <Notice kind="info">Bind your BSC wallet above to request payouts.</Notice>
          ) : (
            <form onSubmit={onSubmit}>
              <div className="grid-2">
                <Field
                  label="Amount (USD)"
                  hint={overview ? `Available: ${formatUsd(overview.totals.availableNano)}` : undefined}
                >
                  <Input
                    inputMode="decimal"
                    placeholder="100"
                    value={amount}
                    onChange={(e) => setAmount(e.target.value)}
                    required
                  />
                </Field>
                <Field label="Destination" hint="USDT (BEP-20), BNB Smart Chain">
                  <Input className="mono" value={shortAddress(wallet)} readOnly disabled />
                </Field>
              </div>
              <Button type="submit" loading={busy}>
                Request payout
              </Button>
            </form>
          )}
        </Card>

        <Card title="Payout history">
          {!payouts ? (
            <Loading />
          ) : payouts.length === 0 ? (
            <EmptyState title="No payouts yet">
              Your requests and their status will appear here.
            </EmptyState>
          ) : (
            <Table
              head={
                <>
                  <th>Requested</th>
                  <th className="num">Amount</th>
                  <th>Destination</th>
                  <th>Status</th>
                  <th>Paid</th>
                </>
              }
            >
              {payouts.map((p) => (
                <tr key={p.id}>
                  <td>{formatDate(p.requestedAt)}</td>
                  <td className="num" style={{ fontWeight: 700 }}>
                    {formatUsd(p.amountNano)}
                  </td>
                  <td className="mono">
                    {p.details && typeof p.details === "object" && "address" in p.details &&
                    typeof (p.details as { address?: unknown }).address === "string"
                      ? `BSC · ${shortAddress((p.details as { address: string }).address)}`
                      : p.method}
                  </td>
                  <td>
                    <StatusBadge status={p.status} />
                  </td>
                  <td>{p.paidAt ? formatDate(p.paidAt) : "—"}</td>
                </tr>
              ))}
            </Table>
          )}
        </Card>
      </div>
    </>
  );
}
