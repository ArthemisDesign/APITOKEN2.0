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

  it("uses flexible whole-USD top-ups and the authoritative pricing tiers", () => {
    const pricing = readFileSync(join(root, "components", "pricing-overview.tsx"), "utf8");
    const messages = readFileSync(join(root, "lib", "messages.json"), "utf8");
    for (const value of ["60%", "65%", "70%", "75%", "80%", "$25", "$75", "$200", "$500", "$2,500"]) {
      expect(pricing).toContain(value);
    }
    expect(messages).not.toMatch(/\bcredit packs?\b|пакет/i);
    expect(pricing).toContain("Choose any whole USD amount");
    expect(pricing).toContain("Negotiated business pricing");
    expect(pricing).toContain('"tier_builder", "65%", "$25", "≈ $71"');
    expect(pricing).toContain('"tier_pro", "70%", "$75", "$250"');
    expect(pricing).toContain('"tier_studio", "75%", "$200", "$800"');
    expect(pricing).toContain('"tier_scale", "80%", "$500", "$2,500"');
  });

  it("keeps the verified model prices and context windows", () => {
    const marketing = readFileSync(join(root, "components", "marketing-pages.tsx"), "utf8");
    expect(marketing).toContain('["Claude Opus 4.8","claude-opus-4-8","1M","$5","$25"');
    expect(marketing).toContain('["Claude Opus 4.7","claude-opus-4-7","1M","$5","$25"');
    expect(marketing).toContain('["Claude Sonnet 4.6","claude-sonnet-4-6","1M","$3","$15"');
    expect(marketing).toContain('["Claude Haiku 4.5","claude-haiku-4-5","200K","$1","$5"');
  });

  it("keeps the header, terminal, workflow hover, and wave loop regression-safe", () => {
    const header = readFileSync(join(root, "components", "site-chrome.tsx"), "utf8");
    const terminal = readFileSync(join(root, "components", "interactive-terminal.tsx"), "utf8");
    const topup = readFileSync(join(root, "components", "topup-amount-input.tsx"), "utf8");
    const motion = readFileSync(join(root, "components", "motion-effects.tsx"), "utf8");
    const styles = readFileSync(join(appRoot, "globals.css"), "utf8");
    const animations = readFileSync(join(appRoot, "anim.css"), "utf8");
    expect(header).not.toContain('k="nav_features"');
    expect(header).not.toContain('k="nav_faq"');
    expect(styles).toContain("header.nav{position:fixed");
    expect(styles).toContain("grid-template-columns:repeat(2,134px)");
    for (const control of ["term-close", "term-minimize", "term-zoom"]) expect(terminal).toContain(control);
    expect(terminal).not.toContain("onPointerMove");
    expect(topup).toContain('inputMode="numeric"');
    expect(topup).toContain('pattern="[1-9][0-9]*"');
    expect(animations).not.toContain(".feat:hover{");
    expect(motion).toContain("transform={`translate(${waveWidth} 0)`}");
    expect(animations).toContain("translateX(-50%)");
  });
});
