import { describe, expect, it, vi } from "vitest";
import { AdminPipelinesController } from "./admin-pipelines.controller.js";
import type { AdminPipelinesService } from "./admin-pipelines.service.js";

describe("admin pipelines HTTP contract", () => {
  it("passes the pipeline health through unchanged", async () => {
    const pipelines = { pipelineHealth: vi.fn() };
    const controller = new AdminPipelinesController(
      pipelines as unknown as AdminPipelinesService,
    );
    pipelines.pipelineHealth.mockResolvedValue({ verdict: "ok" });

    await expect(controller.pipelineHealth()).resolves.toEqual({ verdict: "ok" });
    expect(pipelines.pipelineHealth).toHaveBeenCalledTimes(1);
    // Эндпоинт без параметров: сервис вызывается без аргументов.
    expect(pipelines.pipelineHealth).toHaveBeenCalledWith();
  });
});
