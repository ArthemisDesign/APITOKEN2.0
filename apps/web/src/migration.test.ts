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

  it("keeps persistent public and authentication shells across client navigation", () => {
    const rootLayout = readFileSync(join(appRoot, "layout.tsx"), "utf8");
    const shell = readFileSync(join(root, "components", "persistent-route-shell.tsx"), "utf8");
    const complianceLayout = readFileSync(join(appRoot, "(compliance)", "layout.tsx"), "utf8");
    const compliance = readFileSync(join(root, "components", "compliance-pages.tsx"), "utf8");
    const home = readFileSync(join(appRoot, "page.tsx"), "utf8");
    const marketing = readFileSync(join(root, "components", "marketing-pages.tsx"), "utf8");
    expect(rootLayout).toContain("<PersistentRouteShell>{children}</PersistentRouteShell>");
    expect(shell).toContain("<SiteHeader home={home} />");
    expect(shell).toContain("<SiteFooter full />");
    expect(shell).toContain("<MotionEffects />");
    expect(shell).toContain("<AuthShell>{children}</AuthShell>");
    for (const route of ["/", "/models", "/integrations", "/plans", "/privacy", "/terms", "/support"]) expect(shell).toContain(`"${route}"`);
    for (const route of ["/login", "/register", "/forgot-password", "/reset-password", "/verify-email"]) expect(shell).toContain(`"${route}"`);
    expect(complianceLayout).toContain("<main>{children}</main>");
    expect(home).not.toContain("<SiteHeader");
    expect(home).not.toContain("<SiteFooter");
    expect(marketing).not.toContain("<SiteHeader");
    expect(marketing).not.toContain("<SiteFooter");
    expect(compliance).not.toContain("<SiteHeader />");
    expect(compliance).not.toContain("<SiteFooter />");
    for (const route of ["/privacy", "/terms", "/support", "/plans"]) expect(compliance).toContain(`href: \"${route}\"`);
    const authSource = ["login/login-form.tsx", "register/register-form.tsx", "forgot-password/forgot-password-form.tsx", "reset-password/reset-password-form.tsx", "verify-email/verify-email.tsx", "auth/callback/oauth-callback.tsx"]
      .map((path) => readFileSync(join(appRoot, path), "utf8")).join("\n");
    expect(authSource).not.toContain("<AuthShell");
    expect(authSource).not.toContain("router.refresh()");
  });

  it("limits the advertised welcome bonus to Google and GitHub authentication", () => {
    const authShell = readFileSync(join(root, "components", "auth-shell.tsx"), "utf8");
    const login = readFileSync(join(appRoot, "login", "login-form.tsx"), "utf8");
    const register = readFileSync(join(appRoot, "register", "register-form.tsx"), "utf8");
    const messages = readFileSync(join(root, "lib", "messages.json"), "utf8");
    expect(authShell).toContain('className="auth-bonus"');
    expect(login).toContain("<WelcomeBonusNotice />");
    expect(register).toContain("<WelcomeBonusNotice />");
    expect(messages).toContain("new accounts created with Google or GitHub only");
    expect(messages).toContain("При регистрации по email и паролю бонус не начисляется");
  });

  it("loads Vercel Analytics once and strips sensitive URL data", () => {
    const rootLayout = readFileSync(join(appRoot, "layout.tsx"), "utf8");
    const analytics = readFileSync(join(root, "components", "site-analytics.tsx"), "utf8");
    const packageJson = readFileSync(join(root, "..", "package.json"), "utf8");
    expect(rootLayout).toContain("<SiteAnalytics />");
    expect(analytics).toContain('from "@vercel/analytics/next"');
    expect(analytics).toContain("beforeSend");
    expect(analytics).toContain('"utm_source"');
    expect(analytics).toContain('"utm_campaign"');
    expect(analytics).toContain("query.delete(parameter)");
    expect(packageJson).toContain('"@vercel/analytics"');
  });

  it("loads Yandex Metrika globally with SPA pageviews and replay privacy guards", () => {
    const rootLayout = readFileSync(join(appRoot, "layout.tsx"), "utf8");
    const analytics = readFileSync(join(root, "components", "site-analytics.tsx"), "utf8");
    const metrika = readFileSync(join(root, "lib", "yandex-metrika.ts"), "utf8");
    const authShell = readFileSync(join(root, "components", "auth-shell.tsx"), "utf8");
    const dashboard = readFileSync(join(appRoot, "dashboard", "dashboard.tsx"), "utf8");
    const docs = readFileSync(join(appRoot, "docs", "docs-portal.tsx"), "utf8");

    expect(rootLayout).toContain('id="yandex-metrika"');
    expect(rootLayout).toContain("https://mc.yandex.ru/watch/");
    expect(metrika).toContain("110788366");
    expect(metrika).toContain("https://mc.yandex.ru/metrika/tag.js");
    expect(metrika).toContain("webvisor:true");
    expect(metrika).toContain("clickmap:true");
    expect(metrika).toContain("ecommerce:'dataLayer'");
    expect(metrika).not.toContain("defer:true");
    expect(metrika).toContain('"utm_source"');
    expect(metrika).toContain('"utm_campaign"');
    expect(metrika).toContain("url:pageUrl.href");
    expect(analytics).toContain('window.ym?.(YANDEX_METRIKA_ID, "hit", location.origin + pathname');
    expect(authShell).toContain("auth-card ym-hide-content");
    expect(dashboard).toContain("app ym-hide-content");
    expect(docs).toContain("ym-disable-keys");
    expect(docs).toContain("docs-code-card ym-hide-content");
  });

  it("keeps reloadable dashboard views in the localized canonical dashboard routes", () => {
    const dashboard = readFileSync(join(appRoot, "dashboard", "dashboard.tsx"), "utf8");
    const routes = readFileSync(join(appRoot, "dashboard", "dashboard-route.ts"), "utf8");
    for (const section of ["overview", "keys", "credits", "promos", "usage", "support", "profile"]) {
      expect(dashboard).toContain(`section === \"${section}\"`);
      expect(routes).toContain(`\"${section}\"`);
    }
    expect(dashboard).not.toContain('section === "refer"');
    expect(dashboard).not.toContain('section === "orders"');
    expect(routes).not.toContain('"refer"');
    expect(routes).not.toContain('"orders"');
    expect(dashboard).not.toContain('section === "security"');
    expect(routes).not.toContain('"security"');
    expect(dashboard).not.toMatch(/href=[{\"]+\/dashboard\//);
    expect(routes).toContain('language === "ru" ? "/ru/dashboard" : "/dashboard"');
    expect(dashboard).toContain("const [section, setSection] = useState<Section>(() => parseDashboardSection(searchParams.get(\"view\")))");
    expect(dashboard).toContain("setSection(next)");
    expect(dashboard).toContain('window.history.pushState(null, "", dashboardHref(next, language))');
    expect(dashboard).toContain('window.addEventListener("popstate", syncSectionFromHistory)');
    expect(dashboard).toContain("data-dashboard-section={item.section}");
    expect(dashboard).not.toContain("router.refresh()");
  });

  it("uses flexible whole-USD top-ups and the authoritative pricing tiers", () => {
    const pricing = readFileSync(join(root, "components", "pricing-overview.tsx"), "utf8");
    const pricingTiers = readFileSync(join(root, "lib", "pricing-tiers.ts"), "utf8");
    const messages = readFileSync(join(root, "lib", "messages.json"), "utf8");
    for (const value of ["starter", "builder", "pro", "studio", "scale", "60", "65", "70", "75", "80"]) {
      expect(pricingTiers).toContain(value);
    }
    for (const threshold of ["100", "250", "500", "1000"]) {
      expect(pricingTiers).toContain(`platformSpendUsd: "${threshold}"`);
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
    expect(dashboardCopy).toContain('monthlyTierProgress: "Tier progress"');
    expect(dashboardCopy).toContain('spendMore: "Top up {amount} more"');
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
    expect(dashboard).toContain("user.totpEnabled");
    expect(dashboard).toContain('className="overview-core"');
    expect(dashboard).toContain("officialBalance");
    expect(dashboardCopy).toContain('platformBalance: "Platform balance"');
    expect(dashboardCopy).toContain('platformBalance: "Баланс платформы"');
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

  it("advertises only the supported Anthropic API format", () => {
    const messages = readFileSync(join(root, "lib", "messages.json"), "utf8");
    const home = readFileSync(join(appRoot, "page.tsx"), "utf8");
    const styles = readFileSync(join(appRoot, "globals.css"), "utf8");
    expect(messages).not.toMatch(/openai/i);
    expect(messages).toContain('"f2_h": "Anthropic-native API"');
    expect(messages).toContain('"f2_h": "Нативный Anthropic API"');
    expect(home).toContain('<Stat value="1" label="stat2" />');
    expect(home).not.toContain('className="announce"');
    expect(styles).not.toContain(".announce-");
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
    expect(styles).toContain(".topup-card{padding:30px;display:flex;flex-direction:column");
    expect(styles).toContain(".business-card{padding:30px;display:flex;flex-direction:column");
    expect(styles).toContain(".business-preview{flex:1 1 auto;min-height:168px");
    expect(styles).toContain(".stat b{font-family:var(--font-mono)");
    expect(styles).toContain(".prod{border:1px solid var(--line);border-radius:8px;padding:28px;background:var(--bg-card);display:flex;flex-direction:column");
    expect(styles).toContain(".prod h3{margin:14px 0 18px;font-family:var(--font-i18n-display)");
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
