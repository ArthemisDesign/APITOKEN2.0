"use client";

import Link from "next/link";
import { useDeferredValue, useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { api, ApiError, type ApiKeyView, type AuthUser } from "@/lib/api";
import { useI18n } from "@/components/i18n-provider";
import { DOCS_URL } from "@/lib/site-links";
import { trackFirstProductEvent, trackProductEvent } from "@/lib/product-analytics";
import {
  BASIS_POINTS, CopyButton, NANO_PER_USD, PageHeading,
  compareBigInt, formatNanoUsd, interpolate, localDashboardCopy, useDashboardCopy,
} from "./shared";

type KeyStatusFilter = "current" | "working" | "attention" | "disabled" | "all";

export function ApiKeys({ keys, onChanged, user }: { keys: ApiKeyView[]; onChanged(): Promise<void>; user: AuthUser }) {
  const copy = useDashboardCopy();
  const { language } = useI18n();
  const localCopy = localDashboardCopy[language];
  const locale = language === "ru" ? "ru-RU" : "en-US";
  const [issued, setIssued] = useState<string | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [label, setLabel] = useState("");
  const [spendLimit, setSpendLimit] = useState("");
  const [expirationDate, setExpirationDate] = useState("");
  const [totpCode, setTotpCode] = useState("");
  const [filter, setFilter] = useState<KeyStatusFilter>("current");
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<"newest" | "name" | "spend" | "last-used">("newest");
  const [editTarget, setEditTarget] = useState<ApiKeyView | null>(null);
  const [editLabel, setEditLabel] = useState("");
  const [policySpendLimit, setPolicySpendLimit] = useState("");
  const [policyExpirationDate, setPolicyExpirationDate] = useState("");
  const [policyTotpCode, setPolicyTotpCode] = useState("");
  const [revokeTarget, setRevokeTarget] = useState<ApiKeyView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [policyNow, setPolicyNow] = useState(() => Date.now());
  const createTriggerRef = useRef<HTMLButtonElement>(null);
  const createModalRef = useRef<HTMLFormElement>(null);
  const editModalRef = useRef<HTMLFormElement>(null);
  const revokeModalRef = useRef<HTMLDivElement>(null);
  const keysPanelRef = useRef<HTMLElement>(null);
  const dialogReturnFocusRef = useRef<HTMLElement | null>(null);
  const busyRef = useRef(busy);

  useEffect(() => {
    const interval = window.setInterval(() => setPolicyNow(Date.now()), 60_000);
    return () => window.clearInterval(interval);
  }, []);

  useEffect(() => {
    const panel = keysPanelRef.current;
    if (!panel) return;

    const closeMenusExcept = (currentMenu: HTMLDetailsElement | null = null) => {
      panel.querySelectorAll<HTMLDetailsElement>(".key-menu[open]").forEach((menu) => {
        if (menu !== currentMenu) menu.removeAttribute("open");
      });
    };
    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target instanceof Element ? event.target : null;
      const menu = target?.closest(".key-menu");
      closeMenusExcept(menu instanceof HTMLDetailsElement ? menu : null);
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      const menu = panel.querySelector<HTMLDetailsElement>(".key-menu[open]");
      if (!menu) return;
      event.preventDefault();
      menu.removeAttribute("open");
      menu.querySelector<HTMLElement>("summary")?.focus();
    };

    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, []);

  useEffect(() => { busyRef.current = busy; }, [busy]);

  useEffect(() => {
    const modal = createOpen ? createModalRef.current : editTarget ? editModalRef.current : revokeTarget ? revokeModalRef.current : null;
    if (!modal) return;
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    const focusableSelector = "button:not([disabled]),a[href],input:not([disabled]),select:not([disabled]),[tabindex]:not([tabindex='-1'])";
    const focusFirst = () => (modal.querySelector<HTMLElement>("[autofocus]") ?? modal.querySelector<HTMLElement>(focusableSelector) ?? modal).focus();
    const frame = window.requestAnimationFrame(focusFirst);
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        if (busyRef.current) return;
        setError(null);
        if (createOpen) {
          setCreateOpen(false);
          window.requestAnimationFrame(() => createTriggerRef.current?.focus());
        } else {
          if (editTarget) {
            setEditTarget(null); setEditLabel(""); setPolicySpendLimit(""); setPolicyExpirationDate(""); setPolicyTotpCode("");
          } else {
            setRevokeTarget(null);
          }
          const returnTarget = dialogReturnFocusRef.current;
          window.requestAnimationFrame(() => returnTarget?.focus());
        }
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = [...modal.querySelectorAll<HTMLElement>(focusableSelector)];
      if (focusable.length === 0) { event.preventDefault(); modal.focus(); return; }
      const first = focusable[0]!, last = focusable.at(-1)!;
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      window.cancelAnimationFrame(frame);
      document.removeEventListener("keydown", onKeyDown);
      document.body.style.overflow = previousOverflow;
    };
  }, [createOpen, editTarget, revokeTarget]);

  function closeCreate() {
    if (busy) return;
    setCreateOpen(false); setError(null);
    window.requestAnimationFrame(() => createTriggerRef.current?.focus());
  }

  function openEdit(key: ApiKeyView, returnTarget?: HTMLElement | null) {
    keysPanelRef.current?.querySelectorAll<HTMLDetailsElement>(".key-menu[open]").forEach((menu) => menu.removeAttribute("open"));
    dialogReturnFocusRef.current = returnTarget ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
    setEditLabel(key.label ?? "");
    setPolicySpendLimit(key.spendLimitNano ? nanoToUsdInput(key.spendLimitNano) : "");
    setPolicyExpirationDate(key.expiresAt ? isoToLocalDateInput(key.expiresAt) : "");
    setPolicyTotpCode(""); setError(null); setEditTarget(key);
  }

  function closeEdit() {
    if (busy) return;
    setEditTarget(null); setEditLabel(""); setPolicySpendLimit(""); setPolicyExpirationDate(""); setPolicyTotpCode(""); setError(null);
    const returnTarget = dialogReturnFocusRef.current;
    window.requestAnimationFrame(() => returnTarget?.focus());
  }

  function openRevoke(key: ApiKeyView, returnTarget?: HTMLElement | null) {
    dialogReturnFocusRef.current = returnTarget ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
    setError(null); setRevokeTarget(key);
  }

  function closeRevoke() {
    if (busy) return;
    setRevokeTarget(null); setError(null);
    const returnTarget = dialogReturnFocusRef.current;
    window.requestAnimationFrame(() => returnTarget?.focus());
  }

  async function create(event: FormEvent) {
    event.preventDefault();
    if (user.totpEnabled && !/^\d{6}$/.test(totpCode)) { setError(copy.twoFactorCodeRequired); return; }
    const trimmedLimit = spendLimit.trim();
    if (trimmedLimit &&
        (!/^(?:0\.\d{1,2}|[1-9]\d*(?:\.\d{1,2})?)$/.test(trimmedLimit) || Number(trimmedLimit) <= 0)) {
      setError(localCopy.invalidSpendLimit); return;
    }
    let expiresAt: string | undefined;
    if (expirationDate) {
      const date = new Date(`${expirationDate}T23:59:59.999`);
      if (!Number.isFinite(date.getTime()) || date.getTime() <= Date.now()) {
        setError(localCopy.invalidExpiration); return;
      }
      expiresAt = date.toISOString();
    }
    setBusy(true); setError(null);
    try {
      const created = await api.createApiKey({
        ...(label.trim() ? { label: label.trim() } : {}),
        ...(trimmedLimit ? { spendLimitUsd: trimmedLimit } : {}),
        ...(expiresAt ? { expiresAt } : {}),
        ...(user.totpEnabled ? { totpCode } : {}),
      });
      trackProductEvent("API Key Created", {
        has_label: Boolean(label.trim()), has_limit: Boolean(trimmedLimit), has_expiration: Boolean(expiresAt),
        two_factor: user.totpEnabled,
      });
      trackFirstProductEvent("api_key", "First API Key Created", { two_factor: user.totpEnabled });
      setIssued(created.key ?? null);
      setLabel(""); setSpendLimit(""); setExpirationDate(""); setTotpCode(""); setCreateOpen(false);
      await onChanged();
    } catch (cause) {
      const message = cause instanceof ApiError && (cause.message === "2fa_required" || cause.message === "2fa_invalid")
        ? copy.twoFactorCodeInvalid
        : cause instanceof Error ? cause.message : copy.createKeyError;
      setError(message);
    } finally { setBusy(false); }
  }

  async function updateKey(event: FormEvent) {
    event.preventDefault();
    if (!editTarget) return;
    const nextLabel = editLabel.trim();
    const currentLabel = (editTarget.label ?? "").trim();
    const labelChanged = nextLabel !== currentLabel;
    const guardrailsChanged = editTarget.status === "active" && (
      policySpendLimit.trim() !== (editTarget.spendLimitNano ? nanoToUsdInput(editTarget.spendLimitNano) : "") ||
      policyExpirationDate !== (editTarget.expiresAt ? isoToLocalDateInput(editTarget.expiresAt) : "")
    );
    if (labelChanged && !nextLabel) {
      setError(localCopy.labelRequired); return;
    }
    if (guardrailsChanged && user.totpEnabled && !/^\d{6}$/.test(policyTotpCode)) {
      setError(copy.twoFactorCodeRequired); return;
    }
    const trimmedLimit = policySpendLimit.trim();
    if (guardrailsChanged && trimmedLimit && !/^(?:0\.\d{1,9}|[1-9]\d*(?:\.\d{1,9})?)$/.test(trimmedLimit)) {
      setError(localCopy.invalidPolicySpendLimit); return;
    }
    const proposedNano = trimmedLimit ? usdInputToNano(trimmedLimit) : null;
    if (guardrailsChanged && proposedNano !== null && proposedNano <= 0n) {
      setError(localCopy.invalidPolicySpendLimit); return;
    }
    const committedNano = BigInt(editTarget.spentNano) + BigInt(editTarget.reservedNano ?? "0");
    if (guardrailsChanged && proposedNano !== null && proposedNano < committedNano) {
      setError(interpolate(localCopy.policyBelowCommitted, { amount: formatNanoUsd(committedNano.toString(), locale) }));
      return;
    }
    let expiresAt: string | null = null;
    if (guardrailsChanged && policyExpirationDate) {
      const date = new Date(`${policyExpirationDate}T23:59:59.999`);
      if (!Number.isFinite(date.getTime()) || date.getTime() <= Date.now()) {
        setError(localCopy.invalidExpiration); return;
      }
      expiresAt = date.toISOString();
    }
    setBusy(true); setError(null);
    try {
      // Guardrails and rename hit independent endpoints — send them together.
      await Promise.all([
        guardrailsChanged
          ? api.updateApiKeyPolicy(editTarget.id, {
              spendLimitUsd: trimmedLimit || null,
              expiresAt,
              ...(user.totpEnabled ? { totpCode: policyTotpCode } : {}),
            })
          : Promise.resolve(),
        labelChanged ? api.renameApiKey(editTarget.id, nextLabel) : Promise.resolve(),
      ]);
      if (guardrailsChanged) {
        trackProductEvent("API Key Policy Updated", {
          limit: trimmedLimit ? "set" : "cleared",
          expiration: expiresAt ? "set" : "cleared",
          two_factor: user.totpEnabled,
        });
      }
      if (labelChanged) trackProductEvent("API Key Renamed");
      await onChanged();
      setEditTarget(null); setEditLabel(""); setPolicySpendLimit(""); setPolicyExpirationDate(""); setPolicyTotpCode("");
      const returnTarget = dialogReturnFocusRef.current;
      window.requestAnimationFrame(() => returnTarget?.focus());
    } catch (cause) {
      // With parallel requests one change may have been saved while the other failed;
      // resync with the server so the table behind the dialog shows the real key state.
      try { await onChanged(); } catch { /* the update error below is what matters */ }
      const message = cause instanceof ApiError && (cause.message === "2fa_required" || cause.message === "2fa_invalid")
        ? copy.twoFactorCodeInvalid
        : cause instanceof ApiError && cause.status === 409
          ? interpolate(localCopy.policyBelowCommitted, { amount: formatNanoUsd(committedNano.toString(), locale) })
          : cause instanceof Error ? cause.message : localCopy.updateKeyError;
      setError(message);
    } finally { setBusy(false); }
  }

  async function revoke() {
    if (!revokeTarget) return;
    setBusy(true); setError(null);
    try {
      await api.revokeApiKey(revokeTarget.id); trackProductEvent("API Key Revoked");
      setRevokeTarget(null); await onChanged();
      const returnTarget = dialogReturnFocusRef.current;
      window.requestAnimationFrame(() => returnTarget?.focus());
    } catch (cause) { setError(cause instanceof Error ? cause.message : copy.revokeKeyError); }
    finally { setBusy(false); }
  }

  // keyPolicy is the expensive part (Date.parse + BigInt per key) — compute it once per
  // keys/policyNow change and reuse the result for the counts, the list and every row.
  const keysWithPolicy = useMemo(() => keys.map((key) => ({ key, policy: keyPolicy(key, policyNow) })), [keys, policyNow]);
  const counts = useMemo(() => {
    const result: Record<KeyStatusFilter, number> = { current: 0, working: 0, attention: 0, disabled: 0, all: keysWithPolicy.length };
    for (const { key, policy } of keysWithPolicy) {
      if (key.status === "active") {
        result.current += 1;
        if (!policy.expired && !policy.limitReached) result.working += 1;
        if (policy.expired || policy.expiresSoon || policy.limitReached || policy.nearLimit) result.attention += 1;
      } else if (key.status === "disabled") result.disabled += 1;
    }
    return result;
  }, [keysWithPolicy]);
  const deferredSearch = useDeferredValue(search);
  const query = deferredSearch.trim().toLocaleLowerCase(locale);
  const sortedKeys = useMemo(() => keysWithPolicy
    .filter(({ key, policy }) => {
      if (filter === "current") return key.status === "active";
      if (filter === "working") return key.status === "active" && !policy.expired && !policy.limitReached;
      if (filter === "attention") return key.status === "active" && (policy.expired || policy.expiresSoon || policy.limitReached || policy.nearLimit);
      if (filter === "disabled") return key.status === "disabled";
      return true;
    })
    .filter(({ key }) => !query || (key.label ?? copy.unlabelledKey).toLocaleLowerCase(locale).includes(query) || key.keyMasked.toLocaleLowerCase(locale).includes(query))
    .sort((left, right) => {
      if (sort === "name") return (left.key.label ?? copy.unlabelledKey).localeCompare(right.key.label ?? copy.unlabelledKey, locale);
      if (sort === "spend") return compareBigInt(BigInt(right.key.spentNano), BigInt(left.key.spentNano));
      if (sort === "last-used") return Date.parse(right.key.lastUsedAt ?? "1970-01-01") - Date.parse(left.key.lastUsedAt ?? "1970-01-01");
      return Date.parse(right.key.createdAt) - Date.parse(left.key.createdAt);
    }), [keysWithPolicy, filter, query, sort, locale, copy.unlabelledKey]);
  const emptyMessage = query
    ? localCopy.noSearchResults
    : filter === "current" ? localCopy.noActiveKeys
      : filter === "working" ? localCopy.noWorkingKeys
        : filter === "attention" ? localCopy.noAttentionKeys
          : filter === "disabled" ? localCopy.noDisabledKeys
            : copy.noKeys;
  const todayDate = new Date(policyNow);
  const today = new Date(todayDate.getTime() - todayDate.getTimezoneOffset() * 60_000).toISOString().slice(0, 10);
  const labelDirty = Boolean(editTarget) && editLabel.trim() !== (editTarget?.label ?? "").trim();
  const policyDirty = editTarget?.status === "active" && (
    policySpendLimit.trim() !== (editTarget.spendLimitNano ? nanoToUsdInput(editTarget.spendLimitNano) : "") ||
    policyExpirationDate !== (editTarget.expiresAt ? isoToLocalDateInput(editTarget.expiresAt) : "")
  );
  const editDirty = labelDirty || policyDirty;
  const editTargetState = editTarget ? keyPolicy(editTarget, policyNow) : null;
  const policyCommittedNano = editTarget
    ? (BigInt(editTarget.spentNano) + BigInt(editTarget.reservedNano ?? "0")).toString()
    : "0";
  return <section ref={keysPanelRef} className="panel keys-panel">
    <div className="keys-heading-row"><PageHeading eyebrow={copy.keysEyebrow} title={copy.keysTitle} subtitle={copy.keysSubtitle} /></div>
    {issued && <section className="agent-key-reveal secret-card key-issued-reveal" aria-live="polite">
      <div className="agent-key-reveal-head"><div><strong>{copy.copyNewKeyNow}</strong><span>{copy.rawSecretWarning}</span></div><span className="chip">{copy.shownOnce}</span></div>
      <div className="secret-key-field"><code>{issued}</code><CopyButton value={issued} className="secret-copy" /></div>
      <button type="button" className="btn btn-ghost btn-sm" onClick={() => setIssued(null)}>{copy.savedKey}</button>
    </section>}
    {error && !createOpen && !editTarget && !revokeTarget && <div className="banner banner-error" role="alert">{error}</div>}

    <section className="dsec keys-manager" aria-label={copy.keysTitle}>
      <div className="keys-manager-head">
        <div className="keys-manager-title"><h2>{localCopy.keysListTitle}</h2></div>
        <div className="keys-manager-actions">
          <span className="keys-manager-summary">{interpolate(localCopy.keysListSummary, { shown: sortedKeys.length, total: keys.length })}</span>
          <button ref={createTriggerRef} className="btn btn-primary keys-create-button" type="button" onClick={() => { setCreateOpen(true); setError(null); }}>＋ {localCopy.createKey}</button>
        </div>
      </div>
      <div className="keys-toolbar">
        <label className="keys-search"><span aria-hidden="true">⌕</span><input name="key-search" autoComplete="off" spellCheck={false} value={search} onChange={(event) => setSearch(event.target.value)} placeholder={localCopy.searchKeys} aria-label={localCopy.searchKeys} /></label>
        <div className="keys-toolbar-right">
          <label className="keys-sort"><span>{localCopy.sortBy}</span><select value={sort} onChange={(event) => setSort(event.target.value as typeof sort)}><option value="newest">{localCopy.sortNewest}</option><option value="name">{localCopy.sortName}</option><option value="spend">{localCopy.sortSpend}</option><option value="last-used">{localCopy.sortLastUsed}</option></select></label>
          <div className="keys-filter-tabs" role="group" aria-label={localCopy.filterLabel}>
            {(["current", "working", "attention", "disabled", "all"] as const).map((status) => <button key={status} type="button" data-key-filter={status} className={`keys-filter-tab ${filter === status ? "on" : ""}`} aria-pressed={filter === status} onClick={() => setFilter(status)}><span>{status === "current" ? localCopy.currentFilter : status === "working" ? localCopy.workingFilter : status === "attention" ? localCopy.attentionFilter : status === "disabled" ? localCopy.disabledFilter : localCopy.allFilter}</span><b>{counts[status]}</b></button>)}
          </div>
        </div>
      </div>

      <div className="key-table-wrap"><table className="key-table">
        <thead><tr><th>{localCopy.colName}</th><th>{localCopy.colKey}</th><th>{localCopy.colSpend}</th><th>{localCopy.colExpires}</th><th>{localCopy.colStatus}</th><th><span className="sr-only">{localCopy.colActions}</span></th></tr></thead>
        <tbody>{sortedKeys.length === 0 ? <tr><td colSpan={6} className="empty-cell"><div className="keys-empty"><strong>{emptyMessage}</strong>{keys.length === 0 ? <button type="button" className="btn btn-primary btn-sm" onClick={() => { setCreateOpen(true); setError(null); }}>{localCopy.createFirstKey}</button> : search.trim() ? <button type="button" className="btn btn-ghost btn-sm" onClick={() => setSearch("")}>{localCopy.clearSearch}</button> : filter !== "current" ? <button type="button" className="btn btn-ghost btn-sm" onClick={() => setFilter("current")}>{localCopy.viewCurrentKeys}</button> : null}</div></td></tr> : sortedKeys.map(({ key, policy }) => {
          const health = policy.health;
          const statusText = key.status === "disabled"
            ? localCopy.disabledStatus
            : health === "expired" ? localCopy.expiredStatus
              : health === "limit" ? localCopy.limitReachedStatus
                : health === "expires-soon" ? localCopy.expiresSoonStatus
                  : health === "near-limit" ? localCopy.nearLimitStatus
                    : localCopy.activeStatus;
          const committed = BigInt(key.spentNano) + BigInt(key.reservedNano ?? "0");
          const limit = key.spendLimitNano ? BigInt(key.spendLimitNano) : null;
          const usageBasisPoints = limit !== null && limit > 0n ? (committed * BASIS_POINTS) / limit : 0n;
          const usagePercent = Number(usageBasisPoints > BASIS_POINTS ? BASIS_POINTS : usageBasisPoints) / 100;
          const usageText = limit
            ? interpolate(localCopy.spentOfLimit, { spent: formatNanoUsd(committed, locale), limit: formatNanoUsd(limit, locale) })
            : interpolate(localCopy.spentWithoutLimit, { spent: formatNanoUsd(committed, locale) });
          const lastUsedText = `${localCopy.colLastUsed}: ${key.lastUsedAt ? formatRelativeDate(key.lastUsedAt, language) : localCopy.neverUsed}`;
          return <tr key={key.id} className={`key-row key-row-${health}`}>
            <td data-label={localCopy.colName} className="key-name-cell"><strong>{key.label || copy.unlabelledKey}</strong><span>{interpolate(localCopy.createdOn, { date: new Date(key.createdAt).toLocaleDateString(locale) })}</span></td>
            <td data-label={localCopy.colKey} className="key-credential-cell"><code className="key-mask">{key.keyMasked}</code><span className="key-credential-lastused" title={lastUsedText}>{lastUsedText}</span></td>
            <td data-label={localCopy.colSpend} className="key-usage-cell"><div><strong>{formatNanoUsd(committed, locale)}</strong><span>{limit !== null ? `/ ${formatNanoUsd(limit, locale)}` : localCopy.unlimited}</span></div>{limit !== null && limit > 0n && <span className={`key-usage-track${policy.limitReached || policy.nearLimit ? " warn" : ""}`} role="progressbar" aria-label={usageText} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(usagePercent)}><i style={{ width: `${usagePercent}%` }} /></span>}<small>{usageText}</small></td>
            <td data-label={localCopy.colExpires} className="key-guardrail-cell"><span className={policy.expired || policy.expiresSoon ? "key-policy-warn" : ""}><em>{key.expiresAt ? new Date(key.expiresAt).toLocaleDateString(locale) : localCopy.never}</em></span></td>
            <td data-label={localCopy.colStatus}><span className={`key-status key-status-${health}`}><i aria-hidden="true" />{statusText}</span></td>
            <td data-label={localCopy.colActions} className="key-actions-cell"><div className="key-actions"><button type="button" className="key-edit-action" data-key-action="edit" disabled={busy} onClick={(event) => openEdit(key, event.currentTarget)}>{localCopy.editKey}</button><details className="key-menu"><summary aria-label={`${localCopy.moreActions}: ${key.label || copy.unlabelledKey}`}>•••</summary><div className="key-menu-pop"><Link href={DOCS_URL} target="_blank" rel="noreferrer">{localCopy.openDocs} ↗</Link>{key.status === "active" && <button type="button" className="danger" disabled={busy} onClick={(event) => { const details = event.currentTarget.closest("details"); const summary = details?.querySelector<HTMLElement>("summary"); details?.removeAttribute("open"); openRevoke(key, summary); }}>{localCopy.revokeKey}</button>}</div></details></div></td>
          </tr>;
        })}</tbody>
      </table></div>

    </section>

    {createOpen && <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.currentTarget === event.target) closeCreate(); }}><form ref={createModalRef} className="key-modal" role="dialog" aria-modal="true" aria-labelledby="create-key-title" aria-describedby="create-key-description" tabIndex={-1} onSubmit={create}>
      <div className="key-modal-head"><div><span className="eyebrow">{copy.keysEyebrow}</span><h2 id="create-key-title">{localCopy.createKeyTitle}</h2><p id="create-key-description">{localCopy.createKeyHelp}</p></div><button type="button" className="key-modal-close" onClick={closeCreate} aria-label={localCopy.cancel}>×</button></div>
      <div className="key-modal-fields">
        <label className="key-field key-field-wide"><span>{localCopy.keyName} <small>{localCopy.optional}</small></span><input className="set-in" name="key-label" autoComplete="off" spellCheck={false} value={label} onChange={(event) => { setLabel(event.target.value); setError(null); }} maxLength={64} placeholder={localCopy.keyNameHint} autoFocus /><em>{localCopy.keyNameHelp}</em></label>
        <fieldset className="key-create-guardrails"><legend>{localCopy.guardrailsTitle} <small>{localCopy.optional}</small></legend><p>{localCopy.guardrailsHelp}</p><div className="key-create-guardrail-grid">
          <label className="key-field"><span>{localCopy.spendLimit}</span><div className="key-money-field"><b>$</b><input className="set-in" name="key-spend-limit" autoComplete="off" spellCheck={false} inputMode="decimal" value={spendLimit} onChange={(event) => { setSpendLimit(event.target.value); setError(null); }} placeholder="100.00" /></div><em>{localCopy.spendLimitHint}</em></label>
          <label className="key-field"><span>{localCopy.expiration}</span><input className="set-in" type="date" min={today} value={expirationDate} onChange={(event) => { setExpirationDate(event.target.value); setError(null); }} /><em>{expirationDate ? localCopy.expirationHint : localCopy.noExpiration}</em></label>
        </div></fieldset>
        {user.totpEnabled && <label className="key-field key-field-wide"><span>{copy.twoFactorCodeLabel}</span><input className="set-in tfa-code" name="totp-code" inputMode="numeric" autoComplete="one-time-code" spellCheck={false} maxLength={6} value={totpCode} onChange={(event) => { setTotpCode(event.target.value.replace(/\D/g, "").slice(0, 6)); setError(null); }} placeholder={copy.twoFactorCodePlaceholder} /></label>}
      </div>
      {error && <div className="banner banner-error" role="alert">{error}</div>}
      <div className="key-modal-actions"><button type="button" className="btn btn-ghost" disabled={busy} onClick={closeCreate}>{localCopy.cancel}</button><button className="btn btn-primary" disabled={busy}>{busy ? localCopy.creating : localCopy.createKey}</button></div>
    </form></div>}

    {editTarget && <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.currentTarget === event.target) closeEdit(); }}><form ref={editModalRef} className="key-modal key-edit-modal" role="dialog" aria-modal="true" aria-labelledby="edit-key-title" aria-describedby="edit-key-description" tabIndex={-1} onSubmit={updateKey}>
      <div className="key-modal-head"><div><span className="eyebrow">{localCopy.editKey}</span><h2 id="edit-key-title">{localCopy.editKeyTitle}</h2><p id="edit-key-description">{localCopy.editKeyHelp}</p></div><button type="button" className="key-modal-close" disabled={busy} onClick={closeEdit} aria-label={localCopy.cancel}>×</button></div>
      <div className="key-policy-summary"><div><span>{localCopy.colKey}</span><code>{editTarget.keyMasked}</code></div><div><span>{localCopy.committedSpend}</span><b>{formatNanoUsd(policyCommittedNano, locale)}</b></div></div>
      {(editTargetState?.expired || editTargetState?.limitReached) && <p className="key-policy-reactivate"><span aria-hidden="true">ⓘ</span>{localCopy.policyReactivates}</p>}
      <div className="key-modal-fields">
        <label className="key-field key-field-wide"><span>{localCopy.keyName}</span><input className="set-in" name="key-label" autoComplete="off" spellCheck={false} value={editLabel} onChange={(event) => { setEditLabel(event.target.value); setError(null); }} maxLength={64} placeholder={localCopy.keyNameHint} autoFocus /></label>
        {editTarget.status === "active" && <><label className="key-field"><span>{localCopy.spendLimit}</span><div className="key-money-field"><b>$</b><input className="set-in" name="key-policy-spend-limit" autoComplete="off" spellCheck={false} inputMode="decimal" value={policySpendLimit} onChange={(event) => { setPolicySpendLimit(event.target.value); setError(null); }} placeholder={localCopy.unlimited} /></div><em>{localCopy.policyLimitHint}</em></label>
        <label className="key-field"><span>{localCopy.expiration}</span><input className="set-in" type="date" min={today} value={policyExpirationDate} onChange={(event) => { setPolicyExpirationDate(event.target.value); setError(null); }} /><em>{localCopy.policyExpirationHint}</em></label>
        {policyDirty && user.totpEnabled && <label className="key-field key-field-wide"><span>{copy.twoFactorCodeLabel}</span><input className="set-in tfa-code" name="totp-code" inputMode="numeric" autoComplete="one-time-code" spellCheck={false} maxLength={6} value={policyTotpCode} onChange={(event) => { setPolicyTotpCode(event.target.value.replace(/\D/g, "").slice(0, 6)); setError(null); }} placeholder={copy.twoFactorCodePlaceholder} /></label>}</>}
      </div>
      {error && <div className="banner banner-error" role="alert">{error}</div>}
      <div className="key-modal-actions"><button type="button" className="btn btn-ghost" disabled={busy} onClick={closeEdit}>{localCopy.cancel}</button><button className="btn btn-primary" disabled={busy || !editDirty}>{busy ? localCopy.savingPolicy : localCopy.savePolicy}</button></div>
    </form></div>}

    {revokeTarget && <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.currentTarget === event.target) closeRevoke(); }}><div ref={revokeModalRef} className="key-modal key-revoke-modal" role="alertdialog" aria-modal="true" aria-labelledby="revoke-key-title" aria-describedby="revoke-key-description" tabIndex={-1}><div className="key-modal-head"><div><span className="eyebrow danger-text">{localCopy.revokeKey}</span><h2 id="revoke-key-title">{localCopy.revokeTitle}</h2><p><strong>{revokeTarget.label || copy.unlabelledKey}</strong> · <code>{revokeTarget.keyMasked}</code></p></div></div><p id="revoke-key-description">{localCopy.revokeBody}</p>{error && <div className="banner banner-error" role="alert">{error}</div>}<div className="key-modal-actions"><button type="button" className="btn btn-ghost" disabled={busy} onClick={closeRevoke}>{localCopy.cancel}</button><button type="button" className="btn btn-danger" disabled={busy} autoFocus onClick={() => void revoke()}>{localCopy.confirmRevoke}</button></div></div></div>}
  </section>;
}

function nanoToUsdInput(value: string): string {
  const nano = BigInt(value);
  const whole = nano / NANO_PER_USD;
  const fraction = (nano % NANO_PER_USD).toString().padStart(9, "0").replace(/0+$/, "");
  return fraction ? `${whole}.${fraction}` : whole.toString();
}

function usdInputToNano(value: string): bigint {
  const [whole = "0", fraction = ""] = value.split(".");
  return BigInt(whole) * NANO_PER_USD + BigInt(fraction.padEnd(9, "0"));
}

function isoToLocalDateInput(value: string): string {
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return "";
  return new Date(date.getTime() - date.getTimezoneOffset() * 60_000).toISOString().slice(0, 10);
}

function keyPolicy(key: ApiKeyView, now: number): {
  health: "active" | "disabled" | "expired" | "expires-soon" | "limit" | "near-limit";
  expired: boolean; expiresSoon: boolean; limitReached: boolean; nearLimit: boolean;
} {
  const expired = Boolean(key.expiresAt && Date.parse(key.expiresAt) <= now);
  const expiresSoon = Boolean(key.expiresAt && !expired && Date.parse(key.expiresAt) - now <= 7 * 86_400_000);
  let limitReached = false, nearLimit = false;
  if (key.spendLimitNano) {
    const committed = BigInt(key.spentNano) + BigInt(key.reservedNano ?? "0");
    const limit = BigInt(key.spendLimitNano);
    limitReached = committed >= limit;
    nearLimit = !limitReached && committed * 10n >= limit * 9n;
  }
  const health = key.status === "disabled" ? "disabled" : expired ? "expired" : limitReached ? "limit" : nearLimit ? "near-limit" : expiresSoon ? "expires-soon" : "active";
  return { health, expired, expiresSoon, limitReached, nearLimit };
}

export function isApiKeyUsable(key: ApiKeyView, now: number): boolean {
  const policy = keyPolicy(key, now);
  return key.status === "active" && !policy.expired && !policy.limitReached;
}

function formatRelativeDate(value: string, language: "en" | "ru"): string {
  const elapsedDays = Math.floor((Date.now() - Date.parse(value)) / 86_400_000);
  if (elapsedDays <= 0) return language === "ru" ? "Сегодня" : "Today";
  if (elapsedDays === 1) return language === "ru" ? "Вчера" : "Yesterday";
  if (elapsedDays < 30) return language === "ru" ? `${elapsedDays} дн. назад` : `${elapsedDays}d ago`;
  return new Date(value).toLocaleDateString(language === "ru" ? "ru-RU" : "en-US");
}
