import { ServiceUnavailableException } from "@nestjs/common";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Database } from "@claude-api/db";
import type { EngineClient } from "@claude-api/engine-client";
import { HealthController } from "./health.controller.js";
import type { ReadinessService } from "./readiness.service.js";

function controller(opts?: {
  databaseQuery?: () => Promise<unknown>;
  engineReadiness?: () => Promise<boolean>;
  accepting?: boolean;
}) {
  const database = {
    pool: {
      query: vi.fn(opts?.databaseQuery ?? (async () => ({ rowCount: 1, rows: [{}] }))),
    },
  } as unknown as Database;
  const engine = {
    readiness: vi.fn(opts?.engineReadiness ?? (async () => true)),
    health: vi.fn(async () => true),
  } as unknown as EngineClient;
  const readiness = {
    isAccepting: vi.fn(() => opts?.accepting !== false),
  } as unknown as ReadinessService;
  return {
    subject: new HealthController(database, engine, readiness),
    engine,
  };
}

describe("GET /v1/ready", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("returns HTTP 200 when the engine client rejects, with engine telemetry down", async () => {
    const { subject, engine } = controller({
      engineReadiness: async () => {
        throw new Error("engine Control API refused");
      },
    });

    await expect(subject.ready()).resolves.toEqual({
      status: "ok",
      database: "up",
      engine: "down",
    });
    expect(engine.readiness).toHaveBeenCalledOnce();
  });

  it("returns HTTP 200 when the engine readiness check times out", async () => {
    vi.useFakeTimers();
    const { subject } = controller({
      engineReadiness: () => new Promise(() => undefined),
    });

    const pending = subject.ready();
    await vi.advanceTimersByTimeAsync(2_000);
    await expect(pending).resolves.toEqual({
      status: "ok",
      database: "up",
      engine: "down",
    });
  });

  it("returns HTTP 503 while the slot is draining", async () => {
    const { subject } = controller({ accepting: false });

    const caught = await subject.ready().then(
      () => null,
      (error: unknown) => error,
    );
    expect(caught).toBeInstanceOf(ServiceUnavailableException);
    expect((caught as ServiceUnavailableException).getStatus()).toBe(503);
    expect((caught as ServiceUnavailableException).getResponse()).toEqual({
      status: "unavailable",
    });
  });

  it("returns HTTP 503 when the commerce database query fails", async () => {
    const { subject } = controller({
      databaseQuery: async () => {
        throw new Error("postgres refused");
      },
    });

    const caught = await subject.ready().then(
      () => null,
      (error: unknown) => error,
    );
    expect(caught).toBeInstanceOf(ServiceUnavailableException);
    expect((caught as ServiceUnavailableException).getStatus()).toBe(503);
    expect((caught as ServiceUnavailableException).getResponse()).toEqual({
      status: "unavailable",
      database: "down",
      engine: "up",
    });
  });
});
