"use client";

import { useState, type FormEvent } from "react";
import { api, ApiError, formatBps } from "@/lib/api";
import { usePartner } from "@/components/partner-context";
import {
  Button,
  Card,
  Field,
  Input,
  Notice,
  Select,
  StatusBadge,
  Textarea,
} from "@/components/ui";

const METHODS = [
  { value: "", label: "Not set" },
  { value: "usdt-trc20", label: "USDT (TRC-20)" },
  { value: "card", label: "Card" },
  { value: "other", label: "Other" },
];

export default function SettingsPage() {
  const partner = usePartner();
  const [displayName, setDisplayName] = useState(partner.displayName ?? "");
  const [payoutMethod, setPayoutMethod] = useState("");
  const [payoutDetails, setPayoutDetails] = useState("");
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
        body: {
          displayName: displayName.trim() || undefined,
          payoutMethod: payoutMethod || undefined,
          payoutDetails: payoutDetails.trim() || undefined,
        },
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
      <p className="page-sub">Your profile and payout defaults.</p>

      <div className="stack">
        <Card title="Profile">
          {error ? <Notice kind="error">{error}</Notice> : null}
          {saved ? <Notice kind="success">Settings saved.</Notice> : null}
          <form onSubmit={onSubmit}>
            <Field label="Email">
              <Input value={partner.email} readOnly disabled />
            </Field>
            <Field label="Display name">
              <Input
                value={displayName}
                onChange={(e) => setDisplayName(e.target.value)}
                placeholder="How we should address you"
              />
            </Field>
            <div className="grid-2">
              <Field label="Default payout method">
                <Select
                  value={payoutMethod}
                  onChange={(e) => setPayoutMethod(e.target.value)}
                >
                  {METHODS.map((m) => (
                    <option key={m.value} value={m.value}>
                      {m.label}
                    </option>
                  ))}
                </Select>
              </Field>
              <Field label="Default payout details">
                <Textarea
                  value={payoutDetails}
                  onChange={(e) => setPayoutDetails(e.target.value)}
                  placeholder="Wallet address / card number"
                  style={{ minHeight: 44 }}
                />
              </Field>
            </div>
            <Button type="submit" loading={busy}>
              Save changes
            </Button>
          </form>
        </Card>

        <Card
          title="Your commission terms"
          sub="Set individually by the program. Contact the team to discuss an upgrade."
        >
          <div className="stat-grid" style={{ gridTemplateColumns: "repeat(3, 1fr)", marginBottom: 0 }}>
            <div className="stat-card">
              <div className="stat-label">Direct commission</div>
              <div className="stat-value green">{formatBps(partner.commissionBps)}</div>
              <div className="stat-foot">of your referrals&apos; spend</div>
            </div>
            <div className="stat-card">
              <div className="stat-label">Team override</div>
              <div className="stat-value">{formatBps(partner.subCommissionBps)}</div>
              <div className="stat-foot">of your sub-partners&apos; volume</div>
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
