"use client";

import { useCallback, useEffect, useState } from "react";
import {
  api,
  ApiError,
  formatBps,
  formatDate,
  formatUsd,
  type InviteRow,
  type TeamRow,
} from "@/lib/api";
import {
  Badge,
  Button,
  Card,
  CopyButton,
  EmptyState,
  Input,
  Loading,
  Notice,
  StatusBadge,
  Table,
} from "@/components/ui";

export default function TeamPage() {
  const [team, setTeam] = useState<TeamRow[] | null>(null);
  const [invites, setInvites] = useState<InviteRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [newInvite, setNewInvite] = useState<{ code: string; inviteUrl: string } | null>(null);

  const load = useCallback(async () => {
    try {
      const [t, inv] = await Promise.all([
        api<{ items: TeamRow[] }>("/v1/partner/team"),
        api<{ items: InviteRow[] }>("/v1/partner/invites"),
      ]);
      setTeam(t.items);
      setInvites(inv.items);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Failed to load your team.");
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function createInvite() {
    setCreating(true);
    setCreateError(null);
    try {
      const res = await api<{ code: string; inviteUrl: string }>("/v1/partner/invites", {
        method: "POST",
        body: {},
      });
      setNewInvite(res);
      void load();
    } catch (err) {
      setCreateError(
        err instanceof ApiError ? err.message : "Could not create an invite.",
      );
    } finally {
      setCreating(false);
    }
  }

  return (
    <>
      <h1 className="page-title">Team</h1>
      <p className="page-sub">
        Sub-partners you recruited. You earn an override on the spend their
        referrals generate.
      </p>
      {error ? <Notice kind="error">{error}</Notice> : null}

      <div className="stack">
        <Card
          title="Invite a sub-partner"
          sub="Generate a one-time invite link. Whoever registers with it becomes your sub-partner."
        >
          {createError ? <Notice kind="error">{createError}</Notice> : null}
          {newInvite ? (
            <div style={{ marginBottom: 14 }}>
              <div className="reflink-row">
                <Input
                  readOnly
                  value={newInvite.inviteUrl}
                  onFocus={(e) => e.currentTarget.select()}
                />
                <CopyButton value={newInvite.inviteUrl} label="Copy invite" />
              </div>
              <p className="field-hint" style={{ marginTop: 8 }}>
                Invite code: <span className="mono">{newInvite.code}</span>
              </p>
            </div>
          ) : null}
          <Button onClick={createInvite} loading={creating}>
            {newInvite ? "Create another invite" : "Create invite link"}
          </Button>
        </Card>

        <Card title="Your sub-partners">
          {!team ? (
            <Loading />
          ) : team.length === 0 ? (
            <EmptyState title="No sub-partners yet">
              Invite other promoters and earn an override on their results.
            </EmptyState>
          ) : (
            <Table
              head={
                <>
                  <th>Partner</th>
                  <th>Commission</th>
                  <th className="num">Referred users</th>
                  <th className="num">They earned</th>
                  <th className="num">Your override</th>
                  <th>Status</th>
                </>
              }
            >
              {team.map((m) => (
                <tr key={m.id}>
                  <td>
                    <div style={{ fontWeight: 600 }}>{m.displayName || m.email}</div>
                    {m.displayName ? (
                      <div style={{ fontSize: 12, color: "var(--text-faint)" }}>{m.email}</div>
                    ) : null}
                  </td>
                  <td>{formatBps(m.commissionBps)}</td>
                  <td className="num">{m.referredUsers}</td>
                  <td className="num">{formatUsd(m.earnedNano)}</td>
                  <td className="num" style={{ color: "var(--accent-strong)", fontWeight: 700 }}>
                    {formatUsd(m.myOverrideNano)}
                  </td>
                  <td>
                    <StatusBadge status={m.status} />
                  </td>
                </tr>
              ))}
            </Table>
          )}
        </Card>

        <Card title="Invite links" sub="Invites you have created so far.">
          {!invites ? (
            <Loading />
          ) : invites.length === 0 ? (
            <EmptyState title="No invites yet" />
          ) : (
            <Table
              head={
                <>
                  <th>Code</th>
                  <th>Commission</th>
                  <th>Expires</th>
                  <th>Status</th>
                  <th />
                </>
              }
            >
              {invites.map((inv) => (
                <tr key={inv.code}>
                  <td className="mono">{inv.code}</td>
                  <td>{inv.commissionBps != null ? formatBps(inv.commissionBps) : "Program default"}</td>
                  <td>{inv.expiresAt ? formatDate(inv.expiresAt) : "—"}</td>
                  <td>
                    {inv.consumedAt ? (
                      <Badge tone="green">Used {formatDate(inv.consumedAt)}</Badge>
                    ) : (
                      <Badge tone="yellow">Unused</Badge>
                    )}
                  </td>
                  <td>
                    {!inv.consumedAt ? (
                      <CopyButton value={inv.inviteUrl} label="Copy" />
                    ) : null}
                  </td>
                </tr>
              ))}
            </Table>
          )}
        </Card>
      </div>
    </>
  );
}
