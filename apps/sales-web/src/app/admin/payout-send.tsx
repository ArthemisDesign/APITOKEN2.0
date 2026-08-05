"use client";

import { useCallback, useEffect, useState } from "react";
import {
  api,
  ApiError,
  formatDate,
  formatUsd,
  type PayoutBatchDto,
  type PayoutEngine,
  type PayoutReportDto,
  type PayoutRowDto,
} from "@/lib/api";
import { Badge, Button, Card, EmptyState, Loading, Notice } from "@/components/ui";

function adminHeaders(key: string): Record<string, string> {
  return key ? { "x-sales-admin-key": key } : {};
}

const faint = { color: "var(--text-faint)" } as const;
const mono = { fontFamily: "var(--font-mono, monospace)", fontSize: 12 } as const;

function shortAddr(a: string | null): string {
  if (!a) return "—";
  return a.length > 14 ? `${a.slice(0, 8)}…${a.slice(-6)}` : a;
}
function bnb(wei: string | null): string {
  if (!wei || !/^\d+$/.test(wei)) return "—";
  const v = BigInt(wei);
  const whole = v / 10n ** 18n;
  const frac = ((v % 10n ** 18n) / 10n ** 14n).toString().padStart(4, "0");
  return `${whole}.${frac} BNB`;
}
function fmtWindow(w: PayoutEngine["window"]): string {
  if (w.enforced === false) return "gate OFF (test mode) — sending allowed anytime";
  if (w.open) return `open until ${formatDate(w.closesAt)}`;
  return "closed — sending is disabled outside the 3-day payout window";
}

function chainBadge(row: PayoutRowDto): React.ReactNode {
  const s = row.status === "paid" ? "paid" : row.chainStatus ?? "pending";
  const tone = s === "paid" ? "green" : s === "failed" ? "red" : s === "broadcast" ? "yellow" : undefined;
  return <Badge tone={tone as "green" | "red" | "yellow" | undefined}>{s}</Badge>;
}

export function PayoutSendTab({ adminKey }: { adminKey: string }) {
  const [engine, setEngine] = useState<PayoutEngine | null>(null);
  const [batches, setBatches] = useState<PayoutBatchDto[] | null>(null);
  const [report, setReport] = useState<PayoutReportDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const loadEngine = useCallback(async () => {
    try {
      setEngine(await api<PayoutEngine>("/v1/admin/payouts/engine", { headers: adminHeaders(adminKey) }));
      const res = await api<{ items: PayoutBatchDto[] }>("/v1/admin/payouts/batches", { headers: adminHeaders(adminKey) });
      setBatches(res.items);
      // auto-open the newest non-terminal batch
      const active = res.items.find((b) => b.status === "prepared" || b.status === "sending");
      if (active) await openBatch(active.id);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Failed to load payout engine.");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [adminKey]);

  const openBatch = useCallback(async (id: string) => {
    try {
      setReport(await api<PayoutReportDto>(`/v1/admin/payouts/batches/${id}`, { headers: adminHeaders(adminKey) }));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Failed to load batch.");
    }
  }, [adminKey]);

  useEffect(() => { void loadEngine(); }, [loadEngine]);

  // live progress while a batch is sending: fast right after visible movement,
  // backing off to a quiet cadence while the chain is quiet, fast again after
  // any change — instead of a fixed 4s beat that hammers the API while a long
  // batch just waits for confirmations
  useEffect(() => {
    if (report?.batch.status !== "sending") return;
    const batchId = report.batch.id;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let cancelled = false;
    let delay = 2_000;
    let lastFrame = "";

    const tick = async () => {
      try {
        const next = await api<PayoutReportDto>(`/v1/admin/payouts/batches/${batchId}`, { headers: adminHeaders(adminKey) });
        if (cancelled) return;
        const frame = JSON.stringify(next.rows.map((r) => [r.status, r.chainStatus, r.txHash]));
        delay = frame !== lastFrame ? 2_000 : Math.min(delay * 2, 15_000);
        lastFrame = frame;
        setReport(next);
      } catch {
        // transient API failure — keep the current report, retry on the slow cadence
        delay = Math.min(delay * 2, 15_000);
      }
      if (!cancelled) timer = setTimeout(() => void tick(), delay);
    };
    timer = setTimeout(() => void tick(), delay);
    return () => { cancelled = true; if (timer) clearTimeout(timer); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [report?.batch.status, report?.batch.id, adminKey]);

  async function act<T>(fn: () => Promise<T>): Promise<T | null> {
    setBusy(true); setError(null);
    try { return await fn(); }
    catch (err) { setError(err instanceof ApiError ? err.message : "Action failed."); return null; }
    finally { setBusy(false); }
  }

  async function prepare() {
    const r = await act(() => api<PayoutReportDto>("/v1/admin/payouts/prepare", { method: "POST", headers: adminHeaders(adminKey) }));
    if (r) { setReport(r); void loadEngine(); }
  }
  async function sendAll() {
    if (!report) return;
    if (!confirm(`Send ${report.rows.filter((r) => r.status !== "paid").length} payouts totalling ${formatUsd(report.chain.requiredUsdtNano)} from the hot wallet? This is irreversible.`)) return;
    const r = await act(() => api<PayoutReportDto>(`/v1/admin/payouts/batches/${report.batch.id}/send`, { method: "POST", headers: adminHeaders(adminKey) }));
    if (r) setReport(r);
    void loadEngine();
  }
  async function sendOne(row: PayoutRowDto) {
    if (!report) return;
    await act(() => api(`/v1/admin/payouts/rows/${row.id}/send`, { method: "POST", headers: adminHeaders(adminKey) }));
    await openBatch(report.batch.id);
  }
  async function releaseOne(row: PayoutRowDto) {
    if (!report || !confirm("Release this failed payout? Its balance rolls back into the partner's next window. Only do this if the transaction did NOT go on-chain.")) return;
    await act(() => api(`/v1/admin/payouts/rows/${row.id}/release`, { method: "POST", headers: adminHeaders(adminKey) }));
    await openBatch(report.batch.id);
  }
  async function cancel() {
    if (!report || !confirm("Cancel this prepared batch? Unsent rows are removed and balances freed.")) return;
    await act(() => api(`/v1/admin/payouts/batches/${report.batch.id}/cancel`, { method: "POST", headers: adminHeaders(adminKey) }));
    setReport(null); void loadEngine();
  }

  if (!engine || !batches) return error ? <Notice kind="error">{error}</Notice> : <Loading />;

  const windowOpen = engine.window.open;
  const b = report?.batch;
  const c = report?.chain;
  const canSend = Boolean(report && windowOpen && engine.configured && c?.sufficientUsdt !== false && c?.sufficientBnb !== false && (b?.status === "prepared" || b?.status === "sending"));

  return (
    <div className="stack">
      {error ? <Notice kind="error">{error}</Notice> : null}

      {/* Engine + window status */}
      <div className="stat-grid" style={{ gridTemplateColumns: "repeat(3, 1fr)" }}>
        <div className="stat-card">
          <div className="stat-label">Payout engine</div>
          <div className="stat-value" style={{ fontSize: 18, color: engine.configured ? "var(--accent-strong,#3b5bdb)" : "#d6455a" }}>
            {engine.configured ? "Configured" : "Not configured"}
          </div>
          <div className="stat-foot">USDT · BNB Chain (BEP-20)</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">Payout window</div>
          <div className="stat-value" style={{ fontSize: 18, color: windowOpen ? "#26a15e" : "#d69e2e" }}>{windowOpen ? "OPEN" : "CLOSED"}</div>
          <div className="stat-foot">{fmtWindow(engine.window)}</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">Hot wallet</div>
          <div className="stat-value" style={{ ...mono, fontSize: 15 }}>{shortAddr(c?.hotWalletAddress ?? null)}</div>
          <div className="stat-foot">{c?.usdtBalanceNano != null ? `${formatUsd(c.usdtBalanceNano)} USDT · ${bnb(c.bnbBalanceWei)}` : "balance shown after prepare"}</div>
        </div>
      </div>

      {!windowOpen ? (
        <Notice kind="info">
          Sending is <strong>physically disabled</strong> outside the two 3-day payout windows — the server rejects any send attempt now. You can still prepare and review a batch.
        </Notice>
      ) : null}

      {!report ? (
        <Card title="Prepare a payout run" sub="Collects every active partner with a valid BEP-20 address and unpaid balance > 0, validates addresses, and checks the hot wallet — nothing is sent yet.">
          <Button onClick={prepare} loading={busy} disabled={!engine.configured}>Prepare payout run</Button>
          {!engine.configured ? <div style={{ marginTop: 8, fontSize: 13, ...faint }}>Set the hot-wallet key + BlockRazor send RPC in server env to enable.</div> : null}
        </Card>
      ) : (
        <Card
          title={`Payout batch · ${b!.status}`}
          sub={`${b!.recipientCount} recipients · ${formatUsd(b!.totalNano)} total · gas ${b!.gasPriceGwei} gwei · prepared ${formatDate(b!.preparedAt)}`}
        >
          {/* balance sufficiency */}
          {c ? (
            <div style={{ display: "flex", gap: 16, flexWrap: "wrap", marginBottom: 12, fontSize: 13 }}>
              <span>Needs <strong>{formatUsd(c.requiredUsdtNano)}</strong> USDT {c.sufficientUsdt === false ? <Badge tone="red">insufficient</Badge> : c.sufficientUsdt ? <Badge tone="green">ok</Badge> : null}</span>
              <span>Gas ~<strong>{bnb(c.requiredBnbWei)}</strong> {c.sufficientBnb === false ? <Badge tone="red">insufficient</Badge> : c.sufficientBnb ? <Badge tone="green">ok</Badge> : null}</span>
            </div>
          ) : null}

          <div style={{ display: "flex", gap: 8, flexWrap: "wrap", marginBottom: 14 }}>
            <Button onClick={sendAll} loading={busy && b!.status !== "sending"} disabled={!canSend}>
              {b!.status === "sending" ? "Sending…" : "Send all"}
            </Button>
            {b!.status === "prepared" ? <Button variant="ghost" onClick={cancel} disabled={busy}>Cancel batch</Button> : null}
            <Button variant="ghost" onClick={() => openBatch(b!.id)} disabled={busy}>Refresh</Button>
            {!windowOpen ? <span style={{ alignSelf: "center", fontSize: 12, color: "#d69e2e" }}>Send disabled — window closed</span> : null}
            {c?.sufficientUsdt === false ? <span style={{ alignSelf: "center", fontSize: 12, color: "#d6455a" }}>Top up hot-wallet USDT</span> : null}
          </div>

          {report!.invalidAddresses.length > 0 ? (
            <Notice kind="info">
              {report!.invalidAddresses.length} partner(s) excluded for an invalid address: {report!.invalidAddresses.map((i) => `${i.partnerId.slice(0, 8)} (${i.reason})`).join(", ")}
            </Notice>
          ) : null}

          <div style={{ overflowX: "auto", marginTop: 8 }}>
            <table style={{ width: "100%", fontSize: 13, borderCollapse: "collapse" }}>
              <thead>
                <tr style={{ textAlign: "left", ...faint, fontSize: 11 }}>
                  <th style={{ padding: "4px 8px" }}>Partner</th>
                  <th style={{ padding: "4px 8px" }}>Address</th>
                  <th style={{ padding: "4px 8px" }} className="num">Amount</th>
                  <th style={{ padding: "4px 8px" }}>Status</th>
                  <th style={{ padding: "4px 8px" }}>Tx</th>
                  <th style={{ padding: "4px 8px" }} />
                </tr>
              </thead>
              <tbody>
                {report!.rows.map((r) => (
                  <tr key={r.id} style={{ borderTop: "1px solid var(--border)" }}>
                    <td style={{ padding: "6px 8px", fontWeight: 600 }}>{r.partner}</td>
                    <td style={{ padding: "6px 8px", ...mono }}>{shortAddr(r.walletAddress)}</td>
                    <td style={{ padding: "6px 8px", fontWeight: 600 }} className="num">{formatUsd(r.amountNano)}</td>
                    <td style={{ padding: "6px 8px" }}>{chainBadge(r)}{r.chainError ? <div style={{ fontSize: 11, color: "#d6455a" }}>{r.chainError.slice(0, 60)}</div> : null}</td>
                    <td style={{ padding: "6px 8px" }}>
                      {r.txHash ? <a href={`https://bscscan.com/tx/${r.txHash}`} target="_blank" rel="noreferrer" style={{ ...mono, color: "var(--accent-strong,#3b5bdb)" }}>{r.txHash.slice(0, 10)}…</a> : "—"}
                    </td>
                    <td style={{ padding: "6px 8px" }}>
                      {r.status !== "paid" && r.chainStatus !== "broadcast" ? (
                        <span style={{ display: "inline-flex", gap: 4 }}>
                          <Button size="sm" variant="ghost" disabled={busy || !windowOpen} onClick={() => sendOne(r)}>{r.chainStatus === "failed" ? "Retry" : "Send"}</Button>
                          {r.chainStatus === "failed" ? <Button size="sm" variant="ghost" disabled={busy} onClick={() => releaseOne(r)}>Release</Button> : null}
                        </span>
                      ) : null}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      )}

      {/* history */}
      <Card title="Batch history">
        {batches.length === 0 ? <EmptyState title="No payout runs yet" /> : (
          <div style={{ overflowX: "auto" }}>
            <table style={{ width: "100%", fontSize: 13, borderCollapse: "collapse" }}>
              <thead><tr style={{ textAlign: "left", ...faint, fontSize: 11 }}>
                <th style={{ padding: "4px 8px" }}>Created</th><th style={{ padding: "4px 8px" }}>Status</th>
                <th style={{ padding: "4px 8px" }} className="num">Recipients</th><th style={{ padding: "4px 8px" }} className="num">Total</th><th style={{ padding: "4px 8px" }} />
              </tr></thead>
              <tbody>
                {batches.map((bat) => (
                  <tr key={bat.id} style={{ borderTop: "1px solid var(--border)" }}>
                    <td style={{ padding: "6px 8px" }}>{formatDate(bat.createdAt)}</td>
                    <td style={{ padding: "6px 8px" }}><Badge tone={bat.status === "sent" ? "green" : bat.status === "canceled" || bat.status === "failed" ? "red" : "yellow"}>{bat.status}</Badge></td>
                    <td style={{ padding: "6px 8px" }} className="num">{bat.recipientCount}</td>
                    <td style={{ padding: "6px 8px" }} className="num">{formatUsd(bat.totalNano)}</td>
                    <td style={{ padding: "6px 8px" }}><Button size="sm" variant="ghost" onClick={() => openBatch(bat.id)}>Open</Button></td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
    </div>
  );
}
