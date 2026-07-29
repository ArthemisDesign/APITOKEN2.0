import { API_BASE_URL } from "./api";

export type DependencyState = "up" | "down";
export type ServiceLevel = "operational" | "degraded" | "unavailable" | "unknown";

export type CoreHealth = {
  ok: boolean;
  service: "commercial-api";
  database: DependencyState;
  engine: DependencyState;
};

export type ServiceComponent = {
  name: string;
  note: string;
  level: ServiceLevel;
  label: string;
};

export type ServiceStatusSnapshot = {
  checkedAt: string;
  health: CoreHealth | null;
  components: ServiceComponent[];
  overall: Exclude<ServiceLevel, "unavailable">;
};

const labels: Record<ServiceLevel, string> = {
  operational: "Operational",
  degraded: "Degraded",
  unavailable: "Unavailable",
  unknown: "Not independently monitored",
};

function component(name: string, note: string, level: ServiceLevel): ServiceComponent {
  return { name, note, level, label: labels[level] };
}

export function isCoreHealth(value: unknown): value is CoreHealth {
  if (!value || typeof value !== "object") return false;
  const health = value as Partial<CoreHealth>;
  return health.service === "commercial-api"
    && typeof health.ok === "boolean"
    && (health.database === "up" || health.database === "down")
    && (health.engine === "up" || health.engine === "down");
}

export function deriveServiceStatus(health: CoreHealth | null, checkedAt = new Date().toISOString()): ServiceStatusSnapshot {
  if (!health) {
    return {
      checkedAt,
      health: null,
      overall: "unknown",
      components: [
        component("API gateways (Claude & GPT)", "Live core health could not be verified", "unknown"),
        component("Dashboard & key management", "Live core health could not be verified", "unknown"),
        component("Payments (card & crypto)", "Checkout providers and the payment worker are not covered by the public core check", "unknown"),
        component("Guides & documentation", "This status page is responding", "operational"),
      ],
    };
  }

  const coreOperational = health.database === "up" && health.engine === "up" && health.ok;
  return {
    checkedAt,
    health,
    overall: coreOperational ? "operational" : "degraded",
    components: [
      component(
        "API gateways (Claude & GPT)",
        "Anthropic-compatible /v1/messages and OpenAI-compatible /v1 endpoints",
        health.engine === "up" ? "operational" : "unavailable",
      ),
      component(
        "Dashboard & key management",
        "Account, keys, usage and top-ups",
        coreOperational ? "operational" : health.database === "up" ? "degraded" : "unavailable",
      ),
      component(
        "Payments (card & crypto)",
        coreOperational
          ? "Core balance dependencies are healthy; checkout providers and the payment worker are not independently monitored here"
          : "A core balance dependency is degraded; checkout providers and the payment worker are not independently monitored here",
        coreOperational ? "unknown" : "degraded",
      ),
      component("Guides & documentation", "This status page is responding", "operational"),
    ],
  };
}

export async function loadServiceStatus(): Promise<ServiceStatusSnapshot> {
  const checkedAt = new Date().toISOString();
  try {
    const response = await fetch(`${API_BASE_URL}/health`, {
      next: { revalidate: 30 },
      signal: AbortSignal.timeout(3_000),
    });
    if (!response.ok) return deriveServiceStatus(null, checkedAt);
    const payload: unknown = await response.json();
    return deriveServiceStatus(isCoreHealth(payload) ? payload : null, checkedAt);
  } catch {
    return deriveServiceStatus(null, checkedAt);
  }
}
