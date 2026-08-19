"use client";

import { useCallback, useEffect, useState, type FormEvent } from "react";
import {
  api,
  ApiError,
  formatBps,
  formatDate,
  formatUsd,
  sumCanonicalNanoUsd,
  type AdminPayoutRow,
  type InviteRow,
  type PayoutListResponse,
} from "@/lib/api";
import {
  Badge,
  Button,
  Card,
  CopyButton,
  EmptyState,
  Field,
  Input,
  Loading,
  Notice,
  StatusBadge,
  Table,
} from "@/components/ui";
import { PartnersTab } from "./partner-analytics";
import { PayoutSendTab } from "./payout-send";

const KEY_STORAGE = "sales_admin_key";

// На admin.partners Caddy инжектит x-sales-admin-key после managed admin auth → ключ не нужен (key="").
// При прямом доступе (без инжекта) оператор вводит ключ в KeyGate.
function adminHeaders(key: string): Record<string, string> {
  return key ? { "x-sales-admin-key": key } : {};
}

// ---------------------------------------------------------------------------
// Key gate
// ---------------------------------------------------------------------------

function KeyGate({ onUnlock }: { onUnlock: (key: string) => void }) {
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(e: FormEvent) {
    e.preventDefault();
    const trimmed = key.trim();
    if (!trimmed) return;
    setBusy(true);
    setError(null);
    try {
      await api("/v1/admin/overview", { headers: adminHeaders(trimmed) });
      sessionStorage.setItem(KEY_STORAGE, trimmed);
      onUnlock(trimmed);
    } catch (err) {
      setError(
        err instanceof ApiError && (err.status === 401 || err.status === 403)
          ? "Invalid admin key."
          : err instanceof ApiError
            ? err.message
            : "Could not reach the API.",
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="auth-shell" style={{ justifyContent: "center" }}>
      <div className="auth-card">
        <h1>Admin access</h1>
        <p className="auth-sub">
          Enter the admin key. It is kept in this tab&apos;s session only.
        </p>
        {error ? <Notice kind="error">{error}</Notice> : null}
        <form onSubmit={submit}>
          <Field label="Admin key">
            <Input
              type="password"
              value={key}
              onChange={(e) => setKey(e.target.value)}
              placeholder="x-admin-key"
              autoFocus
              required
            />
          </Field>
          <Button type="submit" loading={busy} style={{ width: "100%" }}>
            Enter admin key
          </Button>
        </form>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Overview tab — renders whatever totals the API returns, defensively.
// ---------------------------------------------------------------------------

function labelize(key: string): string {
  return key
    .replace(/Nano$/, "")
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/^./, (c) => c.toUpperCase());
}

function OverviewTab({ adminKey }: { adminKey: string }) {
  const [data, setData] = useState<Record<string, unknown> | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res = await api<Record<string, unknown>>("/v1/admin/overview", {
          headers: adminHeaders(adminKey),
        });
        if (!cancelled) setData(res);
      } catch (err) {
        if (!cancelled)
          setError(err instanceof ApiError ? err.message : "Failed to load overview.");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [adminKey]);

  if (error) return <Notice kind="error">{error}</Notice>;
  if (!data) return <Loading />;

  const flat: Array<{ key: string; value: string }> = [];
  const walk = (obj: Record<string, unknown>, prefix: string) => {
    for (const [k, v] of Object.entries(obj)) {
      if (v !== null && typeof v === "object" && !Array.isArray(v)) {
        walk(v as Record<string, unknown>, prefix ? `${prefix} · ${labelize(k)}` : labelize(k));
      } else if (typeof v === "string" && /Nano$/.test(k)) {
        flat.push({ key: prefix ? `${prefix} · ${labelize(k)}` : labelize(k), value: formatUsd(v) });
      } else if (typeof v === "number" || typeof v === "string") {
        flat.push({
          key: prefix ? `${prefix} · ${labelize(k)}` : labelize(k),
          value: String(v),
        });
      }
    }
  };
  walk(data, "");

  if (flat.length === 0) return <EmptyState title="No overview data" />;

  return (
    <div className="stat-grid" style={{ gridTemplateColumns: "repeat(3, 1fr)" }}>
      {flat.map((s) => (
        <div className="stat-card" key={s.key}>
          <div className="stat-label">{s.key}</div>
          <div className="stat-value" style={{ fontSize: 20 }}>
            {s.value}
          </div>
        </div>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Partners tab
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Payouts tab
// ---------------------------------------------------------------------------

function PayoutRowView({
  payout,
  adminKey,
  onDone,
  onError,
}: {
  payout: AdminPayoutRow;
  adminKey: string;
  onDone: () => void;
  onError: (msg: string) => void;
}) {
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);

  async function decide(action: string) {
    setBusy(true);
    try {
      await api(`/v1/admin/payouts/${payout.id}/decision`, {
        method: "POST",
        headers: adminHeaders(adminKey),
        body: { action, ...(note.trim() ? { note: note.trim() } : {}) },
      });
      onDone();
    } catch (err) {
      onError(err instanceof ApiError ? err.message : "Decision failed.");
    } finally {
      setBusy(false);
    }
  }

  const pending = ["pending", "requested", "processing"].includes(
    payout.status.toLowerCase(),
  );

  return (
    <tr>
      <td>
        <div style={{ fontWeight: 600 }}>{payout.partnerEmail ?? payout.partnerId ?? "—"}</div>
        <div style={{ fontSize: 12, color: "var(--text-faint)" }}>
          {formatDate(payout.requestedAt)}
        </div>
      </td>
      <td className="num" style={{ fontWeight: 700 }}>
        {formatUsd(payout.amountNano)}
      </td>
      <td>
        <div>{payout.method}</div>
        {payout.details ? (
          <div className="mono" style={{ fontSize: 12, color: "var(--text-faint)", wordBreak: "break-all" }}>
            {payout.details}
          </div>
        ) : null}
      </td>
      <td>
        <StatusBadge status={payout.status} />
      </td>
      <td>
        {pending ? (
          <div style={{ display: "flex", flexDirection: "column", gap: 6, minWidth: 200 }}>
            <Input
              placeholder="Note (optional)"
              value={note}
              onChange={(e) => setNote(e.target.value)}
              style={{ padding: "5px 8px", fontSize: 13 }}
            />
            <div className="row-actions">
              <Button size="sm" disabled={busy} onClick={() => decide("approve")}>
                Approve
              </Button>
              <Button size="sm" variant="ghost" disabled={busy} onClick={() => decide("paid")}>
                Mark paid
              </Button>
              <Button size="sm" variant="danger" disabled={busy} onClick={() => decide("reject")}>
                Reject
              </Button>
            </div>
          </div>
        ) : payout.status.toLowerCase() === "approved" ? (
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => decide("paid")}>
            Mark paid
          </Button>
        ) : (
          <span style={{ color: "var(--text-faint)", fontSize: 13 }}>
            {payout.paidAt ? `Paid ${formatDate(payout.paidAt)}` : "—"}
          </span>
        )}
      </td>
    </tr>
  );
}

function PayoutsTab({ adminKey }: { adminKey: string }) {
  const [filter, setFilter] = useState<"pending" | "all">("pending");
  const [items, setItems] = useState<AdminPayoutRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setItems(null);
    try {
      // Бэкенд принимает только requested|approved|paid|rejected — "pending" даёт 422. Очередь на
      // действие = requested (только что запрошенные выплаты).
      const qs = filter === "pending" ? "?status=requested" : "";
      const res = await api<{ items: AdminPayoutRow[] }>(`/v1/admin/payouts${qs}`, {
        headers: adminHeaders(adminKey),
      });
      setItems(res.items);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Failed to load payouts.");
    }
  }, [adminKey, filter]);

  useEffect(() => {
    void load();
  }, [load]);

  return (
    <>
      <div style={{ marginBottom: 16, display: "flex", gap: 8 }}>
        <Button
          size="sm"
          variant={filter === "pending" ? "primary" : "ghost"}
          onClick={() => setFilter("pending")}
        >
          Pending queue
        </Button>
        <Button
          size="sm"
          variant={filter === "all" ? "primary" : "ghost"}
          onClick={() => setFilter("all")}
        >
          All payouts
        </Button>
      </div>
      {error ? <Notice kind="error">{error}</Notice> : null}
      {!items && !error ? (
        <Loading />
      ) : items && items.length === 0 ? (
        <Card>
          <EmptyState title={filter === "pending" ? "Queue is empty" : "No payouts yet"} />
        </Card>
      ) : items ? (
        <Table
          head={
            <>
              <th>Partner</th>
              <th className="num">Amount</th>
              <th>Method / details</th>
              <th>Status</th>
              <th>Decision</th>
            </>
          }
        >
          {items.map((p) => (
            <PayoutRowView
              key={p.id}
              payout={p}
              adminKey={adminKey}
              onDone={load}
              onError={setError}
            />
          ))}
        </Table>
      ) : null}
    </>
  );
}

// ---------------------------------------------------------------------------
// Onboarding tab — корневые инвайты, привязанные к telegram-юзернейму
// ---------------------------------------------------------------------------

type AdminApplicationRow = {
  id: string;
  telegramUsername: string | null;
  displayName: string | null;
  note: string | null;
  status: string;
  adminNote: string | null;
  createdAt: string;
  decidedAt: string | null;
};

function ApplicationRowView({
  application,
  adminKey,
  onDone,
  onError,
}: {
  application: AdminApplicationRow;
  adminKey: string;
  onDone: () => void;
  onError: (msg: string) => void;
}) {
  const [bps, setBps] = useState("");
  const [subBps, setSubBps] = useState("");
  const [busy, setBusy] = useState(false);

  async function decide(action: "approve" | "reject") {
    setBusy(true);
    try {
      await api(`/v1/admin/applications/${application.id}/decision`, {
        method: "POST",
        headers: adminHeaders(adminKey),
        body: {
          action,
          ...(/^\d+$/.test(bps) ? { commissionBps: Number(bps) } : {}),
          ...(/^\d+$/.test(subBps) ? { subCommissionBps: Number(subBps) } : {}),
        },
      });
      onDone();
    } catch (err) {
      onError(err instanceof ApiError ? err.message : "Decision failed.");
      setBusy(false);
    }
  }

  return (
    <tr>
      <td>
        <div style={{ fontWeight: 600 }}>
          {application.telegramUsername ? `@${application.telegramUsername}` : "—"}
        </div>
        {application.displayName ? (
          <div style={{ fontSize: 12, color: "var(--text-faint)" }}>{application.displayName}</div>
        ) : null}
      </td>
      <td style={{ maxWidth: 320, whiteSpace: "pre-wrap" }}>{application.note ?? "—"}</td>
      <td>{formatDate(application.createdAt)}</td>
      <td>
        <div className="row-actions">
          <Input
            className="inline-edit"
            inputMode="numeric"
            value={bps}
            onChange={(e) => setBps(e.target.value.replace(/[^\d]/g, ""))}
            placeholder="bps"
            aria-label="Commission bps"
            style={{ maxWidth: 90 }}
          />
          <Input
            className="inline-edit"
            inputMode="numeric"
            value={subBps}
            onChange={(e) => setSubBps(e.target.value.replace(/[^\d]/g, ""))}
            placeholder="sub"
            aria-label="Sub-commission bps"
            style={{ maxWidth: 90 }}
          />
          <Button size="sm" disabled={busy} onClick={() => decide("approve")}>
            Approve
          </Button>
          <Button size="sm" variant="danger" disabled={busy} onClick={() => decide("reject")}>
            Reject
          </Button>
        </div>
      </td>
    </tr>
  );
}

function ApplicationsCard({ adminKey }: { adminKey: string }) {
  const [items, setItems] = useState<AdminApplicationRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const res = await api<{ items: AdminApplicationRow[] }>("/v1/admin/applications?status=pending", {
        headers: adminHeaders(adminKey),
      });
      setItems(res.items);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Failed to load applications.");
    }
  }, [adminKey]);

  useEffect(() => {
    void load();
  }, [load]);

  return (
    <Card
      title="Applications"
      sub="People who signed in with Telegram without an invite and applied. Approve creates the partner account instantly."
    >
      {error ? <Notice kind="error">{error}</Notice> : null}
      {!items ? (
        <Loading />
      ) : items.length === 0 ? (
        <EmptyState title="No pending applications" />
      ) : (
        <Table
          head={
            <>
              <th>Telegram</th>
              <th>Application</th>
              <th>Submitted</th>
              <th>Decision (bps optional)</th>
            </>
          }
        >
          {items.map((application) => (
            <ApplicationRowView
              key={application.id}
              application={application}
              adminKey={adminKey}
              onDone={load}
              onError={setError}
            />
          ))}
        </Table>
      )}
    </Card>
  );
}

// "10" / "12.5" (percent) -> bps (1250). Пусто -> undefined (омит, сервер подставит дефолт),
// заполнено но не число -> null (ошибка валидации).
function pctToBpsOptional(value: string): number | null | undefined {
  const s = value.trim();
  if (s === "") return undefined;
  if (!/^\d{1,3}(\.\d{1,2})?$/.test(s)) return null;
  const pct = Number(s);
  if (!Number.isFinite(pct)) return null;
  return Math.round(pct * 100);
}

function OnboardingTab({ adminKey }: { adminKey: string }) {
  const [items, setItems] = useState<InviteRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [username, setUsername] = useState("");
  const [commissionPct, setCommissionPct] = useState("");
  const [subPct, setSubPct] = useState("");
  const [promoCount, setPromoCount] = useState("");
  const [promoMaxUsd, setPromoMaxUsd] = useState("");
  // Empty = no B2B right. A number grants it and fixes the ceiling in one step.
  const [b2bMaxPct, setB2bMaxPct] = useState("");
  const [busy, setBusy] = useState(false);
  const [created, setCreated] = useState<{ inviteUrl: string; telegramUsername: string | null } | null>(null);

  const load = useCallback(async () => {
    try {
      const res = await api<{ items: InviteRow[] }>("/v1/admin/invites", {
        headers: adminHeaders(adminKey),
      });
      setItems(res.items);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Failed to load invites.");
    }
  }, [adminKey]);

  useEffect(() => {
    void load();
  }, [load]);

  async function create() {
    setError(null); // очищаем прошлую ошибку сразу при клике, чтобы не висела на валидном вводе
    const clean = username.trim().replace(/^@/, "");
    if (!/^[A-Za-z0-9_]{5,32}$/.test(clean)) {
      setError("Enter the sales partner's Telegram username (5–32 letters, digits, underscore).");
      return;
    }
    // Пустое поле процента = «по умолчанию» (не отправляем — сервер подставит дефолт). Заполненное,
    // но не число → ошибка. undefined = омит, null = невалидно.
    const commissionBps = pctToBpsOptional(commissionPct);
    const subCommissionBps = pctToBpsOptional(subPct);
    if (commissionBps === null || subCommissionBps === null) {
      setError("Percents must be numbers like 10 or 12.5.");
      return;
    }
    if ((commissionBps ?? 0) > 10000 || (subCommissionBps ?? 0) > 10000) {
      setError("Commission percent cannot exceed 100%.");
      return;
    }
    const b2bMaxBps = pctToBpsOptional(b2bMaxPct);
    if (b2bMaxBps === null) {
      setError("B2B max discount must be a number like 70 or 72.5.");
      return;
    }
    if ((b2bMaxBps ?? 0) > 9500) {
      setError("B2B max discount cannot exceed 95%.");
      return;
    }
    const count = promoCount.trim() === "" ? 0 : Number(promoCount);
    const maxUsd = promoMaxUsd.trim() === "" ? 0 : Number(promoMaxUsd);
    if (!Number.isInteger(count) || count < 0 || !Number.isInteger(maxUsd) || maxUsd < 0) {
      setError("Promo count and max $ must be whole numbers.");
      return;
    }
    setBusy(true);
    try {
      const res = await api<{ inviteUrl: string; telegramUsername: string | null }>("/v1/admin/invites", {
        method: "POST",
        headers: adminHeaders(adminKey),
        body: {
          telegramUsername: clean,
          ...(commissionBps !== undefined ? { commissionBps } : {}),
          ...(subCommissionBps !== undefined ? { subCommissionBps } : {}),
          // Legacy marker rights are retained in the API for old rows but are not granted by the
          // current product: they never reach the Commerce/engine price authority.
          referralDiscountEnabled: false,
          referralDiscountBps: 0,
          promoMaxCount: count,
          promoMaxValueUsd: maxUsd,
          // The right and its ceiling travel together: a blank field onboards an ordinary partner
          // whose referrals are plain B2C customers.
          ...(b2bMaxBps !== undefined && b2bMaxBps > 0
            ? { b2bEnabled: true, b2bMaxDiscountBps: b2bMaxBps }
            : { b2bEnabled: false }),
        },
      });
      setCreated(res);
      setUsername("");
      setB2bMaxPct("");
      void load();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Could not create the invite.");
    } finally {
      setBusy(false);
    }
  }

  const promoOn = Number(promoCount || "0") > 0 && Number(promoMaxUsd || "0") > 0;

  return (
    <div className="stack">
      <ApplicationsCard adminKey={adminKey} />
      <Card
        title="Onboard a sales partner"
        sub="Invite is bound to their Telegram username. Send them the link — they sign in with Telegram and the account is created with the terms below."
      >
        {error ? <Notice kind="error">{error}</Notice> : null}
        {created ? (
          <div style={{ marginBottom: 14 }}>
            <div className="reflink-row">
              <Input readOnly value={created.inviteUrl} onFocus={(e) => e.currentTarget.select()} />
              <CopyButton value={created.inviteUrl} label="Copy invite" />
            </div>
            <p className="field-hint" style={{ marginTop: 8 }}>
              For <span className="mono">@{created.telegramUsername}</span>
            </p>
          </div>
        ) : null}
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(150px, 1fr))", gap: 12 }}>
          <Field label="Telegram username">
            <Input
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder="@telegram_username"
            />
          </Field>
          <Field label="Commission %" hint="Partner's cut of referral deposits">
            <Input
              value={commissionPct}
              onChange={(e) => setCommissionPct(e.target.value.replace(/[^\d.]/g, ""))}
              inputMode="decimal"
              placeholder="10"
            />
          </Field>
          <Field label="Sub-partner %" hint="Override on recruited sub-sales">
            <Input
              value={subPct}
              onChange={(e) => setSubPct(e.target.value.replace(/[^\d.]/g, ""))}
              inputMode="decimal"
              placeholder="10"
            />
          </Field>
          <Field label="Promo codes (count)" hint="0 = no promo access">
            <Input
              value={promoCount}
              onChange={(e) => setPromoCount(e.target.value.replace(/[^\d]/g, ""))}
              inputMode="numeric"
              placeholder="0"
            />
          </Field>
          <Field label="Max promo $" hint="Per code, our balance">
            <Input
              value={promoMaxUsd}
              onChange={(e) => setPromoMaxUsd(e.target.value.replace(/[^\d]/g, ""))}
              inputMode="numeric"
              placeholder="0"
            />
          </Field>
          <Field label="B2B max discount %" hint="Blank = no B2B right; their referrals stay B2C">
            <Input
              value={b2bMaxPct}
              onChange={(e) => setB2bMaxPct(e.target.value.replace(/[^\d.]/g, ""))}
              inputMode="decimal"
              placeholder="off"
            />
          </Field>
        </div>
        <div className="row-actions" style={{ marginTop: 14, alignItems: "center", gap: 12 }}>
          <Button onClick={create} loading={busy}>
            Create invite
          </Button>
          <span className="field-hint">
            {promoOn
              ? `Promo: up to ${promoCount} code(s), max $${promoMaxUsd} each.`
              : "Promo: off (set both count and max $ to enable)."}
            {" · "}
            {Number(b2bMaxPct || "0") > 0
              ? `B2B: may discount their own customers up to ${b2bMaxPct}%.`
              : "B2B: off (referrals are ordinary B2C customers)."}
          </span>
        </div>
      </Card>

      <Card title="Root invites">
        {!items ? (
          <Loading />
        ) : items.length === 0 ? (
          <EmptyState title="No invites yet" />
        ) : (
          <Table
            head={
              <>
                <th>For</th>
                <th>Commission</th>
                <th>Sub</th>
                <th>Legacy marker</th>
                <th>Promo</th>
                <th>Expires</th>
                <th>Status</th>
                <th />
              </>
            }
          >
            {items.map((inv) => (
              <tr key={inv.code}>
                <td className="mono">{inv.telegramUsername ? `@${inv.telegramUsername}` : "—"}</td>
                <td>{inv.commissionBps != null ? formatBps(inv.commissionBps) : "default"}</td>
                <td>{inv.subCommissionBps != null ? formatBps(inv.subCommissionBps) : "default"}</td>
                <td>{inv.referralDiscountEnabled ? formatBps(inv.referralDiscountBps ?? 0) : "—"}</td>
                <td>
                  {inv.promoEnabled
                    ? `${inv.promoMaxCount ?? 0} × ${formatUsd(inv.promoMaxValueNano)}`
                    : "—"}
                </td>
                <td>{inv.expiresAt ? formatDate(inv.expiresAt) : "—"}</td>
                <td>
                  {inv.consumedAt ? (
                    <Badge tone="green">Used {formatDate(inv.consumedAt)}</Badge>
                  ) : (
                    <Badge tone="yellow">Unused</Badge>
                  )}
                </td>
                <td>{!inv.consumedAt ? <CopyButton value={inv.inviteUrl} label="Copy" /> : null}</td>
              </tr>
            ))}
          </Table>
        )}
      </Card>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

function PayoutListTab({ adminKey }: { adminKey: string }) {
  const [data, setData] = useState<PayoutListResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api<PayoutListResponse>("/v1/admin/payout-list", { headers: adminHeaders(adminKey) })
      .then(setData)
      .catch((err) => setError(err instanceof ApiError ? err.message : "Failed to load the payout list."));
  }, [adminKey]);

  if (error) return <Notice kind="error">{error}</Notice>;
  if (!data) return <Loading />;

  const eligible = data.items.filter((i) => i.eligible);
  const eligibleTotal = sumCanonicalNanoUsd(eligible.map((i) => i.payableNano));
  const heldTotal = sumCanonicalNanoUsd(data.items.filter((i) => !i.eligible).map((i) => i.payableNano));
  const unpaidTotal = sumCanonicalNanoUsd(data.items.map((i) => i.payableNano));
  const win = data.period;
  const reasonLabel: Record<string, string> = {
    ok: "Ready",
    no_wallet: "No wallet",
    below_minimum: "Below minimum",
    inactive: "Suspended — held",
    zero: "—",
  };
  const dt = (iso: string) => new Date(iso).toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" });

  return (
    <div className="stack">
      <Card
        title={`Payout list — period ${win.key}`}
        sub={`Auto-generated from gross commissions before ${dt(win.end)}, signed refund adjustments and committed payouts. Window ${dt(win.payoutWindowStart)} → ${dt(win.payoutWindowEnd)} · phase: ${win.phase}. Debt is shown separately and future earnings repay it first.`}
      >
        <div className="stat-grid" style={{ gridTemplateColumns: "repeat(3, 1fr)", marginBottom: 0 }}>
          <div className="stat-card">
            <div className="stat-label">Ready to pay</div>
            <div className="stat-value green">{formatUsd(eligibleTotal)}</div>
            <div className="stat-foot">{eligible.length} partners eligible</div>
          </div>
          <div className="stat-card">
            <div className="stat-label">Held (rolls over)</div>
            <div className="stat-value">
              {formatUsd(heldTotal)}
            </div>
            <div className="stat-foot">{data.items.filter((i) => !i.eligible).length} not eligible yet</div>
          </div>
          <div className="stat-card">
            <div className="stat-label">Total payable</div>
            <div className="stat-value">
              {formatUsd(unpaidTotal)}
            </div>
            <div className="stat-foot">{data.items.length} partners with a balance</div>
          </div>
        </div>
      </Card>

      <Card title="Due this window">
        {data.items.length === 0 ? (
          <EmptyState title="Nothing due">No partner has an unpaid balance for this period.</EmptyState>
        ) : (
          <Table
            head={
              <>
                <th>Partner</th>
                <th className="num">Payable</th>
                <th>Wallet (BSC)</th>
                <th>Status</th>
              </>
            }
          >
            {data.items.map((row) => {
              const debtNano = /^\d+$/.test(row.debtNano) ? BigInt(row.debtNano) : 0n;
              return <tr key={row.partnerId}>
                <td>
                  <div style={{ fontWeight: 600 }}>
                    {row.telegramUsername ? `@${row.telegramUsername}` : row.displayName ?? row.partnerId.slice(0, 8)}
                  </div>
                </td>
                <td className="num" style={{ fontWeight: 700 }}>
                  {formatUsd(row.payableNano)}
                  {debtNano > 0n ? <div style={{ color: "#d6455a", fontSize: 11 }}>{formatUsd(row.debtNano)} debt</div> : null}
                </td>
                <td className="mono">{row.walletAddress ? `${row.walletAddress.slice(0, 8)}…${row.walletAddress.slice(-6)}` : "—"}</td>
                <td>
                  {row.eligible ? (
                    <Badge tone="green">Ready</Badge>
                  ) : (
                    <Badge tone="yellow">{reasonLabel[row.reason] ?? row.reason}</Badge>
                  )}
                </td>
              </tr>;
            })}
          </Table>
        )}
      </Card>
      <p className="field-hint">
        This list is the read-only period preview. Prepare, verify, and execute the on-chain batch in
        <strong> Send payouts</strong>; the server revalidates every partner, wallet, amount, balance,
        window, and hot-wallet identity immediately before signing.
      </p>
    </div>
  );
}

type Tab = "overview" | "onboarding" | "partners" | "payoutList" | "payouts" | "send";

export default function AdminPage() {
  const [adminKey, setAdminKey] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const [tab, setTab] = useState<Tab>("overview");

  useEffect(() => {
    (async () => {
      // admin.partners: Caddy инжектит ключ после managed admin auth — пробуем без ключа.
      try {
        await api("/v1/admin/overview");
        setAdminKey("");
        setReady(true);
        return;
      } catch {
        // нет инжекта (прямой доступ) → ключ из сессии или KeyGate
      }
      setAdminKey(sessionStorage.getItem(KEY_STORAGE));
      setReady(true);
    })();
  }, []);

  if (!ready) return <Loading />;
  if (adminKey === null) return <KeyGate onUnlock={setAdminKey} />;

  return (
    <div className="admin-shell">
      <div className="admin-topbar">
        <div className="brand">
          <span>
            APIToken <em>Partners</em>&nbsp;
            <Badge tone="yellow">Admin</Badge>
          </span>
        </div>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            sessionStorage.removeItem(KEY_STORAGE);
            setAdminKey(null);
          }}
        >
          Lock
        </Button>
      </div>

      <div className="tabs" role="tablist">
        {(
          [
            ["overview", "Overview"],
            ["onboarding", "Onboarding"],
            ["partners", "Partners"],
            ["payoutList", "Payout list"],
            ["send", "Send payouts"],
            ["payouts", "Payouts"],
          ] as Array<[Tab, string]>
        ).map(([id, label]) => (
          <button
            key={id}
            role="tab"
            aria-selected={tab === id}
            className={`tab${tab === id ? " active" : ""}`}
            onClick={() => setTab(id)}
          >
            {label}
          </button>
        ))}
      </div>

      {tab === "overview" ? <OverviewTab adminKey={adminKey} /> : null}
      {tab === "onboarding" ? <OnboardingTab adminKey={adminKey} /> : null}
      {tab === "partners" ? <PartnersTab adminKey={adminKey} /> : null}
      {tab === "payoutList" ? <PayoutListTab adminKey={adminKey} /> : null}
      {tab === "send" ? <PayoutSendTab adminKey={adminKey} /> : null}
      {tab === "payouts" ? <PayoutsTab adminKey={adminKey} /> : null}
    </div>
  );
}
