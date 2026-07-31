"use client";

import Image from "next/image";
import { useState, type FormEvent } from "react";
import { api, type AuthUser, type TotpSetup } from "@/lib/api";
import { useI18n } from "@/components/i18n-provider";
import { trackProductEvent } from "@/lib/product-analytics";
import { CopyButton, PageHeading, localDashboardCopy, useDashboardCopy } from "./shared";

function TwoFactorCard({ user, onUpdated }: { user: AuthUser; onUpdated(user: AuthUser): void }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const localCopy = localDashboardCopy[language];
  const [setup, setSetup] = useState<TotpSetup | null>(null);
  const [code, setCode] = useState("");
  const [disarming, setDisarming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const onCode = (value: string) => { setCode(value.replace(/\D/g, "").slice(0, 6)); setError(null); };
  async function refresh() { const me = await api.me(); onUpdated(me.user); }
  async function beginSetup() {
    setBusy(true); setError(null);
    try { setSetup(await api.totpSetup()); setCode(""); }
    catch (cause) { setError(cause instanceof Error ? cause.message : copy.twoFactorError); }
    finally { setBusy(false); }
  }
  async function confirmEnable() {
    setBusy(true); setError(null);
    try { await api.totpEnable(code); trackProductEvent("Two Factor Enabled"); await refresh(); setSetup(null); setCode(""); }
    catch { setError(copy.twoFactorCodeInvalid); }
    finally { setBusy(false); }
  }
  async function confirmDisable() {
    setBusy(true); setError(null);
    try { await api.totpDisable(code); trackProductEvent("Two Factor Disabled"); await refresh(); setDisarming(false); setCode(""); }
    catch { setError(copy.twoFactorCodeInvalid); }
    finally { setBusy(false); }
  }
  function cancel() { setSetup(null); setDisarming(false); setCode(""); setError(null); }
  const codeRow = (onConfirm: () => void, confirmLabel: string) => <div className="tfa-coderow">
    <input className="set-in tfa-code" name="totp-code" inputMode="numeric" autoComplete="one-time-code" spellCheck={false} maxLength={6} value={code} onChange={(event) => onCode(event.target.value)} placeholder="000000" aria-label={copy.twoFactorCodeLabel} autoFocus />
    <button className="btn btn-ghost btn-sm" disabled={busy} onClick={cancel}>{copy.cancel}</button>
    <button className="btn btn-primary btn-sm" disabled={busy || code.length !== 6} onClick={onConfirm}>{confirmLabel}</button>
  </div>;
  return <div className="card tfa-card">
    <div className="tfa-head"><b>{copy.twoFactorTitle}</b>{user.totpEnabled ? <span className="pill pill-good">{copy.twoFactorOn}</span> : <span className="pill pill-soft">{copy.twoFactorOff}</span>}</div>
    <p className="p-sub tfa-help">{copy.twoFactorGateHelp}</p>
    <p className="p-sub tfa-recovery">{copy.twoFactorRecoveryHelp}</p>
    {user.totpEnabled
      ? (disarming
        ? <><p className="p-sub">{copy.twoFactorDisableHelp}</p>{codeRow(confirmDisable, copy.twoFactorDisable)}</>
        : <button className="btn btn-ghost btn-sm" onClick={() => { setDisarming(true); setError(null); }}>{copy.twoFactorDisable}</button>)
      : (setup
        ? <div className="tfa-enroll">
            <p className="p-sub tfa-scan">{copy.twoFactorScan}</p>
            <div className="tfa-qr"><Image src={setup.qrDataUrl} width={168} height={168} alt={localCopy.twoFactorQrAlt} unoptimized /></div>
            <div className="tfa-secret"><span>{copy.twoFactorManual}</span><code>{setup.secret}</code><CopyButton value={setup.secret} className="tfa-secret-copy" /></div>
            {codeRow(confirmEnable, copy.twoFactorVerify)}
          </div>
        : <button className="btn btn-primary btn-sm" disabled={busy} onClick={beginSetup}>{copy.enable2fa}</button>)}
    {error && <span className="profile-save-error tfa-error" role="alert">{error}</span>}
  </div>;
}

export function Profile({ user, onUpdated }: { user: AuthUser; onUpdated(user: AuthUser): void }) {
  const copy = useDashboardCopy();
  const persistedDisplayName = user.displayName || user.email.split("@")[0];
  const [displayName, setDisplayName] = useState(persistedDisplayName);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const trimmedName = displayName.trim();
  const unchanged = trimmedName === persistedDisplayName;
  async function saveProfile(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!trimmedName || trimmedName.length > 80 || unchanged || saving) return;
    setSaving(true); setSaved(false); setSaveError(null);
    try {
      const result = await api.updateProfile(trimmedName);
      trackProductEvent("Profile Updated");
      onUpdated(result.user); setDisplayName(result.user.displayName); setSaved(true);
      window.setTimeout(() => setSaved(false), 2_000);
    } catch (cause) {
      setSaveError(cause instanceof Error ? cause.message : copy.profileSaveError);
    } finally { setSaving(false); }
  }
  return <section className="panel"><PageHeading eyebrow={copy.navAccount} title={copy.profileTitle} subtitle={copy.profileSubtitle} /><div className="prof-grid"><form className="card" onSubmit={saveProfile}><h2>{copy.accountDetails}</h2><div className="set-row"><label className="set-l" htmlFor="profile-email">{copy.email}</label><input id="profile-email" className="set-in profile-email-input" title={user.email} value={user.email} disabled readOnly /></div><div className="set-row"><label className="set-l" htmlFor="profile-display-name">{copy.displayName}</label><input id="profile-display-name" className="set-in" value={displayName} maxLength={80} autoComplete="name" onChange={(event) => { setDisplayName(event.target.value); setSaved(false); setSaveError(null); }} /></div><div className="set-row profile-id-row"><span className="set-l">{copy.userId}</span><span className="uid-wrap"><input className="set-in" value={user.id} aria-label={copy.userId} disabled readOnly /><CopyButton value={user.id} className="uid-copy-button" /></span></div><p className="p-sub">{copy.supportId}</p><div className="profile-meta"><span className="pill">{user.customerType.toUpperCase()}</span><span className="pill pill-soft">Email {user.emailVerified ? copy.verified : copy.pending}</span></div><div className="prof-save"><button className="btn btn-primary btn-sm" type="submit" disabled={saving || unchanged || trimmedName.length === 0}>{saving ? copy.saving : copy.save}</button>{saved && <span className="set-saved always-visible profile-save-success" role="status">{copy.profileSaved}</span>}{saveError && <span className="profile-save-error" role="alert">{saveError}</span>}</div></form>
    <div className="prof-side"><TwoFactorCard user={user} onUpdated={onUpdated} /></div></div>
  </section>;
}
