import "server-only";
import { EngineClient } from "@claude-api/engine-client";
import { loadConfig } from "./config";

let cached: EngineClient | undefined;

/** Control API движка. CONTROL_KEY живёт только здесь и в браузер не уходит. */
export function getEngineClient(): EngineClient {
  if (!cached) {
    const config = loadConfig();
    cached = new EngineClient({
      baseUrl: config.engineBaseUrl,
      controlKey: config.engineControlKey,
    });
  }
  return cached;
}
