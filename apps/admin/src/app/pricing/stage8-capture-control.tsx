"use client";

import { useEffect, useMemo, useState, type FormEvent } from "react";
import {
  Banner,
  CardGrid,
  EmptyRow,
  LoadingGrid,
  Pill,
  SectionHeader,
  StatCard,
  TableCard,
  type Tone,
} from "@/components/ui";
import { api, send } from "@/lib/api";
import { dialog } from "@/lib/dialog";
import { ago, formatDate } from "@/lib/format";
import { toast } from "@/lib/toast";
import { usePoll } from "@/lib/usePoll";

export type PricingStage8CaptureJobStatusV2 =
  | "pending"
  | "processing"
  | "retry"
  | "passed"
  | "blocked"
  | "dead";

export interface PricingStage8CaptureControlV2 {
  database_observed_at: string;
  counts_by_status: Record<PricingStage8CaptureJobStatusV2, number>;
  jobs: Array<{
    id: string;
    idempotency_key: string;
    request_digest: string;
    target_generation: string;
    recovery_generation: string;
    window_start_at: string;
    window_end_at: string;
    min_samples_per_provider: string;
    financial_sample_size: number;
    gemini_client_admissions: string;
    operator_id: string;
    reason: string;
    status: PricingStage8CaptureJobStatusV2;
    attempts: number;
    next_attempt_at: string;
    locked_at: string | null;
    locked_by: string | null;
    last_error: string | null;
    result_engine_evidence_digest: string | null;
    result_combined_evidence_digest: string | null;
    result_passed: boolean | null;
    completed_at: string | null;
    created_at: string;
    updated_at: string;
  }>;
  artifacts: Array<{
    id: string;
    job_id: string;
    attempt: number;
    engine_evidence_digest: string;
    engine_captured_at: string;
    combined_evidence_digest: string | null;
    combined_passed: boolean | null;
    combined_write_result: "stored" | "unchanged" | "not_persisted" | null;
    combined_observed_at: string | null;
    combined_valid_until: string | null;
    combined_blocker_count: string | null;
    combined_blockers: Array<{
      source: "commerce" | "engine";
      code: string;
      count: string;
      subject_digests: string[];
    }> | null;
    combined_blockers_truncated: boolean | null;
    completed_at: string | null;
    created_at: string;
  }>;
}

export interface PricingStage8CaptureDraftV2 {
  idempotencyKey: string;
  targetGeneration: string;
  recoveryGeneration: string;
  windowStartTs: string;
  windowEndTs: string;
  minSamplesPerProvider: string;
  financialSampleSize: string;
  geminiClientAdmissions: string;
  reason: string;
}

export interface PricingStage8CaptureStagePayloadV2 {
  idempotency_key: string;
  target_generation: number;
  recovery_generation: number;
  window_start_ts: number;
  window_end_ts: number;
  min_samples_per_provider: number;
  financial_sample_size: number;
  gemini_client_admissions: number;
  reason: string;
}

const ACTIVE_CAPTURE_STATUSES = new Set<PricingStage8CaptureJobStatusV2>([
  "pending",
  "processing",
  "retry",
]);
const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const CANONICAL_INTEGER_PATTERN = /^(0|[1-9][0-9]*)$/;
const MAX_SAFE_INTEGER_BIGINT = BigInt(Number.MAX_SAFE_INTEGER);

const emptyDraft = (): PricingStage8CaptureDraftV2 => ({
  idempotencyKey: "",
  targetGeneration: "",
  recoveryGeneration: "",
  windowStartTs: "",
  windowEndTs: "",
  minSamplesPerProvider: "100",
  financialSampleSize: "100",
  geminiClientAdmissions: "",
  reason: "",
});

function safeInteger(
  raw: string,
  label: string,
  minimum: bigint,
  maximum = MAX_SAFE_INTEGER_BIGINT,
): { value?: number; error?: string } {
  if (!CANONICAL_INTEGER_PATTERN.test(raw)) {
    return { error: `${label}: требуется целое число без знака и ведущих нулей.` };
  }
  const value = BigInt(raw);
  if (value < minimum || value > maximum) {
    return { error: `${label}: допустимый диапазон ${minimum}…${maximum}.` };
  }
  return { value: Number(value) };
}

export function buildPricingStage8CapturePayloadV2(
  draft: PricingStage8CaptureDraftV2,
  databaseObservedAt: string,
): { payload?: PricingStage8CaptureStagePayloadV2; error?: string } {
  if (!UUID_PATTERN.test(draft.idempotencyKey)) {
    return { error: "Idempotency key должен быть UUID." };
  }
  const observedMs = Date.parse(databaseObservedAt);
  if (!Number.isFinite(observedMs)) {
    return { error: "Commerce database time недоступно; staging запрещён." };
  }
  const fields = [
    safeInteger(draft.targetGeneration, "Target generation", 1n),
    safeInteger(draft.recoveryGeneration, "Recovery generation", 1n),
    safeInteger(draft.windowStartTs, "Window start", 1n),
    safeInteger(draft.windowEndTs, "Window end", 1n),
    safeInteger(draft.minSamplesPerProvider, "Minimum/provider", 1n, 1_000_000n),
    safeInteger(draft.financialSampleSize, "Financial sample", 1n, 1_000n),
    safeInteger(draft.geminiClientAdmissions, "Gemini admissions", 0n),
  ] as const;
  const fieldError = fields.find((field) => field.error)?.error;
  if (fieldError) return { error: fieldError };
  const [target, recovery, windowStart, windowEnd, providerMinimum, financialSample, geminiAdmissions] =
    fields.map((field) => field.value!);
  if (recovery <= target) return { error: "Recovery generation должна быть новее target generation." };
  if (windowEnd <= windowStart) return { error: "Capture window должен быть непустым: end > start." };
  if (windowEnd > Math.floor(observedMs / 1_000)) {
    return { error: "Capture window ещё не закрыт по времени commerce database." };
  }
  const reason = draft.reason.trim();
  if (reason.length < 10) return { error: "Содержательная причина должна быть не короче 10 символов." };
  if (reason.length > 2_000) return { error: "Причина не может быть длиннее 2000 символов." };
  if (/[\u0000-\u001f\u007f-\u009f]/u.test(reason)) {
    return { error: "Причина не должна содержать управляющие символы." };
  }
  return {
    payload: {
      idempotency_key: draft.idempotencyKey,
      target_generation: target,
      recovery_generation: recovery,
      window_start_ts: windowStart,
      window_end_ts: windowEnd,
      min_samples_per_provider: providerMinimum,
      financial_sample_size: financialSample,
      gemini_client_admissions: geminiAdmissions,
      reason,
    },
  };
}

export function pricingStage8CaptureActiveCount(control: PricingStage8CaptureControlV2): number {
  return [...ACTIVE_CAPTURE_STATUSES]
    .reduce((total, status) => total + (control.counts_by_status[status] ?? 0), 0);
}

export function pricingStage8CaptureConfirmationPhrase(
  payload: PricingStage8CaptureStagePayloadV2,
): string {
  return `CAPTURE ${payload.target_generation}->${payload.recovery_generation} ${payload.window_end_ts}`;
}

function digestLabel(digest: string | null): string {
  if (!digest) return "—";
  return digest.length > 27 ? `${digest.slice(0, 16)}…${digest.slice(-8)}` : digest;
}

function statusTone(status: PricingStage8CaptureJobStatusV2): Tone {
  if (status === "passed") return "ok";
  if (status === "blocked" || status === "dead") return "bad";
  return "warn";
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

async function loadPricingStage8CaptureControl(): Promise<PricingStage8CaptureControlV2> {
  return api<PricingStage8CaptureControlV2>("/admin/pricing-stage8-capture-v2");
}

export function PricingStage8CaptureControl() {
  const { data, error, refresh } = usePoll(
    "pricing-stage8-capture-v2",
    loadPricingStage8CaptureControl,
    { interval: 5_000 },
  );
  const [draft, setDraft] = useState<PricingStage8CaptureDraftV2>(emptyDraft);
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [accepted, setAccepted] = useState<{ job_id: string; request_digest: string } | null>(null);
  const [nowMs, setNowMs] = useState(() => Date.now());

  useEffect(() => {
    const timer = window.setInterval(() => setNowMs(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, []);

  const activeCount = data ? pricingStage8CaptureActiveCount(data) : 0;
  const parsedDraft = useMemo(
    () => data ? buildPricingStage8CapturePayloadV2(draft, data.database_observed_at) : {},
    [data, draft],
  );

  const update = (field: keyof PricingStage8CaptureDraftV2, value: string) => {
    setDraft((current) => ({ ...current, [field]: value }));
    setFormError(null);
    setAccepted(null);
  };

  const newIdempotencyKey = () => {
    update("idempotencyKey", window.crypto.randomUUID());
  };

  const stage = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!data || error) {
      setFormError("Fresh capture-control snapshot недоступен; staging запрещён.");
      return;
    }
    if (activeCount !== 0) {
      setFormError("Уже есть pending/processing/retry capture job; дождитесь terminal status.");
      return;
    }
    if (!parsedDraft.payload) {
      setFormError(parsedDraft.error ?? "Capture request некорректен.");
      return;
    }
    const phrase = pricingStage8CaptureConfirmationPhrase(parsedDraft.payload);
    const values = await dialog({
      title: "Stage managed Stage 8 capture",
      message: [
        `Будет создан один immutable job ${parsedDraft.payload.idempotency_key}.`,
        "Он не останавливает traffic/money writers и не создаёт activation job.",
        `Для подтверждения введите: ${phrase}`,
      ].join(" "),
      fields: [{ name: "confirmation", label: "Точная подтверждающая фраза" }],
      confirmLabel: "Stage read-only capture",
    });
    if (!values) return;
    if (values.confirmation?.trim() !== phrase) {
      setFormError(`Подтверждение не совпало. Введите точно: ${phrase}`);
      return;
    }

    setBusy(true);
    setFormError(null);
    try {
      const latest = await loadPricingStage8CaptureControl();
      if (pricingStage8CaptureActiveCount(latest) !== 0) {
        throw new Error("Fresh preflight: другой capture job уже active.");
      }
      const latestPayload = buildPricingStage8CapturePayloadV2(draft, latest.database_observed_at);
      if (!latestPayload.payload) throw new Error(`Fresh preflight: ${latestPayload.error}`);
      const staged = await send<{
        job_id: string;
        request_digest: string;
        status: "accepted";
      }>("/admin/pricing-stage8-capture-v2/stage", "POST", latestPayload.payload);
      setAccepted({ job_id: staged.job_id, request_digest: staged.request_digest });
      toast(`Stage 8 capture job ${staged.job_id} durable staged; ожидаем terminal evidence.`);
      refresh();
    } catch (cause) {
      setFormError(errorText(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="capture-control">
      <SectionHeader
        title="Managed Stage 8 capture"
        sub="explicit immutable request → exact engine bytes → combined full-inventory evidence"
      />
      {!data ? (
        error
          ? <Banner kind="bad" title="Capture control недоступен">Staging fail-closed: {error.message}</Banner>
          : <LoadingGrid count={4} />
      ) : (
        <>
          {error ? (
            <Banner kind="bad" title="Fresh capture snapshot недоступен">
              Показаны stale данные; staging отключён. {error.message}
            </Banner>
          ) : null}
          <Banner kind="ok" title="Capture не изменяет production traffic или pricing head">
            Job только собирает read-only evidence. Blocked — terminal доказательство, а не transport failure;
            activation остаётся отдельным explicit действием ниже.
          </Banner>
          <CardGrid>
            <StatCard label="Active queue" value={activeCount} hint="pending + processing + retry" />
            <StatCard label="Passed" value={data.counts_by_status.passed} hint="freshness проверяется по artifact" />
            <StatCard label="Blocked" value={data.counts_by_status.blocked} hint="terminal reviewed evidence" />
            <StatCard label="Dead" value={data.counts_by_status.dead} hint={`DB snapshot ${ago(data.database_observed_at)}`} />
          </CardGrid>

          <SectionHeader title="Stage one capture" sub="exact reviewed bounds; UUID и reason сохраняются в audit" />
          <form className="form-card capture-form" onSubmit={(event) => void stage(event)}>
            <div className="field capture-idempotency">
              <label htmlFor="capture-idempotency">Idempotency UUID</label>
              <div className="capture-inline-input">
                <input
                  id="capture-idempotency"
                  value={draft.idempotencyKey}
                  onChange={(event) => update("idempotencyKey", event.target.value)}
                  autoComplete="off"
                  spellCheck={false}
                  placeholder="Нажмите «новый UUID»"
                />
                <button className="btn ghost" type="button" disabled={busy} onClick={newIdempotencyKey}>новый UUID</button>
              </div>
            </div>
            <CaptureIntegerField label="Target generation" value={draft.targetGeneration} disabled={busy} onChange={(value) => update("targetGeneration", value)} />
            <CaptureIntegerField label="Recovery generation" value={draft.recoveryGeneration} disabled={busy} onChange={(value) => update("recoveryGeneration", value)} />
            <CaptureIntegerField label="Window start · epoch sec" value={draft.windowStartTs} disabled={busy} onChange={(value) => update("windowStartTs", value)} />
            <CaptureIntegerField label="Window end · epoch sec" value={draft.windowEndTs} disabled={busy} onChange={(value) => update("windowEndTs", value)} />
            <CaptureIntegerField label="Minimum / provider" value={draft.minSamplesPerProvider} disabled={busy} onChange={(value) => update("minSamplesPerProvider", value)} />
            <CaptureIntegerField label="Financial sample" value={draft.financialSampleSize} disabled={busy} onChange={(value) => update("financialSampleSize", value)} />
            <CaptureIntegerField label="Gemini client admissions" value={draft.geminiClientAdmissions} disabled={busy} onChange={(value) => update("geminiClientAdmissions", value)} />
            <div className="field capture-reason">
              <label htmlFor="capture-reason">Содержательная причина</label>
              <input
                id="capture-reason"
                value={draft.reason}
                maxLength={2_000}
                onChange={(event) => update("reason", event.target.value)}
                placeholder="reviewed full-inventory peak window…"
                disabled={busy}
              />
            </div>
            <div className="capture-submit">
              <div className="sub">
                Database time: {formatDate(data.database_observed_at, true)}. Window end должен быть закрыт;
                Gemini admissions вводится из независимого client-edge aggregate без identities.
              </div>
              <button
                className="btn"
                type="submit"
                disabled={busy || Boolean(error) || activeCount !== 0 || !parsedDraft.payload}
                title={activeCount !== 0
                  ? "Дождитесь terminal status текущего job"
                  : parsedDraft.error}
              >
                {busy ? "fresh preflight + stage…" : "stage immutable capture"}
              </button>
            </div>
            {formError ? <div className="policy-rule-count bad capture-form-message">{formError}</div> : null}
            {accepted ? (
              <div className="policy-rule-count capture-form-message">
                Accepted job <code>{accepted.job_id}</code> · <code>{digestLabel(accepted.request_digest)}</code>.
                Exact replay с неизменным UUID/body безопасен; для другой попытки создайте новый UUID.
              </div>
            ) : null}
          </form>

          <SectionHeader title="Capture jobs" sub="bounded durable queue; terminal passed/blocked/dead не ретраятся" />
          <TableCard>
            <table className="capture-table">
              <thead><tr><th className="left">job / request</th><th>pair / window</th><th>samples</th><th>status</th><th>attempt</th><th>result</th><th className="left">operator / reason</th><th className="left">last error</th><th>updated</th></tr></thead>
              <tbody>
                {data.jobs.map((job) => (
                  <tr key={job.id}>
                    <td className="left"><b className="mono">{job.id}</b><div className="sub mono" title={job.request_digest}>{digestLabel(job.request_digest)}</div><div className="sub mono">idem {job.idempotency_key}</div></td>
                    <td>g{job.target_generation} → g{job.recovery_generation}<div className="sub">{formatDate(job.window_start_at, true)} — {formatDate(job.window_end_at, true)}</div></td>
                    <td>{job.min_samples_per_provider} / {job.financial_sample_size}<div className="sub">Gemini {job.gemini_client_admissions}</div></td>
                    <td><Pill kind={statusTone(job.status)}>{job.status}</Pill></td>
                    <td>{job.attempts}<div className="sub">next {formatDate(job.next_attempt_at, true)}</div></td>
                    <td><Pill kind={job.result_passed === true ? "ok" : job.result_passed === false ? "bad" : "warn"}>{job.result_passed === true ? "passed" : job.result_passed === false ? "blocked" : "pending"}</Pill><div className="sub mono" title={job.result_combined_evidence_digest ?? undefined}>{digestLabel(job.result_combined_evidence_digest)}</div></td>
                    <td className="left capture-copy"><b>{job.operator_id}</b><div className="sub">{job.reason}</div></td>
                    <td className="left capture-copy">{job.last_error ?? "—"}</td>
                    <td>{formatDate(job.updated_at, true)}</td>
                  </tr>
                ))}
                {data.jobs.length === 0 ? <EmptyRow columns={9} text="capture jobs отсутствуют" /> : null}
              </tbody>
            </table>
          </TableCard>

          <SectionHeader title="Capture artifacts" sub="source digest, combined freshness и sanitized blocker summary" />
          <TableCard>
            <table className="capture-table">
              <thead><tr><th className="left">artifact</th><th>attempt</th><th>engine source</th><th>combined</th><th>freshness</th><th className="left">blockers</th><th>completed</th></tr></thead>
              <tbody>
                {data.artifacts.map((artifact) => {
                  const fresh = artifact.combined_valid_until !== null
                    && Date.parse(artifact.combined_valid_until) > nowMs;
                  return (
                    <tr key={artifact.id}>
                      <td className="left"><b className="mono">{artifact.id}</b><div className="sub mono">job {artifact.job_id}</div></td>
                      <td>{artifact.attempt}</td>
                      <td className="mono" title={artifact.engine_evidence_digest}>{digestLabel(artifact.engine_evidence_digest)}<div className="sub">{formatDate(artifact.engine_captured_at, true)}</div></td>
                      <td><Pill kind={artifact.combined_passed === true ? "ok" : artifact.combined_passed === false ? "bad" : "warn"}>{artifact.combined_passed === true ? "passed" : artifact.combined_passed === false ? "blocked" : "source only"}</Pill><div className="sub mono" title={artifact.combined_evidence_digest ?? undefined}>{digestLabel(artifact.combined_evidence_digest)}</div><div className="sub">{artifact.combined_write_result ?? "—"}</div></td>
                      <td><Pill kind={fresh ? "ok" : "bad"}>{fresh ? "fresh" : artifact.combined_valid_until ? "expired" : "pending"}</Pill><div className="sub">{artifact.combined_valid_until ? formatDate(artifact.combined_valid_until, true) : "—"}</div></td>
                      <td className="left capture-blockers">
                        <b>{artifact.combined_blocker_count ?? "—"} total{artifact.combined_blockers_truncated ? " · truncated" : ""}</b>
                        {(artifact.combined_blockers ?? []).map((blocker, index) => (
                          <div className="sub" key={`${blocker.source}:${blocker.code}:${index}`}>
                            {blocker.source}:{blocker.code} × {blocker.count}
                            {blocker.subject_digests.slice(0, 2).map((digest) => (
                              <span className="capture-subject" title={digest} key={digest}> · {digestLabel(digest)}</span>
                            ))}
                            {blocker.subject_digests.length > 2
                              ? ` · +${blocker.subject_digests.length - 2} hashed subjects`
                              : ""}
                          </div>
                        ))}
                      </td>
                      <td>{artifact.completed_at ? formatDate(artifact.completed_at, true) : "—"}</td>
                    </tr>
                  );
                })}
                {data.artifacts.length === 0 ? <EmptyRow columns={7} text="capture artifacts отсутствуют" /> : null}
              </tbody>
            </table>
          </TableCard>
        </>
      )}
    </section>
  );
}

function CaptureIntegerField(props: {
  label: string;
  value: string;
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <div className="field">
      <label>{props.label}</label>
      <input
        value={props.value}
        inputMode="numeric"
        autoComplete="off"
        spellCheck={false}
        disabled={props.disabled}
        onChange={(event) => props.onChange(event.target.value)}
      />
    </div>
  );
}
