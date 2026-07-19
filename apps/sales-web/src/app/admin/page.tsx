"use client";

import { useCallback, useEffect, useState, type FormEvent } from "react";
import {
  api,
  ApiError,
  formatBps,
  formatDate,
  formatUsd,
  type AdminPartnerRow,
  type AdminPayoutRow,
  type InviteRow,
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

const KEY_STORAGE = "sales_admin_key";

function adminHeaders(key: string) {
  return { "x-sales-admin-key": key };
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

function PartnerEditor({
  partner,
  adminKey,
  onSaved,
  onError,
}: {
  partner: AdminPartnerRow;
  adminKey: string;
  onSaved: () => void;
  onError: (msg: string) => void;
}) {
  const [bps, setBps] = useState(String(partner.commissionBps));
  const [subBps, setSubBps] = useState(String(partner.subCommissionBps));
  const [busy, setBusy] = useState(false);

  const dirty =
    bps !== String(partner.commissionBps) || subBps !== String(partner.subCommissionBps);

  async function patch(body: Record<string, unknown>) {
    setBusy(true);
    try {
      await api(`/v1/admin/partners/${partner.id}`, {
        method: "PATCH",
        headers: adminHeaders(adminKey),
        body,
      });
      onSaved();
    } catch (err) {
      onError(err instanceof ApiError ? err.message : "Update failed.");
    } finally {
      setBusy(false);
    }
  }

  const suspended = partner.status.toLowerCase() !== "active";

  return (
    <tr>
      <td>
        <div style={{ fontWeight: 600 }}>
          {partner.telegramUsername ? `@${partner.telegramUsername}` : partner.email ?? "—"}
        </div>
        {partner.displayName ? (
          <div style={{ fontSize: 12, color: "var(--text-faint)" }}>{partner.displayName}</div>
        ) : null}
      </td>
      <td className="mono">{partner.referralCode ?? "—"}</td>
      <td>
        {partner.parentTelegramUsername
          ? `@${partner.parentTelegramUsername}`
          : partner.parentEmail ?? (partner.parentId ? <span className="mono">{partner.parentId}</span> : "—")}
      </td>
      <td>
        <Input
          className="inline-edit"
          inputMode="numeric"
          value={bps}
          onChange={(e) => setBps(e.target.value.replace(/[^\d]/g, ""))}
          aria-label="Commission bps"
        />
        <div style={{ fontSize: 11, color: "var(--text-faint)" }}>
          {/^\d+$/.test(bps) ? formatBps(Number(bps)) : "—"}
        </div>
      </td>
      <td>
        <Input
          className="inline-edit"
          inputMode="numeric"
          value={subBps}
          onChange={(e) => setSubBps(e.target.value.replace(/[^\d]/g, ""))}
          aria-label="Sub-commission bps"
        />
        <div style={{ fontSize: 11, color: "var(--text-faint)" }}>
          {/^\d+$/.test(subBps) ? formatBps(Number(subBps)) : "—"}
        </div>
      </td>
      <td className="num">{partner.earnedNano ? formatUsd(partner.earnedNano) : "—"}</td>
      <td>
        <StatusBadge status={partner.status} />
      </td>
      <td>
        <div className="row-actions">
          <Button
            size="sm"
            variant="ghost"
            disabled={!dirty || busy || !/^\d+$/.test(bps) || !/^\d+$/.test(subBps)}
            onClick={() =>
              patch({ commissionBps: Number(bps), subCommissionBps: Number(subBps) })
            }
          >
            Save
          </Button>
          <Button
            size="sm"
            variant={suspended ? "primary" : "danger"}
            disabled={busy}
            onClick={() => patch({ status: suspended ? "active" : "suspended" })}
          >
            {suspended ? "Activate" : "Suspend"}
          </Button>
        </div>
      </td>
    </tr>
  );
}

function PartnersTab({ adminKey }: { adminKey: string }) {
  const [items, setItems] = useState<AdminPartnerRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const res = await api<{ items: AdminPartnerRow[] }>("/v1/admin/partners", {
        headers: adminHeaders(adminKey),
      });
      setItems(res.items);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Failed to load partners.");
    }
  }, [adminKey]);

  useEffect(() => {
    void load();
  }, [load]);

  if (error) return <Notice kind="error">{error}</Notice>;
  if (!items) return <Loading />;
  if (items.length === 0) return <EmptyState title="No partners yet" />;

  return (
    <Table
      head={
        <>
          <th>Partner</th>
          <th>Code</th>
          <th>Parent</th>
          <th>Bps</th>
          <th>Sub-bps</th>
          <th className="num">Earned</th>
          <th>Status</th>
          <th />
        </>
      }
    >
      {items.map((p) => (
        <PartnerEditor
          key={p.id}
          partner={p}
          adminKey={adminKey}
          onSaved={load}
          onError={setError}
        />
      ))}
    </Table>
  );
}

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
      const qs = filter === "pending" ? "?status=pending" : "";
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

function OnboardingTab({ adminKey }: { adminKey: string }) {
  const [items, setItems] = useState<InviteRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [username, setUsername] = useState("");
  const [bps, setBps] = useState("");
  const [subBps, setSubBps] = useState("");
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
    const clean = username.trim().replace(/^@/, "");
    if (!/^[A-Za-z0-9_]{5,32}$/.test(clean)) {
      setError("Enter the sales partner's Telegram username (5–32 letters, digits, underscore).");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const res = await api<{ inviteUrl: string; telegramUsername: string | null }>("/v1/admin/invites", {
        method: "POST",
        headers: adminHeaders(adminKey),
        body: {
          telegramUsername: clean,
          ...(/^\d+$/.test(bps) ? { commissionBps: Number(bps) } : {}),
          ...(/^\d+$/.test(subBps) ? { subCommissionBps: Number(subBps) } : {}),
        },
      });
      setCreated(res);
      setUsername("");
      setBps("");
      setSubBps("");
      void load();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Could not create the invite.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="stack">
      <ApplicationsCard adminKey={adminKey} />
      <Card
        title="Onboard a sales partner"
        sub="Invite is bound to their Telegram username. Send them the link — they sign in with Telegram and the account is created."
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
        <div className="row-actions" style={{ flexWrap: "wrap", gap: 8 }}>
          <Input
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            placeholder="@telegram_username"
            style={{ maxWidth: 220 }}
          />
          <Input
            value={bps}
            onChange={(e) => setBps(e.target.value.replace(/[^\d]/g, ""))}
            placeholder="bps (default 1000)"
            inputMode="numeric"
            style={{ maxWidth: 160 }}
          />
          <Input
            value={subBps}
            onChange={(e) => setSubBps(e.target.value.replace(/[^\d]/g, ""))}
            placeholder="sub-bps (default 1000)"
            inputMode="numeric"
            style={{ maxWidth: 160 }}
          />
          <Button onClick={create} loading={busy}>
            Create invite
          </Button>
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
                <th>Bps</th>
                <th>Sub-bps</th>
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

type Tab = "overview" | "onboarding" | "partners" | "payouts";

export default function AdminPage() {
  const [adminKey, setAdminKey] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const [tab, setTab] = useState<Tab>("overview");

  useEffect(() => {
    setAdminKey(sessionStorage.getItem(KEY_STORAGE));
    setReady(true);
  }, []);

  if (!ready) return <Loading />;
  if (!adminKey) return <KeyGate onUnlock={setAdminKey} />;

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
      {tab === "payouts" ? <PayoutsTab adminKey={adminKey} /> : null}
    </div>
  );
}
