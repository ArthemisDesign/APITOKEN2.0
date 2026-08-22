import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const root = join(import.meta.dirname);
const appRoot = join(root, "app");

function dashboardSource(): string {
  return [
    readFileSync(join(appRoot, "dashboard", "dashboard.tsx"), "utf8"),
    readFileSync(join(appRoot, "dashboard", "dashboard-shell.tsx"), "utf8"),
    ...sourceFiles(join(appRoot, "dashboard", "sections")).map((path) => readFileSync(path, "utf8")),
  ].join("\n");
}

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
    expect(shell).toContain("<AuthShell>");
    expect(shell).toContain("<AuthEntryGuard");
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

  it("uses an eight-character password minimum throughout authentication forms", () => {
    const login = readFileSync(join(appRoot, "login", "login-form.tsx"), "utf8");
    const register = readFileSync(join(appRoot, "register", "register-form.tsx"), "utf8");
    const reset = readFileSync(join(appRoot, "reset-password", "reset-password-form.tsx"), "utf8");

    expect(login).toContain("minLength={8}");
    for (const source of [register, reset]) {
      expect(source).toContain("password.length < 8");
      expect(source).toContain("at least 8 characters");
      expect(source).toContain("minLength={8}");
      expect(source).not.toContain("12 characters");
      expect(source).not.toContain("minLength={12}");
    }
  });

  it("loads Vercel Analytics once and strips sensitive URL data", () => {
    const rootLayout = readFileSync(join(appRoot, "layout.tsx"), "utf8");
    const analytics = readFileSync(join(root, "components", "site-analytics.tsx"), "utf8");
    const packageJson = readFileSync(join(root, "..", "package.json"), "utf8");
    expect(rootLayout).toContain("<SiteAnalytics />");
    expect(rootLayout).toContain("<SiteSpeedInsights />");
    expect(rootLayout).not.toContain('from "@vercel/speed-insights/next"');
    expect(analytics).toContain('import("@vercel/analytics/next")');
    expect(analytics).toContain('import("@vercel/speed-insights/next")');
    expect(analytics).toContain("ssr: false");
    expect(analytics).toContain("beforeSend");
    expect(analytics).toContain('"utm_source"');
    expect(analytics).toContain('"utm_campaign"');
    expect(analytics).toContain("query.delete(parameter)");
    expect(packageJson).toContain('"@vercel/analytics"');
    expect(packageJson).toContain('"@vercel/speed-insights"');
  });

  it("loads Yandex Metrika globally with SPA pageviews and replay privacy guards", () => {
    const rootLayout = readFileSync(join(appRoot, "layout.tsx"), "utf8");
    const analytics = readFileSync(join(root, "components", "site-analytics.tsx"), "utf8");
    const metrika = readFileSync(join(root, "lib", "yandex-metrika.ts"), "utf8");
    const authShell = readFileSync(join(root, "components", "auth-shell.tsx"), "utf8");
    const dashboard = dashboardSource();
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
    expect(docs).toContain("docs-agent-card ym-hide-content");
    expect(docs).not.toContain('id="docs-api-key"');
    expect(docs).toContain("ApiReference");
  });

  it("keeps reloadable dashboard views in the localized canonical dashboard routes", () => {
    const dashboard = dashboardSource();
    const dashboardShell = readFileSync(join(appRoot, "dashboard", "dashboard-shell.tsx"), "utf8");
    const routes = readFileSync(join(appRoot, "dashboard", "dashboard-route.ts"), "utf8");
    for (const section of ["overview", "keys", "credits", "usage", "support", "profile"]) {
      expect(dashboard).toContain(`section === \"${section}\"`);
      expect(routes).toContain(`\"${section}\"`);
    }
    expect(dashboard).not.toContain('section === "refer"');
    expect(dashboard).not.toContain('section === "orders"');
    expect(routes).not.toContain('"refer"');
    expect(routes).not.toContain('"orders"');
    expect(routes).not.toContain('"promos"');
    expect(dashboard).not.toContain("PromoPanel");
    expect(readFileSync(join(root, "lib", "api.ts"), "utf8")).not.toContain("redeemPromo");
    const messages = readFileSync(join(root, "lib", "messages.json"), "utf8");
    expect(messages).not.toMatch(/"(?:nav_promos|pr_(?:title|sub|ph|redeem|hist|code|reward|empty|ok|bad|already)|cr_promo_[tp])"/);
    expect(messages).not.toContain("Promo codes");
    expect(messages).not.toContain("Промокоды");
    expect(dashboard).not.toContain('section === "security"');
    expect(routes).toContain('value === "security"');
    expect(dashboard).not.toMatch(/href=[{\"]+\/dashboard\//);
    expect(routes).toContain('language === "ru" ? "/ru/dashboard" : "/dashboard"');
    // Подписка useSearchParams заставляет Next перерендеривать маршрут на каждом
    // pushState, поэтому начальный ?view читается одноразово из window.location.
    const dashboardMain = readFileSync(join(appRoot, "dashboard", "dashboard.tsx"), "utf8");
    expect(dashboardMain).toContain("const [section, setSection] = useState<Section>(() => parseDashboardSection(");
    expect(dashboardMain).not.toContain("useSearchParams()");
    expect(dashboard).toContain("setSection(next)");
    expect(dashboard).toContain('window.history.pushState(null, "", dashboardHref(next, language))');
    expect(dashboard).toContain('window.addEventListener("popstate", syncSectionFromHistory)');
    expect(dashboard).toContain("data-dashboard-section={item.section}");
    expect(dashboard).not.toContain("router.refresh()");
    expect(dashboard).toContain("<DashboardSidebar");
    expect(dashboard).toContain("<DashboardTopBar");
    expect(dashboard).toContain("<DashboardContent");
    expect(dashboardShell).toContain("memo(function DashboardSidebar");
    expect(dashboardShell).toContain("memo(function DashboardScrim");
    expect(dashboardShell).toContain("memo(function DashboardTopBar");
  });

  it("uses flexible whole-USD top-ups and one flat 50% B2C discount", () => {
    const pricing = readFileSync(join(root, "components", "pricing-overview.tsx"), "utf8");
    const pricingTiers = readFileSync(join(root, "lib", "pricing-tiers.ts"), "utf8");
    const messages = readFileSync(join(root, "lib", "messages.json"), "utf8");
    expect(pricingTiers).toContain("B2C_DISCOUNT_PERCENT = 50");
    expect(pricingTiers).not.toMatch(/milestone|Starter|Builder|Studio|Scale|62\.5|67\.5|tierIndex/);
    expect(messages).not.toMatch(/\bcredit packs?\b|пакет/i);
    expect(pricing).toContain("Choose any whole USD amount");
    expect(pricing).toContain("Negotiated business pricing");
    expect(pricing).not.toContain("B2C_PRICING_MILESTONES");
    expect(pricing).not.toContain("BillingFormula");
    expect(messages).toContain("$5 of platform bonus credit");
    expect(messages).toContain("приветственный бонус $5 на баланс платформы");
    expect(messages).not.toContain("$2.50");
  });

  it("advertises the exact $5 platform welcome credit in every public locale", () => {
    const welcomeCopy = [
      join(root, "lib", "messages.json"),
      join(root, "lib", "learn.ts"),
      ...sourceFiles(join(root, "lib", "learn-core-ru")),
      ...sourceFiles(join(root, "lib", "learn-core-ko")),
      ...sourceFiles(join(root, "lib", "learn-core-zh")),
      join(root, "lib", "llms.ts"),
      join(root, "lib", "md-pages.ts"),
      join(appRoot, "page.tsx"),
      join(appRoot, "models", "[slug]", "page.tsx"),
      join(root, "components", "cost-calculator.tsx"),
      join(root, "components", "compliance-pages.tsx"),
      join(appRoot, "dashboard", "dashboard.tsx"),
      join(appRoot, "dashboard", "sections", "credits.tsx"),
    ].map((path) => readFileSync(path, "utf8")).join("\n");

    expect(welcomeCopy).toContain("$5 of platform bonus credit");
    expect(welcomeCopy).toContain("приветственный бонус $5 на баланс платформы");
    expect(welcomeCopy).toContain("$5 플랫폼 웰컴 보너스 크레딧");
    expect(welcomeCopy).toContain("$5 平台欢迎奖励余额");
    expect(welcomeCopy).not.toContain("$10 of API usage at official prices");
    expect(welcomeCopy).not.toContain("$10 использования API по официальным ценам");
    expect(welcomeCopy).not.toContain("$4 welcome bonus");
    expect(welcomeCopy).not.toContain("Track-only bonus");
  });

  it("renders dashboard pricing from the account discount", () => {
    const dashboard = dashboardSource();
    const styles = [
      readFileSync(join(appRoot, "globals.css"), "utf8"),
      readFileSync(join(appRoot, "dashboard", "dashboard.css"), "utf8"),
    ].join("\n");
    // One discount prices the account. The retired policy view and the funding split had
    // exactly one writer each, and both are gone — a dashboard that still read them would
    // render a permanently empty state.
    expect(dashboard).toContain("account.pricing?.discountPercent");
    expect(dashboard).not.toContain("account.pricingPolicies");
    expect(dashboard).not.toContain("account.funding");
    expect(dashboard).not.toContain("FLAT_DISCOUNT_PERCENT");
    expect(dashboard).not.toContain("B2C_PRICING_MILESTONES");
    expect(dashboard).not.toContain("officialNanoFromCharged");
    expect(dashboard).not.toContain("modelProvider(model.model)");
    expect(styles).toContain(".app section.pricing-banner{border:1px solid var(--accent-line)}");
  });

  it("keeps the API-key section focused on issuance and management", () => {
    const dashboard = dashboardSource();
    expect(dashboard).toContain('className="agent-key-reveal secret-card key-issued-reveal"');
    expect(dashboard).not.toContain("QuickConnectDock");
    expect(dashboard).not.toContain("agent-connect-dock");
  });

  it("keeps the dashboard bilingual and authentication-aware", () => {
    const dashboard = dashboardSource();
    const dashboardCopy = readFileSync(join(root, "lib", "dashboard-copy.ts"), "utf8");
    const styles = [
      readFileSync(join(appRoot, "globals.css"), "utf8"),
      readFileSync(join(appRoot, "dashboard", "dashboard.css"), "utf8"),
    ].join("\n");
    expect(dashboard).toContain("dashboardCopy[language]");
    expect(dashboardCopy).toContain('navOverview: "Overview"');
    expect(dashboardCopy).toContain('navOverview: "Обзор"');
    expect(dashboard).toContain("user.totpEnabled");
    expect(dashboard).toContain('className="overview-primary-grid"');
    expect(dashboard).toContain('className="overview-metrics-grid"');
    expect(dashboard).toContain('className="card overview-activity"');
    expect(dashboard).toContain('className="app-top-in"');
    expect(dashboard).toContain('className="overview-pricing-facts"');
    expect(dashboard).not.toContain("copy.payPerOfficialDollar");
    expect(dashboard).toContain('<main className="app-main">');
    expect(dashboard).not.toContain("app-main-overview");
    expect(dashboard).not.toContain("app-top-up");
    expect(styles).toContain(".overview-primary-grid{display:grid;grid-template-columns:minmax(0,1.68fr) minmax(320px,.82fr);gap:20px}");
    expect(styles).toContain(".overview-metrics-grid{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:20px}");
    expect(styles).toContain(".overview-panel{display:grid;gap:20px}");
    expect(styles).toContain(".overview-balance-card{position:relative;container-type:inline-size;display:flex;");
    expect(styles).toContain("@media(max-width:960px){\n  .overview-primary-grid{grid-template-columns:1fr}\n  .overview-balance-card{grid-column:auto}");
    expect(dashboard).toContain('paidBalance: "Paid balance"');
    expect(dashboard).toContain('bonusBalance: "Welcome bonus"');
    expect(dashboardCopy).toContain('platformBalance: "Available credit"');
    expect(dashboardCopy).toContain('platformBalance: "Доступный баланс"');
    expect(dashboardCopy).toContain('usageLast30Days: "Usage · 30 days"');
    expect(dashboardCopy).toContain('usageLast30Days: "Использование · 30 дней"');
    expect(dashboard).not.toContain('title="API keys" subtitle="Create and revoke keys"');
  });

  it("serves documentation as a standalone copyable portal", () => {
    const docs = readFileSync(join(appRoot, "docs", "docs-portal.tsx"), "utf8");
    const apiReference = readFileSync(join(appRoot, "docs", "api-reference-data.ts"), "utf8");
    const agentGuideRoute = readFileSync(join(appRoot, "md", "connect", "route.ts"), "utf8");
    const dynamicRoute = readFileSync(join(appRoot, "[slug]", "page.tsx"), "utf8");
    expect(docs).toContain("docs-layout");
    expect(docs).toContain("navigator.clipboard.writeText");
    expect(docs).toContain("docs-agent-chip");
    expect(docs).toContain('copyAgent: "Скопировать"');
    expect(docs).toContain('className="docs-agent-prompt"');
    expect(docs).toContain("https://github.com/apitokensale-admin/apitoken.sale/blob/main/skills/use-apitoken/SKILL.md");
    expect(docs).not.toContain("Connection details");
    expect(docs).not.toContain("Параметры подключения");
    expect(agentGuideRoute).toContain("buildAgentSetupMarkdown");
    expect(apiReference).toContain("ROUTER_BASE_URL");
    expect(apiReference).toContain("Python SDK");
    expect(dynamicRoute).not.toContain("DocsPage");
  });

  it("advertises all supported API surfaces", () => {
    const messages = readFileSync(join(root, "lib", "messages.json"), "utf8");
    const home = readFileSync(join(appRoot, "page.tsx"), "utf8");
    const styles = readFileSync(join(appRoot, "globals.css"), "utf8");
    expect(messages).toContain("https://router.apitoken.sale");
    expect(messages).toContain("POST /v1/chat/completions");
    expect(messages).toContain("Legacy per-provider hosts (api.apitoken.sale, openai.api.apitoken.sale/v1, gemini.api.apitoken.sale) remain supported");
    expect(messages).toContain('"f2_h": "Three native API surfaces"');
    expect(messages).toContain('"f2_h": "Три нативных формата API"');
    expect(home).toContain('<Stat value="3" label="stat2" />');
    expect(home).not.toContain('className="announce"');
    expect(styles).not.toContain(".announce-");
  });

  it("keeps the verified model prices and context windows", () => {
    const marketing = readFileSync(join(root, "components", "marketing-pages.tsx"), "utf8");
    const seoModels = readFileSync(join(root, "lib", "models.ts"), "utf8");
    const integrationModels = readFileSync(join(appRoot, "docs", "integration-builder-data.ts"), "utf8");
    const llms = readFileSync(join(root, "lib", "llms.ts"), "utf8");
    expect(marketing).toContain('["Claude Opus 4.8","claude-opus-4-8","1M","$5","$25"');
    expect(marketing).toContain('["Claude Opus 4.7","claude-opus-4-7","1M","$5","$25"');
    expect(marketing).toContain('["Claude Sonnet 4.6","claude-sonnet-4-6","1M","$3","$15"');
    expect(marketing).toContain('["Claude Haiku 4.5","claude-haiku-4-5","200K","$1","$5"');
    expect(marketing).toContain('["GPT-5.6 Sol","gpt-5.6-sol","400K",formatUsd(solRates.inputPerM),formatUsd(solRates.outputPerM)');
    expect(seoModels).toContain("export const GPT_56_SOL_PROMO_END_UNIX = 1_795_305_600;");
    expect(marketing).toContain('["GPT-5.6 Terra","gpt-5.6-terra","400K","$2","$12"');
    expect(marketing).toContain('["GPT-5.6 Luna","gpt-5.6-luna","400K","$0.20","$1.20"');
    expect(marketing).toContain('["GPT-5.5","gpt-5.5","400K","$5","$30"');
    expect(marketing).toContain('["GPT-5.4","gpt-5.4","400K","$2.50","$15"');
    expect(marketing).toContain('const geminiFlashRates = geminiFlashPricingAt();');
    expect(marketing).toContain('const geminiFlashInput = `${formatUsd(geminiFlashRates.inputPerM)}*`;');
    expect(marketing).toContain('const geminiFlashOutput = `${formatUsd(geminiFlashRates.outputPerM)}*`;');
    expect(marketing).toContain('["Gemini 3.5 Flash","gemini-3.5-flash","1M","$1.50","$9.00"');
    expect(marketing).toContain('["Gemini 3 Flash Preview","gemini-3-flash-preview","1M","$0.50","$3.00"');
    expect(marketing).toContain('["Gemini 3.1 Pro Preview","gemini-3.1-pro-preview","1M","$2*","$12*"');
    expect(marketing).toContain('["Gemini 3.1 Flash-Lite","gemini-3.1-flash-lite","1M","$0.25","$1.50"');
    expect(marketing).toContain('["Gemini 2.5 Flash","gemini-2.5-flash","1M","$0.30","$2.50"');
    expect(marketing).toContain('["Gemini 2.5 Flash-Lite","gemini-2.5-flash-lite","1M","$0.10","$0.40"');
    expect(marketing).toContain('["Gemini 3.1 Flash Image (Nano Banana 2)","gemini-3.1-flash-image","128K","$0.50","$3.00"');
    for (const publicSurface of [marketing, seoModels, integrationModels, llms]) {
      expect(publicSurface).toContain("gemini-3-flash-preview");
    }
    expect(seoModels).toContain("cachedInputPerM: 0.5,\n    outputPerM: 3,\n    imageOutputPerM: 60");
    expect(seoModels).toContain("Supports minimal, low, medium and high thinking levels");
  });

  it("keeps the header, workflow hover, and wave loop regression-safe", () => {
    const header = readFileSync(join(root, "components", "site-chrome.tsx"), "utf8");
    const topup = readFileSync(join(root, "components", "topup-amount-input.tsx"), "utf8");
    const motion = readFileSync(join(root, "components", "motion-effects.tsx"), "utf8");
    const styles = readFileSync(join(appRoot, "globals.css"), "utf8");
    const animations = readFileSync(join(appRoot, "anim.css"), "utf8");
    expect(header).not.toContain('k="nav_features"');
    expect(header).not.toContain('k="nav_faq"');
    expect(header).not.toContain("api.logout");
    expect(header).toContain('onClick={() => setLanguage("en")}');
    expect(header).toContain('onClick={() => setLanguage("ru")}');
    expect(header).not.toContain("englishPath");
    expect(header).not.toContain("russianPath");
    expect(styles).toContain("header.nav{position:fixed");
    expect(styles).toContain(".nav-links{display:flex;align-items:center;justify-content:space-evenly");
    expect(styles).not.toContain(".nav-links{display:grid;grid-template-columns:");
    expect(styles).toContain("grid-template-columns:repeat(2,134px)");
    expect(styles).toContain(".term-controls i:hover::after");
    expect(styles).not.toContain(".term-controls:hover i::after");
    expect(styles).toContain("inset:0;display:grid;place-items:center");
    expect(styles).toContain(".pricing-intro{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:22px;align-items:stretch}");
    expect(styles).toContain(".business-card,.topup-card{padding:30px;display:flex;flex-direction:column");
    expect(styles).toContain(".business-preview{overflow:hidden;border:1px solid var(--line-strong)");
    expect(styles).toContain(".business-preview-head{height:82px;min-height:82px");
    expect(styles).toContain(".business-terms{display:grid;grid-template-columns:repeat(2,minmax(0,1fr))");
    expect(styles).toContain(".topup-live + p,.business-preview + p{margin-top:18px}");
    expect(styles).toContain(".offer-table-head,.ot{display:grid;grid-template-columns:minmax(0,.8fr) 80px minmax(0,1.1fr)");
    expect(styles).toContain(".ot-best{background:var(--accent-soft);box-shadow:inset 3px 0 0 var(--accent)}");
    expect(styles).not.toContain(".business-preview{flex:1 1 auto;min-height:168px");
    expect(styles).toContain(".stat b{font-family:var(--font-mono)");
    expect(styles).toContain(".prod{border:1px solid var(--line);border-radius:8px;padding:28px;background:var(--bg-card);display:flex;flex-direction:column");
    expect(styles).toContain(".prod h3{margin:14px 0 18px;font-family:var(--font-i18n-display)");
    expect(topup).toContain('inputMode="numeric"');
    expect(topup).toContain('pattern="[1-9][0-9]*"');
    expect(topup).not.toContain("editable");
    expect(styles).toContain(".prod .amt .now{font-family:var(--font-mono)");
    expect(styles).not.toContain(".hero-note{");
    const home = readFileSync(join(appRoot, "page.tsx"), "utf8");
    expect(home).not.toContain('k="hero_note"');
    expect(home).toContain('k="offer_free_eyebrow"');
    expect(home).toContain('className="offer-value-table"');
    expect(home).toContain("−{row.discount}%");
    expect(animations).not.toContain(".feat:hover{");
    expect(motion).toContain("transform={`translate(${waveWidth} 0)`}");
    expect(animations).toContain("translateX(-50%)");
  });
});
