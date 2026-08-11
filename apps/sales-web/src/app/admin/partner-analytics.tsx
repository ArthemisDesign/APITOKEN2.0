"use client";

import { useCallback, useEffect, useState } from "react";
import {
  api,
  ApiError,
  formatBps,
  formatDate,
  formatUsd,
  formatUsdCompact,
  isPositiveNanoUsd,
  parseCanonicalNanoUsd,
  type PartnerActivityEvent,
  type PartnerAnalyticsList,
  type PartnerAnalyticsRow,
  type PartnerAnalyticsSortKey,
  type PartnerDetailBundle,
} from "@/lib/api";
import { Badge, Button, Card, EmptyState, Input, Loading, Notice, StatusBadge, Table } from "@/components/ui";

// На admin.partners Caddy инжектит x-sales-admin-key после managed admin auth → ключ пустой ("").
function adminHeaders(key: string): Record<string, string> {
  return key ? { "x-sales-admin-key": key } : {};
}

const PAGE = 25;
const faint = { color: "var(--text-faint)" } as const;

function relTime(iso: string | null): string {
  if (!iso) return "never";
  const diff = Date.now() - new Date(iso).getTime();
  if (Number.isNaN(diff)) return "—";
  const d = Math.floor(diff / 86_400_000);
  if (d <= 0) {
    const h = Math.floor(diff / 3_600_000);
    if (h <= 0) return "just now";
    return `${h}h ago`;
  }
  if (d < 30) return `${d}d ago`;
  return formatDate(iso);
}

function convPct(row: PartnerAnalyticsRow): string {
  if (row.referredUsers === 0) return "—";
  return `${Math.round((row.convertedUsers / row.referredUsers) * 100)}%`;
}

const COLUMNS: { key: PartnerAnalyticsSortKey; label: string; num?: boolean }[] = [
  { key: "deposits_total", label: "Deposits driven", num: true },
  { key: "referred_users", label: "Referred", num: true },
  { key: "spend_total", label: "Spend", num: true },
  { key: "earned_total", label: "Earned", num: true },
  { key: "unpaid", label: "Unpaid", num: true },
  { key: "team_size", label: "Team", num: true },
  { key: "last_seen_at", label: "Last seen", num: false },
];

export function PartnersTab({ adminKey }: { adminKey: string }) {
  const [data, setData] = useState<PartnerAnalyticsList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sort, setSort] = useState<PartnerAnalyticsSortKey>("deposits_total");
  const [dir, setDir] = useState<"asc" | "desc">("desc");
  const [status, setStatus] = useState<"all" | "active" | "pending" | "suspended">("all");
  const [search, setSearch] = useState("");
  const [query, setQuery] = useState("");
  const [offset, setOffset] = useState(0);
  const [selected, setSelected] = useState<PartnerAnalyticsRow | null>(null);

  useEffect(() => {
    const t = setTimeout(() => {
      setQuery(search);
      setOffset(0);
    }, 300);
    return () => clearTimeout(t);
  }, [search]);

  const load = useCallback(async () => {
    setError(null);
    const params = new URLSearchParams({ sort, dir, status, limit: String(PAGE), offset: String(offset) });
    if (query.trim()) params.set("q", query.trim());
    try {
      const res = await api<PartnerAnalyticsList>(`/v1/admin/partner-analytics?${params.toString()}`, {
        headers: adminHeaders(adminKey),
      });
      setData(res);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Failed to load analytics.");
    }
  }, [adminKey, sort, dir, status, query, offset]);

  useEffect(() => {
    void load();
  }, [load]);

  function toggleSort(key: PartnerAnalyticsSortKey) {
    if (sort === key) setDir((d) => (d === "desc" ? "asc" : "desc"));
    else {
      setSort(key);
      setDir("desc");
    }
    setOffset(0);
  }

  const totals = data?.totals;
  const items = data?.items ?? [];
  const shownFrom = totals && totals.total > 0 ? offset + 1 : 0;
  const shownTo = Math.min(offset + PAGE, totals?.total ?? 0);
  const convTotal = totals && totals.referredUsers > 0 ? Math.round((totals.convertedUsers / totals.referredUsers) * 100) : 0;

  return (
    <div className="stack">
      {/* KPI strip over the CURRENT filter */}
      <div className="stat-grid" style={{ gridTemplateColumns: "repeat(5, 1fr)" }}>
        <Kpi label="Partners" value={totals ? String(totals.total) : "…"} foot={totals ? `${totals.active} active` : ""} />
        <Kpi label="Deposits driven" value={totals ? formatUsd(totals.depositsNano) : "…"} foot="real money in" accent />
        <Kpi label="Referred users" value={totals ? String(totals.referredUsers) : "…"} foot={`${totals?.convertedUsers ?? 0} deposited`} />
        <Kpi label="Conversion" value={totals ? `${convTotal}%` : "…"} foot="referred → paid" />
        <Kpi label="Unpaid owed" value={totals ? formatUsd(totals.unpaidNano) : "…"} foot="across filter" />
      </div>

      <Card
        title="Partner analytics"
        sub="Ranked by real deposits their referrals paid. Click a row to open the partner card with full stats and activity."
      >
        <div style={{ display: "flex", gap: 10, flexWrap: "wrap", alignItems: "center", marginBottom: 12 }}>
          <Input
            placeholder="Search @username, email, name, code…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            style={{ maxWidth: 320 }}
          />
          <select
            value={status}
            onChange={(e) => {
              setStatus(e.target.value as typeof status);
              setOffset(0);
            }}
            style={selectStyle}
          >
            <option value="all">All statuses</option>
            <option value="active">Active</option>
            <option value="pending">Pending</option>
            <option value="suspended">Suspended</option>
          </select>
          <span style={{ marginLeft: "auto", fontSize: 12, ...faint }}>
            {totals ? `${shownFrom}–${shownTo} of ${totals.total}` : ""}
          </span>
          <div style={{ display: "flex", gap: 6 }}>
            <Button size="sm" variant="ghost" disabled={offset === 0} onClick={() => setOffset(Math.max(0, offset - PAGE))}>
              ‹ Prev
            </Button>
            <Button size="sm" variant="ghost" disabled={!totals || shownTo >= totals.total} onClick={() => setOffset(offset + PAGE)}>
              Next ›
            </Button>
          </div>
        </div>

        {error ? <Notice kind="error">{error}</Notice> : !data ? <Loading /> : items.length === 0 ? (
          <EmptyState title="No partners match this filter" />
        ) : (
          <Table
            head={
              <>
                <th>Partner</th>
                {COLUMNS.map((c) => (
                  <SortTh key={c.key} col={c} sort={sort} dir={dir} onClick={() => toggleSort(c.key)} />
                ))}
                <th>Status</th>
              </>
            }
          >
            {items.map((p) => (
              <tr key={p.id} style={{ cursor: "pointer" }} onClick={() => setSelected(p)}>
                <td>
                  <div style={{ fontWeight: 600 }}>{p.telegramUsername ? `@${p.telegramUsername}` : p.email ?? "—"}</div>
                  <div style={{ fontSize: 12, ...faint }}>{p.displayName ?? p.referralCode}</div>
                </td>
                <td className="num">
                  <div style={{ fontWeight: 600 }}>{formatUsd(p.depositsTotalNano)}</div>
                  <div style={{ fontSize: 11, ...faint }}>{formatUsd(p.deposits30dNano)} · 30d</div>
                </td>
                <td className="num">
                  <div>{p.referredUsers}</div>
                  <div style={{ fontSize: 11, ...faint }}>{p.convertedUsers} · {convPct(p)}</div>
                </td>
                <td className="num">{formatUsd(p.spendTotalNano)}</td>
                <td className="num">
                  <div>{formatUsd(p.earnedTotalNano)}</div>
                  <div style={{ fontSize: 11, ...faint }}>{formatUsd(p.earned30dNano)} · 30d</div>
                </td>
                <td className="num" style={{ fontWeight: isPositiveNanoUsd(p.unpaidNano) ? 600 : 400 }}>{formatUsd(p.unpaidNano)}</td>
                <td className="num">{p.teamSize}</td>
                <td style={{ fontSize: 12, ...faint }}>{relTime(p.lastSeenAt)}</td>
                <td><StatusBadge status={p.status} /></td>
              </tr>
            ))}
          </Table>
        )}
      </Card>

      {selected ? (
        <PartnerDrawer
          row={selected}
          adminKey={adminKey}
          onClose={() => setSelected(null)}
          onChanged={() => {
            void load();
          }}
        />
      ) : null}
    </div>
  );
}

function SortTh({
  col,
  sort,
  dir,
  onClick,
}: {
  col: { key: PartnerAnalyticsSortKey; label: string; num?: boolean };
  sort: PartnerAnalyticsSortKey;
  dir: "asc" | "desc";
  onClick: () => void;
}) {
  const active = sort === col.key;
  return (
    <th
      className={col.num ? "num" : undefined}
      style={{ cursor: "pointer", userSelect: "none", color: active ? "var(--text)" : undefined }}
      onClick={onClick}
    >
      {col.label} {active ? (dir === "desc" ? "▾" : "▴") : ""}
    </th>
  );
}

function Kpi({ label, value, foot, accent }: { label: string; value: string; foot?: string; accent?: boolean }) {
  return (
    <div className="stat-card">
      <div className="stat-label">{label}</div>
      <div className="stat-value" style={accent ? { color: "var(--accent-strong, #3b5bdb)" } : undefined}>{value}</div>
      {foot ? <div className="stat-foot">{foot}</div> : null}
    </div>
  );
}

const selectStyle: React.CSSProperties = {
  padding: "8px 10px",
  borderRadius: 8,
  border: "1px solid var(--border)",
  background: "var(--surface)",
  color: "var(--text)",
  fontSize: 13,
};

// ---------------------------------------------------------------------------
// Partner detail drawer
// ---------------------------------------------------------------------------

function PartnerDrawer({
  row,
  adminKey,
  onClose,
  onChanged,
}: {
  row: PartnerAnalyticsRow;
  adminKey: string;
  onClose: () => void;
  onChanged: () => void;
}) {
  const [detail, setDetail] = useState<PartnerDetailBundle | null>(null);
  const [activity, setActivity] = useState<PartnerActivityEvent[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [bps, setBps] = useState(String(row.commissionBps));
  const [subBps, setSubBps] = useState(String(row.subCommissionBps));

  const reload = useCallback(async () => {
    try {
      const [d, a] = await Promise.all([
        api<PartnerDetailBundle>(`/v1/admin/partners/${row.id}/analytics`, { headers: adminHeaders(adminKey) }),
        api<{ events: PartnerActivityEvent[] }>(`/v1/admin/partners/${row.id}/activity?limit=80`, { headers: adminHeaders(adminKey) }),
      ]);
      setDetail(d);
      setActivity(a.events);
      setBps(String(d.partner.commissionBps));
      setSubBps(String(d.partner.subCommissionBps));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Failed to load partner.");
    }
  }, [adminKey, row.id]);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function patch(body: Record<string, unknown>) {
    setBusy(true);
    setError(null);
    try {
      await api(`/v1/admin/partners/${row.id}`, { method: "PATCH", headers: adminHeaders(adminKey), body });
      await reload();
      onChanged();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Update failed.");
    } finally {
      setBusy(false);
    }
  }

  async function postPromo(body: { enabled: boolean; maxValueUsd: number; maxCount: number }) {
    setBusy(true);
    setError(null);
    try {
      await api(`/v1/admin/partners/${row.id}/promo`, { method: "POST", headers: adminHeaders(adminKey), body });
      await reload();
      onChanged();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Promo update failed.");
    } finally {
      setBusy(false);
    }
  }

  const p = detail?.partner ?? row;
  const label = p.telegramUsername ? `@${p.telegramUsername}` : p.email ?? p.id.slice(0, 8);
  const suspended = p.status !== "active";
  const dirty = bps !== String(p.commissionBps) || subBps !== String(p.subCommissionBps);

  function editDiscount() {
    const cur = p.referralDiscountEnabled ? String(p.referralDiscountBps / 100) : "off";
    const v = window.prompt(`Referral discount right for ${label}\nPercent 0–95 (price floor for their referrals), or "off".`, cur);
    if (v == null) return;
    if (v.trim().toLowerCase() === "off") return void patch({ referralDiscountEnabled: false, referralDiscountBps: 0 });
    const pct = Number(v.trim());
    if (!Number.isFinite(pct) || pct < 0 || pct > 95) return setError("Discount must be 0–95.");
    void patch({ referralDiscountEnabled: true, referralDiscountBps: Math.round(pct * 100) });
  }

  // Партнёрская ставка конкретному рефералу: перевод b2c-реферала на фикс-процент или смена/снятие (0).
  function editReferralRate(u: { userMask: string; userRef?: string; referralFloorBps?: number | null }) {
    if (!u.userRef) return;
    const cur = u.referralFloorBps ? String(u.referralFloorBps / 100) : "";
    const v = window.prompt(`Partner rate for ${u.userMask}\nPercent 0–95 (price floor; 0 removes the rate, back to tiers).`, cur);
    if (v == null) return;
    const pct = Number(v.trim());
    if (!Number.isFinite(pct) || pct < 0 || pct > 95) return setError("Rate must be 0–95.");
    void (async () => {
      setBusy(true);
      setError(null);
      try {
        await api(`/v1/admin/partners/${row.id}/referrals/${u.userRef}/discount`, {
          method: "POST", headers: adminHeaders(adminKey), body: { discountBps: Math.round(pct * 100) },
        });
        await reload();
        onChanged();
      } catch (err) {
        setError(err instanceof ApiError ? err.message : "Rate update failed.");
      } finally {
        setBusy(false);
      }
    })();
  }

  function editPromo() {
    const v = window.prompt(`Promo codes for ${label}\n"maxUSD/count" to enable (e.g. 20/10), or "off".`, p.promoEnabled ? "" : "off");
    if (v == null) return;
    if (v.trim().toLowerCase() === "off") return void postPromo({ enabled: false, maxValueUsd: 0, maxCount: 0 });
    const m = /^\s*(\d{1,5})\s*\/\s*(\d{1,5})\s*$/.exec(v);
    if (!m) return setError("Format: maxUSD/count, e.g. 20/10");
    void postPromo({ enabled: true, maxValueUsd: Number(m[1]), maxCount: Number(m[2]) });
  }

  return (
    <div style={overlayStyle} onClick={onClose}>
      <aside style={panelStyle} onClick={(e) => e.stopPropagation()}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start", gap: 12 }}>
          <div>
            <div style={{ fontSize: 18, fontWeight: 700 }}>{label} <StatusBadge status={p.status} /></div>
            <div style={{ fontSize: 12, ...faint, marginTop: 2 }}>
              {p.displayName ? `${p.displayName} · ` : ""}code <span className="mono">{p.referralCode}</span>
              {p.parentLabel ? <> · under {p.parentLabel}</> : null}
            </div>
            <div style={{ fontSize: 12, ...faint }}>
              joined {formatDate(p.createdAt)} · last seen {relTime(p.lastSeenAt)}
            </div>
          </div>
          <Button size="sm" variant="ghost" onClick={onClose}>✕ Close</Button>
        </div>

        {error ? <div style={{ marginTop: 10 }}><Notice kind="error">{error}</Notice></div> : null}

        {/* Rights & actions */}
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center", marginTop: 14 }}>
          <label style={{ fontSize: 12, ...faint }}>Rate</label>
          <Input className="inline-edit" inputMode="numeric" value={bps} onChange={(e) => setBps(e.target.value.replace(/[^\d]/g, ""))} aria-label="Commission bps" style={{ width: 84 }} />
          <span style={{ fontSize: 11, ...faint }}>{/^\d+$/.test(bps) ? formatBps(Number(bps)) : "—"}</span>
          <label style={{ fontSize: 12, ...faint }}>Sub</label>
          <Input className="inline-edit" inputMode="numeric" value={subBps} onChange={(e) => setSubBps(e.target.value.replace(/[^\d]/g, ""))} aria-label="Sub bps" style={{ width: 84 }} />
          <span style={{ fontSize: 11, ...faint }}>{/^\d+$/.test(subBps) ? formatBps(Number(subBps)) : "—"}</span>
          <Button size="sm" variant="ghost" disabled={!dirty || busy || !/^\d+$/.test(bps) || !/^\d+$/.test(subBps)} onClick={() => patch({ commissionBps: Number(bps), subCommissionBps: Number(subBps) })}>Save</Button>
          <Button size="sm" variant={suspended ? "primary" : "danger"} disabled={busy} onClick={() => patch({ status: suspended ? "active" : "suspended" })}>{suspended ? "Activate" : "Suspend"}</Button>
          <Button size="sm" variant="ghost" disabled={busy} onClick={editPromo}>{p.promoEnabled ? "Promo: on" : "Promo: off"}</Button>
          <Button size="sm" variant="ghost" disabled={busy} onClick={editDiscount}>{p.referralDiscountEnabled ? `Discount ${formatBps(p.referralDiscountBps)}` : "Discount: off"}</Button>
        </div>

        {/* Stat grid */}
        <div className="stat-grid" style={{ gridTemplateColumns: "repeat(4, 1fr)", marginTop: 16 }}>
          <Kpi label="Deposits driven" value={formatUsd(p.depositsTotalNano)} foot={`${formatUsd(p.deposits30dNano)} · 30d`} accent />
          <Kpi label="Referred / paid" value={`${p.referredUsers} / ${p.convertedUsers}`} foot={`${convPct(p)} conversion`} />
          <Kpi label="Real spend" value={formatUsd(p.spendTotalNano)} foot={`${formatUsd(p.spend30dNano)} · 30d`} />
          <Kpi label="Earned" value={formatUsd(p.earnedTotalNano)} foot={`${formatUsd(p.earned30dNano)} · 30d`} />
          <Kpi label="Paid out" value={formatUsd(p.paidNano)} />
          <Kpi label="Unpaid owed" value={formatUsd(p.unpaidNano)} />
          <Kpi label="Team" value={String(p.teamSize)} foot="sub-partners" />
          <Kpi label="Links / promos" value={`${p.linksUsed}/${p.linksTotal} · ${p.promosUsed}/${p.promosTotal}`} foot="used / total" />
        </div>

        {detail ? <Sparkline daily={detail.daily} /> : null}

        {!detail || !activity ? (
          <div style={{ marginTop: 16 }}><Loading /></div>
        ) : (
          <div style={{ marginTop: 16, display: "grid", gap: 16 }}>
            <Section title={`Activity (${activity.length})`}>
              {activity.length === 0 ? <Muted>No activity yet.</Muted> : (
                <ul style={feedStyle}>
                  {activity.map((e, i) => (
                    <li key={i} style={feedItemStyle}>
                      <span style={{ ...dotStyle, background: activityColor(e.type) }} />
                      <span style={{ flex: 1 }}>{e.label}{e.amountNano ? <strong> {formatUsd(e.amountNano)}</strong> : null}</span>
                      <span style={{ fontSize: 11, ...faint, whiteSpace: "nowrap" }}>{relTime(e.at)}</span>
                    </li>
                  ))}
                </ul>
              )}
            </Section>

            <Section title={`Discount links (${detail.discountLinks.length})`}>
              {detail.discountLinks.length === 0 ? <Muted>None issued.</Muted> : (
                <MiniTable rows={detail.discountLinks.map((l) => [
                  <span key="c" className="mono">{l.code}</span>,
                  formatBps(l.discountBps),
                  l.note ?? "—",
                  l.consumedAt ? `used ${relTime(l.consumedAt)}` : "unused",
                ])} cols={["Code", "Discount", "For", "State"]} />
              )}
            </Section>

            <Section title={`Promo codes (${detail.promos.length})`}>
              {detail.promos.length === 0 ? <Muted>None created.</Muted> : (
                <MiniTable rows={detail.promos.map((c) => [
                  <span key="c" className="mono">{c.code}</span>,
                  formatUsd(c.valueNano),
                  c.status,
                  c.redeemedAt ? `redeemed ${relTime(c.redeemedAt)}` : formatDate(c.createdAt),
                ])} cols={["Code", "Value", "Status", "When"]} />
              )}
            </Section>

            <Section title={`Team (${detail.team.length})`}>
              {detail.team.length === 0 ? <Muted>No sub-partners.</Muted> : (
                <MiniTable rows={detail.team.map((m) => [
                  m.telegramUsername ? `@${m.telegramUsername}` : m.email ?? m.id.slice(0, 8),
                  formatBps(m.commissionBps),
                  `${m.referredUsers} refs`,
                  formatUsd(m.myOverrideNano) + " override",
                ])} cols={["Sub-partner", "Rate", "Referrals", "My override"]} />
              )}
            </Section>

            <Section title={`Referred users (${detail.referrals.length})`}>
              {detail.referrals.length === 0 ? <Muted>No referrals yet.</Muted> : (
                <MiniTable rows={detail.referrals.slice(0, 50).map((u) => [
                  <span key="u" className="mono">{u.userMask}</span>,
                  u.customerType === "b2b"
                    ? <Badge key="t" tone="green">B2B</Badge>
                    : u.customerType === "b2c"
                      ? ((u.referralFloorBps ?? 0) > 0
                        ? <Badge key="t" tone="yellow">partner {u.discountPercent != null ? `${u.discountPercent}%` : ""}</Badge>
                        : <span key="t">B2C{u.discountPercent != null ? ` · ${u.discountPercent}%` : ""}</span>)
                      : "—",
                  formatDate(u.attributedAt),
                  formatUsd(u.spendNano) + " spend",
                  formatUsd(u.earnedNano) + " earned",
                  u.userRef && u.customerType === "b2c"
                    ? <Button key="a" size="sm" variant="ghost" disabled={busy} onClick={() => editReferralRate(u)}>Set&nbsp;rate</Button>
                    : "—",
                ])} cols={["User", "Type", "Joined", "Spend", "Earned", ""]} />
              )}
            </Section>

            <Section title={`Payouts (${detail.payouts.length})`}>
              {detail.payouts.length === 0 ? <Muted>No payouts.</Muted> : (
                <MiniTable rows={detail.payouts.map((po) => [
                  formatUsd(po.amountNano),
                  <Badge key="s" tone={po.status === "paid" ? "green" : po.status === "rejected" ? "red" : "yellow"}>{po.status}</Badge>,
                  formatDate(po.requestedAt),
                  po.paidAt ? `paid ${formatDate(po.paidAt)}` : po.adminNote ?? "—",
                ])} cols={["Amount", "Status", "Requested", "Note"]} />
              )}
            </Section>
          </div>
        )}
      </aside>
    </div>
  );
}

function Sparkline({ daily }: { daily: { date: string; spendNano: string; earnedNano: string }[] }) {
  if (daily.length === 0) return null;
  const parsed = daily.map((d) => parseCanonicalNanoUsd(d.spendNano));
  if (parsed.some((value) => value === null)) {
    return <div style={{ marginTop: 14, fontSize: 11, ...faint }}>Real-spend chart unavailable: invalid API money.</div>;
  }
  const values = parsed as bigint[];
  const max = values.reduce((m, value) => value > m ? value : m, 1n);
  const total = values.reduce((m, value) => m + value, 0n);
  return (
    <div style={{ marginTop: 14 }}>
      <div style={{ display: "flex", justifyContent: "space-between", fontSize: 11, ...faint, marginBottom: 6 }}>
        <span>Real spend · last 30 days</span>
        <span>{formatUsdCompact(total.toString())} total</span>
      </div>
      <div style={{ display: "flex", alignItems: "flex-end", gap: 2, height: 44 }}>
        {daily.map((d, index) => {
          const v = values[index];
          const h = max > 0n ? Number((v * 100n) / max) : 0;
          return (
            <div
              key={d.date}
              title={`${d.date}: ${formatUsd(d.spendNano)}`}
              style={{ flex: 1, height: `${Math.max(h, 2)}%`, background: v > 0n ? "var(--accent-strong, #3b5bdb)" : "var(--border)", borderRadius: 2, minWidth: 3 }}
            />
          );
        })}
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <div style={{ fontSize: 12, fontWeight: 700, textTransform: "uppercase", letterSpacing: "0.05em", ...faint, marginBottom: 8 }}>{title}</div>
      {children}
    </div>
  );
}

function Muted({ children }: { children: React.ReactNode }) {
  return <div style={{ fontSize: 13, ...faint }}>{children}</div>;
}

function MiniTable({ cols, rows }: { cols: string[]; rows: React.ReactNode[][] }) {
  return (
    <div style={{ overflowX: "auto" }}>
      <table style={{ width: "100%", fontSize: 13, borderCollapse: "collapse" }}>
        <thead>
          <tr style={{ textAlign: "left", ...faint, fontSize: 11 }}>
            {cols.map((c) => <th key={c} style={{ padding: "4px 8px", fontWeight: 500 }}>{c}</th>)}
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={i} style={{ borderTop: "1px solid var(--border)" }}>
              {r.map((cell, j) => <td key={j} style={{ padding: "6px 8px" }}>{cell}</td>)}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function activityColor(type: string): string {
  if (type === "deposit") return "#26a15e";
  if (type === "referral") return "#3b5bdb";
  if (type.startsWith("payout")) return "#d69e2e";
  if (type.startsWith("discount") || type.startsWith("promo")) return "#7c3aed";
  if (type === "admin") return "#e8590c";
  return "var(--text-faint)";
}

const overlayStyle: React.CSSProperties = {
  position: "fixed",
  inset: 0,
  background: "rgba(0,0,0,0.38)",
  zIndex: 50,
  display: "flex",
  justifyContent: "flex-end",
};
const panelStyle: React.CSSProperties = {
  width: "min(680px, 100%)",
  height: "100%",
  overflowY: "auto",
  background: "var(--surface, #fff)",
  borderLeft: "1px solid var(--border)",
  padding: "20px 22px",
  boxShadow: "-8px 0 24px rgba(0,0,0,0.12)",
};
const feedStyle: React.CSSProperties = { listStyle: "none", margin: 0, padding: 0, display: "grid", gap: 6, maxHeight: 320, overflowY: "auto" };
const feedItemStyle: React.CSSProperties = { display: "flex", alignItems: "center", gap: 8, fontSize: 13, padding: "3px 0" };
const dotStyle: React.CSSProperties = { width: 8, height: 8, borderRadius: "50%", flex: "0 0 auto" };
