"use client";

import { useCallback, useEffect, useState, type FormEvent } from "react";
import {
  api,
  ApiError,
  formatDate,
  formatUsd,
  usdToNano,
  type Overview,
  type PayoutRow,
} from "@/lib/api";
import {
  Button,
  Card,
  EmptyState,
  Field,
  Input,
  Loading,
  Notice,
  Select,
  StatusBadge,
  Table,
  Textarea,
} from "@/components/ui";

const METHODS = [
  { value: "usdt-trc20", label: "USDT (TRC-20)" },
  { value: "card", label: "Card" },
  { value: "other", label: "Other (describe in details)" },
];

export default function PayoutsPage() {
  const [overview, setOverview] = useState<Overview | null>(null);
  const [payouts, setPayouts] = useState<PayoutRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [amount, setAmount] = useState("");
  const [method, setMethod] = useState(METHODS[0].value);
  const [details, setDetails] = useState("");
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
      setPayouts(
        [...po.items].sort((a, b) => (a.requestedAt < b.requestedAt ? 1 : -1)),
      );
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
    if (!details.trim()) {
      setFormError("Add payout details (wallet address, card number, etc.).");
      return;
    }
    setBusy(true);
    try {
      await api("/v1/partner/payouts", {
        method: "POST",
        body: { amountNano, method, details: details.trim() },
      });
      setSuccess("Payout requested. The program team will review it shortly.");
      setAmount("");
      setDetails("");
      void load();
    } catch (err) {
      setFormError(
        err instanceof ApiError ? err.message : "Could not request the payout.",
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <h1 className="page-title">Payouts</h1>
      <p className="page-sub">Withdraw your available partner balance.</p>
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

        <Card title="Request a payout" sub="Requests are reviewed manually by the program team.">
          {formError ? <Notice kind="error">{formError}</Notice> : null}
          {success ? <Notice kind="success">{success}</Notice> : null}
          <form onSubmit={onSubmit}>
            <div className="grid-2">
              <Field
                label="Amount (USD)"
                hint={
                  overview
                    ? `Available: ${formatUsd(overview.totals.availableNano)}`
                    : undefined
                }
              >
                <Input
                  inputMode="decimal"
                  placeholder="100"
                  value={amount}
                  onChange={(e) => setAmount(e.target.value)}
                  required
                />
              </Field>
              <Field label="Method">
                <Select value={method} onChange={(e) => setMethod(e.target.value)}>
                  {METHODS.map((m) => (
                    <option key={m.value} value={m.value}>
                      {m.label}
                    </option>
                  ))}
                </Select>
              </Field>
            </div>
            <Field
              label="Payout details"
              hint="TRC-20 wallet address, card number, or other instructions."
            >
              <Textarea
                value={details}
                onChange={(e) => setDetails(e.target.value)}
                placeholder="e.g. TXk3…9fA (USDT TRC-20)"
                required
              />
            </Field>
            <Button type="submit" loading={busy}>
              Request payout
            </Button>
          </form>
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
                  <th>Method</th>
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
                  <td>{p.method}</td>
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
