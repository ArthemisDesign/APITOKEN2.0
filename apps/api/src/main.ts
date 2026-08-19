import "reflect-metadata";
import { Logger } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { createApplication } from "./application.js";
import type { Environment } from "./config.js";
import { ReadinessService } from "./readiness.service.js";

function installDrainFailsafe(deadlineMs: number): void {
  let armed = false;
  const arm = (): void => {
    if (armed) return;
    armed = true;
    setTimeout(() => process.exit(0), deadlineMs).unref();
  };
  process.once("SIGTERM", arm);
  process.once("SIGINT", arm);
}

async function bootstrap(): Promise<void> {
  const app = await createApplication();
  const readiness = app.get(ReadinessService);
  process.on("SIGUSR1", () => {
    readiness.markDraining();
    Logger.log("pre-drain: readiness flipped to draining (still serving in-flight)", "Bootstrap");
  });
  const config = app.get(ConfigService<Environment, true>);
  app.enableShutdownHooks();
  installDrainFailsafe(config.get("API_DRAIN_DEADLINE_MS", { infer: true }));

  const host = config.get("HOST", { infer: true });
  const port = config.get("PORT", { infer: true });
  await app.listen(port, host);
  Logger.log(`commercial API listening on http://${host}:${port}`, "Bootstrap");
}

void bootstrap();
