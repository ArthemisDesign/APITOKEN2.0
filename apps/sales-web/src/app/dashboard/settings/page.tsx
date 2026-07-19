"use client";

// Только профиль и условия. Всё платёжное (кошелёк, выплаты) живёт в Payouts.

import { useState, type FormEvent } from "react";
import { api, ApiError, formatBps } from "@/lib/api";
import { usePartner } from "@/components/partner-context";
import { Button, Card, Field, Input, Notice, StatusBadge } from "@/components/ui";

export default function SettingsPage() {
  const partner = usePartner();
  const [displayName, setDisplayName] = useState(partner.displayName ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setSaved(false);
    setBusy(true);
    try {
      await api("/v1/partner/settings", {
        method: "PATCH",
        body: { displayName: displayName.trim() || undefined },
      });
      setSaved(true);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Could not save settings.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <h1 className="page-title">Settings</h1>
      <p className="page-sub">Your profile and commission terms.</p>

      <div className="stack">
        <Card title="Profile">
          {error ? <Notice kind="error">{error}</Notice> : null}
          {saved ? <Notice kind="success">Settings saved.</Notice> : null}
          <form onSubmit={onSubmit}>
            <Field label="Telegram">
              <Input
                value={partner.telegramUsername ? `@${partner.telegramUsername}` : partner.email ?? "—"}
                readOnly
                disabled
              />
            </Field>
            <Field label="Display name">
              <Input
                value={displayName}
                onChange={(e) => setDisplayName(e.target.value)}
                placeholder="How we should address you"
              />
            </Field>
            <Button type="submit" loading={busy}>
              Save changes
            </Button>
          </form>
        </Card>

        <Card
          title="Your commission terms"
          sub="Set individually by the program. Contact us to discuss an upgrade."
        >
          <div className="stat-grid" style={{ gridTemplateColumns: "repeat(2, 1fr)", marginBottom: 0 }}>
            <div className="stat-card">
              <div className="stat-label">Direct commission</div>
              <div className="stat-value green">{formatBps(partner.commissionBps)}</div>
              <div className="stat-foot">of your referrals&apos; spend</div>
            </div>
            <div className="stat-card">
              <div className="stat-label">Account status</div>
              <div className="stat-value" style={{ fontSize: 16, paddingTop: 6 }}>
                <StatusBadge status={partner.status} />
              </div>
            </div>
          </div>
        </Card>
      </div>
    </>
  );
}
