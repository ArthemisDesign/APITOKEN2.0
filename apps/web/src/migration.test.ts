import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const root = join(import.meta.dirname);
const appRoot = join(root, "app");

function sourceFiles(directory: string): string[] {
  return readdirSync(directory).flatMap((name) => {
    const path = join(directory, name);
    return statSync(path).isDirectory() ? sourceFiles(path) : /\.(tsx?|css)$/.test(name) ? [path] : [];
  });
}

describe("completed Next.js migration", () => {
  it("does not retain the legacy HTML renderer or injected markup", () => {
    expect(existsSync(join(root, "legacy"))).toBe(false);
    const source = sourceFiles(root).filter((path) => !path.endsWith(".test.ts") && !path.endsWith("layout.tsx")).map((path) => readFileSync(path, "utf8")).join("\n");
    expect(source).not.toContain("dangerouslySetInnerHTML");
    expect(source).not.toContain("LegacyPage");
    expect(source).not.toContain("legacyBody");
  });

  it("owns every migrated public page through App Router components", () => {
    const staticRoute = readFileSync(join(appRoot, "[slug]", "page.tsx"), "utf8");
    for (const route of [
      "plans", "models", "docs", "integrations", "int-claude-code", "int-cursor", "int-cline",
      "int-continue", "int-zed", "int-sdk", "terms", "privacy",
    ]) expect(staticRoute).toContain(`\"${route}\"`);
    for (const route of ["login", "register", "dashboard"]) {
      expect(existsSync(join(appRoot, route, "page.tsx"))).toBe(true);
    }
  });

  it("keeps dashboard sections in the single canonical /dashboard route", () => {
    const dashboard = readFileSync(join(appRoot, "dashboard", "dashboard.tsx"), "utf8");
    for (const section of ["overview", "keys", "credits", "refer", "promos", "usage", "orders", "profile", "security"]) {
      expect(dashboard).toContain(`section === \"${section}\"`);
    }
    expect(dashboard).not.toMatch(/href=[{\"]+\/dashboard\//);
  });
});
