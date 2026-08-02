import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

const stub = fileURLToPath(new URL("./test/stubs/empty.ts", import.meta.url));
const source = fileURLToPath(new URL("./src", import.meta.url));

export default defineConfig({
  resolve: {
    alias: {
      "@": source,
      // Оба модуля существуют только внутри рантайма Next и в юнит-тестах не нужны:
      // проверяемая логика чистая и получает конфиг параметром.
      "server-only": stub,
      "next/headers": stub,
    },
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
