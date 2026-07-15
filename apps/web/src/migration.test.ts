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
      "models", "integrations", "int-claude-code", "int-cursor", "int-cline",
      "int-continue", "int-zed", "int-sdk",
    ]) expect(staticRoute).toContain(`\"${route}\"`);
    for (const route of ["plans", "terms", "privacy", "support"]) {
      expect(existsSync(join(appRoot, "(compliance)", route, "page.tsx"))).toBe(true);
    }
    for (const route of ["login", "register", "dashboard", "docs"]) {
      expect(existsSync(join(appRoot, route, "page.tsx"))).toBe(true);
    }
    expect(staticRoute).not.toContain('slug === "docs"');
  });

  it("keeps one persistent shell while navigating between compliance pages", () => {
    const layout = readFileSync(join(appRoot, "(compliance)", "layout.tsx"), "utf8");
    const compliance = readFileSync(join(root, "components", "compliance-pages.tsx"), "utf8");
    expect(layout).toContain("<SiteHeader />");
    expect(layout).toContain("<SiteFooter />");
    expect(layout).toContain("<main>{children}</main>");
    expect(compliance).not.toContain("<SiteHeader />");
    expect(compliance).not.toContain("<SiteFooter />");
    for (const route of ["/privacy", "/terms", "/support", "/plans"]) expect(compliance).toContain(`href: \"${route}\"`);
  });

  it("keeps reloadable dashboard views in the single canonical /dashboard route", () => {
    const dashboard = readFileSync(join(appRoot, "dashboard", "dashboard.tsx"), "utf8");
    const routes = readFileSync(join(appRoot, "dashboard", "dashboard-route.ts"), "utf8");
    for (const section of ["overview", "keys", "credits", "refer", "promos", "usage", "orders", "profile", "security"]) {
      expect(dashboard).toContain(`section === \"${section}\"`);
      expect(routes).toContain(`\"${section}\"`);
    }
    expect(dashboard).not.toMatch(/href=[{\"]+\/dashboard\//);
    expect(routes).toContain('`/dashboard?view=${section}`');
    expect(dashboard).toContain("const [section, setSection] = useState<Section>(() => parseDashboardSection(searchParams.get(\"view\")))");
    expect(dashboard).toContain("setSection(next)");
    expect(dashboard).toContain('window.history.pushState(null, "", dashboardHref(next))');
    expect(dashboard).toContain('window.addEventListener("popstate", syncSectionFromHistory)');
    expect(dashboard).toContain("data-dashboard-section={item.section}");
  });

  it("uses flexible whole-USD top-ups and the authoritative pricing tiers", () => {
    const pricing = readFileSync(join(root, "components", "pricing-overview.tsx"), "utf8");
    const pricingTiers = readFileSync(join(root, "lib", "pricing-tiers.ts"), "utf8");
    const messages = readFileSync(join(root, "lib", "messages.json"), "utf8");
    for (const value of ["starter", "builder", "pro", "studio", "scale", "60", "65", "70", "75", "80", "25", "75", "200", "500", "2500"]) {
      expect(pricingTiers).toContain(value);
    }
    expect(messages).not.toMatch(/\bcredit packs?\b|пакет/i);
    expect(pricing).toContain("Choose any whole USD amount");
    expect(pricing).toContain("Negotiated business pricing");
    expect(pricing).toContain("B2C_PRICING_MILESTONES.map");
    expect(pricingTiers).toContain('{ code: "builder", label: "Builder", messageKey: "tier_builder", discountPercent: 65');
    expect(pricingTiers).toContain('{ code: "pro", label: "Pro", messageKey: "tier_pro", discountPercent: 70');
    expect(pricingTiers).toContain('{ code: "studio", label: "Studio", messageKey: "tier_studio", discountPercent: 75');
    expect(pricingTiers).toContain('{ code: "scale", label: "Scale", messageKey: "tier_scale", discountPercent: 80');
    expect(pricing).not.toContain("BillingFormula");
    expect(messages).toContain("$10 of Claude usage at official API prices");
    expect(messages).toContain("$10 на Claude по официальным ценам API");
    expect(messages).not.toContain("$2.50");
  });

  it("renders dashboard pricing as a complete milestone track", () => {
    const dashboard = readFileSync(join(appRoot, "dashboard", "dashboard.tsx"), "utf8");
    const dashboardCopy = readFileSync(join(root, "lib", "dashboard-copy.ts"), "utf8");
    const styles = readFileSync(join(appRoot, "globals.css"), "utf8");
    expect(dashboardCopy).toContain('monthlyTierProgress: "Monthly tier progress"');
    expect(dashboardCopy).toContain('spendMore: "Spend {amount} more"');
    expect(dashboard).toContain("B2C_PRICING_MILESTONES.map");
    expect(styles).toContain(".pricing-milestone-track");
    expect(styles).toContain("height:var(--tier-progress)");
    expect(styles).toContain(".app section.pricing-banner{border:1px solid var(--accent-line)}");
  });

  it("keeps the dashboard bilingual and authentication-aware", () => {
    const dashboard = readFileSync(join(appRoot, "dashboard", "dashboard.tsx"), "utf8");
    const dashboardCopy = readFileSync(join(root, "lib", "dashboard-copy.ts"), "utf8");
    expect(dashboard).toContain("dashboardCopy[language]");
    expect(dashboardCopy).toContain('navOverview: "Overview"');
    expect(dashboardCopy).toContain('navOverview: "Обзор"');
    expect(dashboard).toContain("user.passwordEnabled ?");
    expect(dashboard).toContain("ov-tiles ov-tiles-two");
    expect(dashboard).not.toContain('title="API keys" subtitle="Create and revoke keys"');
  });

  it("serves documentation as a standalone copyable portal", () => {
    const docs = readFileSync(join(appRoot, "docs", "docs-portal.tsx"), "utf8");
    const dynamicRoute = readFileSync(join(appRoot, "[slug]", "page.tsx"), "utf8");
    expect(docs).toContain("docs-layout");
    expect(docs).toContain("navigator.clipboard.writeText");
    expect(docs).toContain("ANTHROPIC_BASE_URL");
    expect(docs).toContain("Python SDK");
    expect(dynamicRoute).not.toContain("DocsPage");
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
    expect(header).not.toContain("api.logout");
    expect(styles).toContain("header.nav{position:fixed");
    expect(styles).toContain("grid-template-columns:repeat(2,134px)");
    for (const control of ["term-close", "term-minimize", "term-zoom"]) expect(terminal).toContain(control);
    expect(terminal).not.toContain("onPointerMove");
    expect(styles).toContain(".term-controls i:hover::after");
    expect(styles).not.toContain(".term-controls:hover i::after");
    expect(styles).toContain("inset:0;display:grid;place-items:center");
    expect(styles).toContain(".pricing-intro{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:22px;align-items:stretch}");
    expect(styles).toContain(".topup-card,.business-card{padding:30px;display:grid;grid-template-rows:");
    expect(styles).toContain(".business-preview{justify-content:space-between");
    expect(styles).toContain(".stat b{font-family:var(--font-mono)");
    expect(styles).toContain(".prod{border:1px solid var(--line);border-radius:8px;padding:28px;background:var(--bg-card);display:grid");
    expect(topup).toContain('inputMode="numeric"');
    expect(topup).toContain('pattern="[1-9][0-9]*"');
    expect(topup).not.toContain("editable");
    expect(styles).toContain(".prod .amt .now{font-family:var(--font-mono)");
    expect(styles).not.toContain(".hero-note{");
    expect(readFileSync(join(appRoot, "page.tsx"), "utf8")).not.toContain('k="hero_note"');
    expect(animations).not.toContain(".feat:hover{");
    expect(motion).toContain("transform={`translate(${waveWidth} 0)`}");
    expect(animations).toContain("translateX(-50%)");
  });
});
