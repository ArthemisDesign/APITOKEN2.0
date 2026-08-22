import type { PhaseState } from "./state.js";

/** Один инстанс алерта из webhook'а Alertmanager v4 или AM API. */
export interface AlertInstance {
  fingerprint: string;
  status: "firing" | "resolved";
  alertname: string;
  severity: string;
  component?: string;
  summary?: string;
  description?: string;
  startsAt: string;
  endsAt?: string;
}

export type DeployEvent =
  | { kind: "new-sha"; sha: string; title: string; author?: string }
  | { kind: "phase"; sha: string; phase: string; state: PhaseState }
  | { kind: "green"; sha: string }
  | { kind: "quarantine"; sha: string; phase?: string };

export interface JournalEvent {
  source: string;
  severity: "info" | "warning" | "critical";
  text: string;
}

/** Incoming Chatwoot client message after webhook filtering (outgoing/private/activity dropped). */
export interface ChatwootIncomingMessage {
  id: string;
  content: string;
  createdAt: string;
  conversationId: string;
  accountId: string;
  inboxName?: string;
  channel?: string;
  name?: string;
  email?: string;
  attachments: { fileName?: string; fileType?: string }[];
}
