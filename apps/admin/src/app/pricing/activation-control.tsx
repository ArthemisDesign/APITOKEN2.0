"use client";

import { useEffect, useState } from "react";
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

export type ActivationKind = "cutover" | "recovery";

export interface PricingReleaseHeadV2 {
  active_generation: number;
  active_digest: string;
  head_version: number;
  updated_ts: number;
}

export interface PricingReleaseActivationEvidenceViewV2 {
  evidence_digest: string;
  engine_evidence_digest: string | null;
  engine_captured_at: string | null;
  target_generation: string;
  target_digest: string;
  recovery_generation: string;
  recovery_digest: string;
  service_inventory_digest: string | null;
  legacy_inflight_count: string;
  blocker_count: string;
  passed: boolean;
  observed_at: string;
  valid_until: string;
  target_status: string;
  recovery_status: string;
  target_engine_digest: string | null;
  recovery_engine_digest: string | null;
  fresh: boolean;
  source_complete: boolean;
  local_blockers: string[];
}

export interface PricingReleaseActivationControlV2 {
  database_observed_at: string;
  unresolved_pricing_jobs: number;
  engine: {
    observed_at: string;
    available: boolean;
    head: PricingReleaseHeadV2 | null;
  };
  releases: Array<{
    generation: string;
    release_kind: "target" | "recovery";
    status: string;
    content_digest: string;
    engine_release_digest: string | null;
    commerce_inventory_digest: string;
    engine_inventory_digest: string;
    openkeys_inventory_digest: string;
    service_inventory_digest: string;
    created_at: string;
    updated_at: string;
  }>;
  evidence: PricingReleaseActivationEvidenceViewV2[];
  jobs: Array<{
    id: string;
    activation_kind: ActivationKind;
    release_generation: string;
    release_digest: string;
    evidence_digest: string;
    status: string;
    attempts: number;
    operator_id: string | null;
    reason: string | null;
    last_error: string | null;
    result_digest: string | null;
    confirmed_at: string | null;
    created_at: string;
    updated_at: string;
  }>;
  receipts: Array<{
    activation_id: string;
    activation_kind: ActivationKind;
    release_generation: string;
    release_digest: string;
    evidence_digest: string;
    head_version: string;
    receipt_digest: string;
    activated_at: string;
    created_at: string;
  }>;
}

type ActivationBlocker =
  | "activation_control_refresh_failed"
  | "engine_unavailable"
  | "evidence_not_passed"
  | "evidence_expired"
  | "source_incomplete"
  | "unresolved_pricing_jobs"
  | "cutover_head_present"
  | "recovery_head_absent"
  | "recovery_head_mismatch"
  | "cutover_receipt_missing"
  | `authority:${string}`;

const BLOCKER_LABELS: Record<Exclude<ActivationBlocker, `authority:${string}`>, string> = {
  activation_control_refresh_failed: "fresh control snapshot недоступен",
  engine_unavailable: "engine head недоступен",
  evidence_not_passed: "Stage 8 не passed/zero-blocker",
  evidence_expired: "Stage 8 evidence истёк",
  source_incomplete: "неполная source identity Stage 8",
  unresolved_pricing_jobs: "есть незавершённые pricing jobs",
  cutover_head_present: "global head уже существует",
  recovery_head_absent: "для recovery отсутствует active target head",
  recovery_head_mismatch: "active head не совпадает с exact target",
  cutover_receipt_missing: "нет exact durable cutover receipt",
};

const errorText = (error: unknown): string => error instanceof Error ? error.message : String(error);

function digestLabel(digest: string | null): string {
  if (!digest) return "—";
  return digest.length > 27 ? `${digest.slice(0, 16)}…${digest.slice(-8)}` : digest;
}

function statusTone(status: string): Tone {
  if (["prepared", "confirmed", "active", "passed"].includes(status)) return "ok";
  if (["dead", "failed", "rejected"].includes(status)) return "bad";
  return "warn";
}

function unique<T>(values: T[]): T[] {
  return [...new Set(values)];
}

function exactCutoverReceiptExists(
  control: PricingReleaseActivationControlV2,
  evidence: PricingReleaseActivationEvidenceViewV2,
): boolean {
  return control.receipts.some((receipt) => receipt.activation_kind === "cutover"
    && receipt.release_generation === evidence.target_generation
    && receipt.release_digest === evidence.target_digest);
}

export function activationBlockers(
  control: PricingReleaseActivationControlV2,
  evidence: PricingReleaseActivationEvidenceViewV2,
  kind: ActivationKind,
  nowMs: number,
  refreshFailed = false,
): ActivationBlocker[] {
  const blockers: ActivationBlocker[] = [];
  if (refreshFailed) blockers.push("activation_control_refresh_failed");
  if (!control.engine.available) blockers.push("engine_unavailable");
  if (!evidence.passed || evidence.blocker_count !== "0") blockers.push("evidence_not_passed");
  if (!evidence.fresh || Date.parse(evidence.valid_until) <= nowMs) blockers.push("evidence_expired");
  if (!evidence.source_complete) blockers.push("source_incomplete");
  if (control.unresolved_pricing_jobs !== 0) blockers.push("unresolved_pricing_jobs");
  blockers.push(...evidence.local_blockers.map((blocker) => `authority:${blocker}` as const));

  const head = control.engine.head;
  if (kind === "cutover") {
    if (head !== null) blockers.push("cutover_head_present");
  } else if (head === null) {
    blockers.push("recovery_head_absent");
  } else {
    if (String(head.active_generation) !== evidence.target_generation
      || head.active_digest !== evidence.target_engine_digest) {
      blockers.push("recovery_head_mismatch");
    }
    if (!exactCutoverReceiptExists(control, evidence)) blockers.push("cutover_receipt_missing");
  }
  return unique(blockers);
}

export function activationConfirmationPhrase(
  kind: ActivationKind,
  evidence: PricingReleaseActivationEvidenceViewV2,
): string {
  const generation = kind === "cutover" ? evidence.target_generation : evidence.recovery_generation;
  return `${kind.toUpperCase()} ${generation} ${evidence.evidence_digest.slice(-8)}`;
}

export function activationConfirmationError(
  values: Record<string, string>,
  expectedPhrase: string,
): string | null {
  const reason = values.reason?.trim() ?? "";
  if (reason.length < 10) return "Причина должна содержать минимум 10 символов.";
  if (reason.length > 2_000) return "Причина не может быть длиннее 2000 символов.";
  if (values.confirmation?.trim() !== expectedPhrase) {
    return `Подтверждение не совпало. Введите точно: ${expectedPhrase}`;
  }
  return null;
}

function blockerLabel(blocker: ActivationBlocker): string {
  if (blocker.startsWith("authority:")) return blocker.slice("authority:".length);
  return BLOCKER_LABELS[blocker as keyof typeof BLOCKER_LABELS];
}

function EvidenceActions(props: {
  control: PricingReleaseActivationControlV2;
  evidence: PricingReleaseActivationEvidenceViewV2;
  nowMs: number;
  refreshFailed: boolean;
  busy: string | null;
  onStage: (kind: ActivationKind, evidence: PricingReleaseActivationEvidenceViewV2) => void;
}) {
  const id = props.evidence.evidence_digest;
  const cutoverBlockers = activationBlockers(
    props.control,
    props.evidence,
    "cutover",
    props.nowMs,
    props.refreshFailed,
  );
  const recoveryBlockers = activationBlockers(
    props.control,
    props.evidence,
    "recovery",
    props.nowMs,
    props.refreshFailed,
  );
  return (
    <div className="activation-actions">
      <button
        className="btn bad"
        disabled={props.busy !== null || cutoverBlockers.length !== 0}
        title={cutoverBlockers.map(blockerLabel).join("; ")}
        onClick={() => props.onStage("cutover", props.evidence)}
      >
        {props.busy === `cutover:${id}` ? "stage…" : "stage cutover"}
      </button>
      <button
        className="btn warn"
        disabled={props.busy !== null || recoveryBlockers.length !== 0}
        title={recoveryBlockers.map(blockerLabel).join("; ")}
        onClick={() => props.onStage("recovery", props.evidence)}
      >
        {props.busy === `recovery:${id}` ? "stage…" : "stage recovery"}
      </button>
      <div className="sub activation-blockers">
        {props.control.engine.head === null
          ? cutoverBlockers.map(blockerLabel).join(" · ") || "cutover preflight green"
          : recoveryBlockers.map(blockerLabel).join(" · ") || "recovery preflight green"}
      </div>
    </div>
  );
}

async function loadActivationControl(): Promise<PricingReleaseActivationControlV2> {
  return api<PricingReleaseActivationControlV2>("/admin/pricing-release-activation-v2");
}

export function PricingReleaseActivationControl() {
  const { data, error, refresh } = usePoll(
    "pricing-release-activation-v2",
    loadActivationControl,
    { interval: 5_000 },
  );
  const [nowMs, setNowMs] = useState(() => Date.now());
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    const timer = window.setInterval(() => setNowMs(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, []);

  const stage = async (kind: ActivationKind, evidence: PricingReleaseActivationEvidenceViewV2) => {
    const phrase = activationConfirmationPhrase(kind, evidence);
    const releaseGeneration = kind === "cutover" ? evidence.target_generation : evidence.recovery_generation;
    const values = await dialog({
      title: kind === "cutover" ? "Stage global pricing cutover" : "Stage global pricing recovery",
      message: [
        `Release g${releaseGeneration}; evidence ${evidence.evidence_digest}.`,
        "После durable staging worker может немедленно выполнить один глобальный CAS для всех клиентов.",
        `Для подтверждения введите: ${phrase}`,
      ].join(" "),
      fields: [
        { name: "reason", label: "Содержательная причина" },
        { name: "confirmation", label: "Точная подтверждающая фраза" },
      ],
      confirmLabel: kind === "cutover" ? "Stage global cutover" : "Stage global recovery",
      danger: true,
    });
    if (!values) return;
    const confirmationError = activationConfirmationError(values, phrase);
    if (confirmationError) {
      toast(confirmationError, "bad");
      return;
    }

    const busyId = `${kind}:${evidence.evidence_digest}`;
    setBusy(busyId);
    try {
      const latest = await loadActivationControl();
      const latestEvidence = latest.evidence.find(
        (candidate) => candidate.evidence_digest === evidence.evidence_digest,
      );
      if (!latestEvidence) throw new Error("Exact Stage 8 evidence больше не входит в bounded control snapshot.");
      const blockers = activationBlockers(latest, latestEvidence, kind, Date.now());
      if (blockers.length !== 0) {
        throw new Error(`Fresh preflight заблокирован: ${blockers.map(blockerLabel).join("; ")}`);
      }
      const accepted = await send<{
        job_id: string;
        activation_kind: ActivationKind;
        evidence_digest: string;
        status: "accepted";
      }>("/admin/pricing-release-activation-v2/stage", "POST", {
        activation_kind: kind,
        evidence_digest: latestEvidence.evidence_digest,
        reason: values.reason.trim(),
      });
      toast(`Job ${accepted.job_id} durable staged; ожидаем worker receipt.`);
      refresh();
    } catch (cause) {
      toast(errorText(cause), "bad");
    } finally {
      setBusy(null);
    }
  };

  return (
    <section className="activation-control">
      <SectionHeader
        title="Global release activation"
        sub="read-only control snapshot + единственный explicit durable staging path"
      />
      {!data ? (
        error
          ? <Banner kind="bad" title="Activation control недоступен">Mutation fail-closed: {error.message}</Banner>
          : <LoadingGrid count={4} />
      ) : (
        <>
          {error ? (
            <Banner kind="bad" title="Fresh activation snapshot недоступен">
              Показаны stale данные; обе mutation-кнопки отключены. {error.message}
            </Banner>
          ) : null}
          <Banner
            kind={data.engine.available ? "warn" : "bad"}
            title="Staging — не dry-run"
          >
            POST только создаёт immutable job, но worker может выполнить глобальный CAS сразу после
            fresh authority revalidation. Traffic, money writers и reservations не останавливаются.
          </Banner>
          <CardGrid>
            <StatCard
              label="Engine head"
              value={data.engine.available
                ? data.engine.head ? `g${data.engine.head.active_generation}` : "absent"
                : "unavailable"}
              hint={data.engine.head
                ? `head v${data.engine.head.head_version} · ${digestLabel(data.engine.head.active_digest)}`
                : `observed ${ago(data.engine.observed_at)}`}
            />
            <StatCard
              label="Stage 8"
              value={data.evidence.length}
              hint={data.evidence[0]
                ? `${data.evidence[0].fresh && data.evidence[0].source_complete ? "fresh + complete" : "blocked"} · ${ago(data.evidence[0].observed_at)}`
                : "evidence отсутствует"}
            />
            <StatCard
              label="Pricing blockers"
              value={data.unresolved_pricing_jobs}
              hint={data.unresolved_pricing_jobs === 0 ? "local backlog clear" : "staging запрещён"}
            />
            <StatCard
              label="Receipts"
              value={data.receipts.length}
              hint={`DB snapshot ${ago(data.database_observed_at)}`}
            />
          </CardGrid>

          <SectionHeader title="Prepared releases" sub="target/recovery pair и full-inventory identities" />
          <TableCard>
            <table className="activation-table">
              <thead><tr><th className="left">release</th><th>status</th><th>engine digest</th><th>commerce</th><th>engine inventory</th><th>OpenKeys</th><th>service</th><th>updated</th></tr></thead>
              <tbody>
                {data.releases.map((release) => (
                  <tr key={`${release.release_kind}:${release.generation}`}>
                    <td className="left"><b>{release.release_kind} g{release.generation}</b><div className="sub mono" title={release.content_digest}>{digestLabel(release.content_digest)}</div></td>
                    <td><Pill kind={statusTone(release.status)}>{release.status}</Pill></td>
                    <td className="mono" title={release.engine_release_digest ?? undefined}>{digestLabel(release.engine_release_digest)}</td>
                    <td className="mono" title={release.commerce_inventory_digest}>{digestLabel(release.commerce_inventory_digest)}</td>
                    <td className="mono" title={release.engine_inventory_digest}>{digestLabel(release.engine_inventory_digest)}</td>
                    <td className="mono" title={release.openkeys_inventory_digest}>{digestLabel(release.openkeys_inventory_digest)}</td>
                    <td className="mono" title={release.service_inventory_digest}>{digestLabel(release.service_inventory_digest)}</td>
                    <td>{formatDate(release.updated_at, true)}</td>
                  </tr>
                ))}
                {data.releases.length === 0 ? <EmptyRow columns={8} text="prepared releases отсутствуют" /> : null}
              </tbody>
            </table>
          </TableCard>

          <SectionHeader title="Stage 8 evidence" sub="freshness, source completeness, blockers и explicit activation" />
          <TableCard>
            <table className="activation-table">
              <thead><tr><th className="left">evidence</th><th>pair</th><th>verdict</th><th>sources</th><th>legacy inflight</th><th>valid until</th><th className="left">действие</th></tr></thead>
              <tbody>
                {data.evidence.map((evidence) => {
                  const locallyFresh = evidence.fresh && Date.parse(evidence.valid_until) > nowMs;
                  const ready = evidence.passed && evidence.blocker_count === "0"
                    && locallyFresh && evidence.source_complete && evidence.local_blockers.length === 0;
                  return (
                    <tr key={evidence.evidence_digest}>
                      <td className="left"><b className="mono" title={evidence.evidence_digest}>{digestLabel(evidence.evidence_digest)}</b><div className="sub">observed {formatDate(evidence.observed_at, true)}</div></td>
                      <td>g{evidence.target_generation} → g{evidence.recovery_generation}<div className="sub">{evidence.target_status} / {evidence.recovery_status}</div></td>
                      <td><Pill kind={ready ? "ok" : "bad"}>{ready ? "passed" : "blocked"}</Pill><div className="sub">blockers {evidence.blocker_count}</div></td>
                      <td><Pill kind={evidence.source_complete ? "ok" : "bad"}>{evidence.source_complete ? "complete" : "missing"}</Pill><div className="sub">{evidence.local_blockers.join(", ") || "local clear"}</div></td>
                      <td>{evidence.legacy_inflight_count}<div className="sub">audit-only; drain не нужен</div></td>
                      <td><Pill kind={locallyFresh ? "ok" : "bad"}>{locallyFresh ? "fresh" : "expired"}</Pill><div className="sub">{formatDate(evidence.valid_until, true)}</div></td>
                      <td className="left">
                        <EvidenceActions
                          control={data}
                          evidence={evidence}
                          nowMs={nowMs}
                          refreshFailed={Boolean(error)}
                          busy={busy}
                          onStage={(kind, selected) => void stage(kind, selected)}
                        />
                      </td>
                    </tr>
                  );
                })}
                {data.evidence.length === 0 ? <EmptyRow columns={7} text="Stage 8 evidence отсутствует" /> : null}
              </tbody>
            </table>
          </TableCard>

          <SectionHeader title="Activation jobs" sub="durable worker lifecycle; dead/retry остаются явными blockers" />
          <TableCard>
            <table className="activation-table">
              <thead><tr><th className="left">job</th><th>kind</th><th>release</th><th>status</th><th>attempts</th><th className="left">operator / reason</th><th className="left">last error</th><th>updated</th></tr></thead>
              <tbody>
                {data.jobs.map((job) => (
                  <tr key={job.id}>
                    <td className="left"><b className="mono">{job.id}</b><div className="sub mono" title={job.evidence_digest}>{digestLabel(job.evidence_digest)}</div></td>
                    <td>{job.activation_kind}</td>
                    <td>g{job.release_generation}<div className="sub mono" title={job.release_digest}>{digestLabel(job.release_digest)}</div></td>
                    <td><Pill kind={statusTone(job.status)}>{job.status}</Pill></td>
                    <td>{job.attempts}</td>
                    <td className="left activation-copy"><b>{job.operator_id ?? "—"}</b><div className="sub">{job.reason ?? "—"}</div></td>
                    <td className="left activation-copy">{job.last_error ?? "—"}</td>
                    <td>{formatDate(job.updated_at, true)}</td>
                  </tr>
                ))}
                {data.jobs.length === 0 ? <EmptyRow columns={8} text="activation jobs отсутствуют" /> : null}
              </tbody>
            </table>
          </TableCard>

          <SectionHeader title="Activation receipts" sub="validated engine ACK и exact global head history" />
          <TableCard>
            <table className="activation-table">
              <thead><tr><th className="left">activation</th><th>kind</th><th>release</th><th>head version</th><th>evidence</th><th>receipt</th><th>activated</th></tr></thead>
              <tbody>
                {data.receipts.map((receipt) => (
                  <tr key={receipt.activation_id}>
                    <td className="left"><b className="mono">#{receipt.activation_id}</b></td>
                    <td><Pill kind="ok">{receipt.activation_kind}</Pill></td>
                    <td>g{receipt.release_generation}<div className="sub mono" title={receipt.release_digest}>{digestLabel(receipt.release_digest)}</div></td>
                    <td>{receipt.head_version}</td>
                    <td className="mono" title={receipt.evidence_digest}>{digestLabel(receipt.evidence_digest)}</td>
                    <td className="mono" title={receipt.receipt_digest}>{digestLabel(receipt.receipt_digest)}</td>
                    <td>{formatDate(receipt.activated_at, true)}</td>
                  </tr>
                ))}
                {data.receipts.length === 0 ? <EmptyRow columns={7} text="activation receipts отсутствуют" /> : null}
              </tbody>
            </table>
          </TableCard>
        </>
      )}
    </section>
  );
}
