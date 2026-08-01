import type { DeployEvent } from "./events.js";
import { errorMessage, type Logger } from "./log.js";
import type { GithubSnapshot, PhaseState, StateStore } from "./state.js";

/** Фазовые контексты commit statuses → фазы чеклиста деплой-сообщения. */
const STATUS_PHASES: Record<string, string> = {
  "deploy/tests": "tests",
  "deploy/migration": "migration",
};

/** Компонентные окружения Deployments API → фазы чеклиста. */
const DEPLOY_PHASE_ENVS: Record<string, string> = {
  "production-engine": "engine",
  "production-backend": "backend",
  "production-sales": "sales",
  "production-openkeys": "openkeys",
  "production-admin": "admin",
};

const CI_ENVS = new Set(["candidate-validation"]);
const RELEVANT_ENVS = new Set([...Object.keys(DEPLOY_PHASE_ENVS), ...CI_ENVS]);

interface GhStatus {
  context?: string;
  state?: string;
  created_at?: string;
}

interface GhDeployment {
  id: number;
  environment?: string;
  sha?: string;
  created_at?: string;
}

interface GhDeploymentStatus {
  state?: string;
  created_at?: string;
}

function mapGhState(state: string | undefined): PhaseState {
  if (state === "success") return "success";
  if (state === "failure" || state === "error") return "failure";
  return "pending";
}

/**
 * Чистая diff-логика вех деплоя (DEVBOT.md §2.1): новый SHA, фазовые переходы,
 * карантин (tests/migration/watchdog failure), зелёный пайплайн (watchdog success),
 * candidate-validation → 🧪 CI. Первый снимок — база без событий, чтобы рестарт
 * бота не спамил историей.
 */
export function diffSnapshots(prev: GithubSnapshot | null, next: GithubSnapshot): DeployEvent[] {
  if (!prev) return [];
  const events: DeployEvent[] = [];
  const newSha = next.sha !== prev.sha;
  if (newSha) {
    events.push({ kind: "new-sha", sha: next.sha, title: next.title, ...(next.author ? { author: next.author } : {}) });
  }
  for (const [context, state] of Object.entries(next.statuses)) {
    if (!newSha && prev.statuses[context] === state) continue;
    const phase = STATUS_PHASES[context];
    if (phase) {
      events.push({ kind: "phase", sha: next.sha, phase, state });
      if (state === "failure") events.push({ kind: "quarantine", sha: next.sha, phase });
    } else if (context === "deploy/watchdog") {
      if (state === "success") events.push({ kind: "green", sha: next.sha });
      if (state === "failure") events.push({ kind: "quarantine", sha: next.sha });
    }
  }
  for (const [env, deployment] of Object.entries(next.deployments)) {
    const prevDeployment = prev.deployments[env];
    const changed = !prevDeployment || prevDeployment.id !== deployment.id || prevDeployment.state !== deployment.state;
    // Деплой относится к текущему SHA, только если катил именно его; чужой/старый
    // деплой не должен закрашивать фазы нового чеклиста.
    const belongsToSha = !deployment.sha || deployment.sha === next.sha;
    if (CI_ENVS.has(env)) {
      if (changed || newSha) events.push({ kind: "ci", environment: env, state: deployment.state });
      continue;
    }
    const phase = DEPLOY_PHASE_ENVS[env];
    if (phase && changed && belongsToSha) {
      events.push({ kind: "phase", sha: next.sha, phase, state: deployment.state });
      if (deployment.state === "failure") events.push({ kind: "quarantine", sha: next.sha, phase });
    }
  }
  return events;
}

export interface GithubPollerDeps {
  token: string;
  repo: string;
  state: StateStore;
  onEvents: (events: DeployEvent[]) => void;
  logger?: Logger;
  intervalMs?: number;
  fetchFn?: typeof fetch;
}

export class GithubPoller {
  private readonly fetchFn: typeof fetch;
  private timer: NodeJS.Timeout | undefined;
  private running = false;

  constructor(private readonly deps: GithubPollerDeps) {
    this.fetchFn = deps.fetchFn ?? fetch;
  }

  private async api<T>(apiPath: string): Promise<T> {
    const response = await this.fetchFn(`https://api.github.com/repos/${this.deps.repo}${apiPath}`, {
      headers: {
        accept: "application/vnd.github+json",
        authorization: `Bearer ${this.deps.token}`,
        "user-agent": "apitoken-devbot",
      },
    });
    if (!response.ok) {
      throw new Error(`github ${apiPath}: HTTP ${response.status}`);
    }
    return await response.json() as T;
  }

  /** deploy/* статусы одного коммита (новейшее состояние каждого контекста). */
  private async fetchStatuses(sha: string): Promise<Record<string, PhaseState>> {
    const statusesRaw = await this.api<GhStatus[]>(`/commits/${sha}/statuses?per_page=100`);
    const statuses: Record<string, PhaseState> = {};
    for (const item of statusesRaw) {
      const context = item.context ?? "";
      if (!context.startsWith("deploy/")) continue;
      // statuses отсортированы новейшими первыми — первое вхождение контекста актуально.
      if (!(context in statuses)) statuses[context] = mapGhState(item.state);
    }
    return statuses;
  }

  /** Один цикл опроса: statuses origin/master HEAD + deployments, diff против state. */
  async pollOnce(): Promise<void> {
    const { state } = this.deps;
    const branch = await this.api<{
      sha: string;
      commit: { message: string; author?: { name?: string } };
      author?: { login?: string } | null;
    }>("/commits/master");
    const sha = branch.sha;
    const title = branch.commit.message.split("\n")[0] ?? "";
    // Кто отправил коммит: git author name всегда есть в коммите; @login — только когда
    // email коммита привязан к GitHub-аккаунту (для агентских адресов author === null).
    const gitName = branch.commit.author?.name?.trim() ?? "";
    const login = branch.author?.login?.trim() ?? "";
    const author = gitName && login && gitName.toLowerCase() !== login.toLowerCase()
      ? `${gitName} @${login}`
      : gitName || (login ? `@${login}` : undefined);

    const statuses = await this.fetchStatuses(sha);

    const deploymentsRaw = await this.api<GhDeployment[]>("/deployments?per_page=30");
    const latestByEnv = new Map<string, GhDeployment>();
    for (const deployment of deploymentsRaw) {
      const env = deployment.environment ?? "";
      if (!RELEVANT_ENVS.has(env) || latestByEnv.has(env)) continue;
      latestByEnv.set(env, deployment);
    }
    const deployments: GithubSnapshot["deployments"] = {};
    for (const [env, deployment] of latestByEnv) {
      const statusesOfDeployment = await this.api<GhDeploymentStatus[]>(
        `/deployments/${deployment.id}/statuses?per_page=1`,
      );
      deployments[env] = {
        id: deployment.id,
        state: mapGhState(statusesOfDeployment[0]?.state),
        ...(deployment.sha ? { sha: deployment.sha } : {}),
      };
    }

    const prev = state.data.github;
    const snapshot: GithubSnapshot = { sha, title, ...(author ? { author } : {}), statuses, deployments };

    // Хвост предыдущего SHA: agent-merge пушит следующий master сразу после зелени
    // предыдущего, поэтому финал (deploy/watchdog=success) почти всегда ускользает от
    // diff'а по HEAD. Пока финал не наблюдался — держим хвост и доталкиваем его события.
    // Одного слота достаточно: следующий master не может быть запушен, пока предыдущий
    // не дойдёт до терминала (merge-lock + проверка зелени в agent-merge).
    if (prev && prev.sha !== snapshot.sha) {
      const prevWatchdog = prev.statuses["deploy/watchdog"];
      if (prevWatchdog !== "success" && prevWatchdog !== "failure") {
        snapshot.tail = { sha: prev.sha, statuses: prev.statuses };
      }
    } else if (prev?.tail) {
      snapshot.tail = prev.tail;
    }

    const events = diffSnapshots(prev, snapshot);

    if (snapshot.tail) {
      const tailSha = snapshot.tail.sha;
      const tailStatuses = await this.fetchStatuses(tailSha);
      const tailPrev: GithubSnapshot = { sha: tailSha, title: "", statuses: snapshot.tail.statuses, deployments: {} };
      const tailNext: GithubSnapshot = { sha: tailSha, title: "", statuses: tailStatuses, deployments: {} };
      events.push(...diffSnapshots(tailPrev, tailNext));
      const terminal = tailStatuses["deploy/watchdog"];
      if (terminal === "success" || terminal === "failure") {
        delete snapshot.tail;
      } else {
        snapshot.tail = { sha: tailSha, statuses: tailStatuses };
      }
    }

    state.data.github = snapshot;
    if (events.length > 0) {
      this.deps.onEvents(events);
    }
    await state.save();
  }

  start(): void {
    if (this.timer) return;
    const intervalMs = this.deps.intervalMs ?? 45_000;
    const tick = async () => {
      if (this.running) return;
      this.running = true;
      try {
        await this.pollOnce();
      } catch (error) {
        this.deps.logger?.warn(`github-poller: ${errorMessage(error)}`);
      } finally {
        this.running = false;
      }
    };
    void tick();
    this.timer = setInterval(() => void tick(), intervalMs);
    this.timer.unref();
  }

  stop(): void {
    if (this.timer) {
      clearInterval(this.timer);
      this.timer = undefined;
    }
  }
}
