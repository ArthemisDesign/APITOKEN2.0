// Заглушка для рантайм-модулей Next в юнит-тестах (см. vitest.config.ts).
export function cookies(): never {
  throw new Error("next/headers is not available in unit tests");
}
