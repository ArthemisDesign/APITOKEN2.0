import { spawn } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";

const baseUrl = process.env.SITE_URL ?? "http://localhost:3001";
const outputDirectory = path.resolve(process.env.SCREENSHOT_DIR ?? ".artifacts/site-audit");
const auditScope = process.env.AUDIT_SCOPE ?? "site";
const auditFilter = new Set((process.env.AUDIT_FILTER ?? "").split(",").map((value) => value.trim()).filter(Boolean));
const chromeCandidates = [
  process.env.CHROME_PATH,
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/Applications/Chromium.app/Contents/MacOS/Chromium",
  "/usr/bin/google-chrome",
  "/usr/bin/chromium",
].filter(Boolean);

const siteCaptures = [
  ["home-desktop", "/", 1440, 1000, "light"],
  ["home-mobile", "/", 390, 844, "light"],
  ["home-dark", "/", 1440, 1000, "dark"],
  ["home-russian", "/", 1440, 1000, "dark", "ru"],
  ["home-authenticated", "/?audit-auth=1", 1440, 1000, "light"],
  ["plans-desktop", "/plans", 1440, 1000, "light"],
  ["plans-mobile", "/plans", 390, 844, "light"],
  ["plans-dark", "/plans", 1440, 1000, "dark"],
  ["models-desktop", "/models", 1440, 1000, "light"],
  ["models-dark", "/models", 1440, 1000, "dark"],
  ["docs-desktop", "/docs", 1440, 1000, "light"],
  ["docs-dark", "/docs", 1440, 1000, "dark"],
  ["docs-mobile", "/docs", 390, 844, "light"],
  ["integrations-desktop", "/integrations", 1440, 1000, "light"],
  ["integration-guide-desktop", "/int-claude-code", 1440, 1000, "light"],
  ["login-desktop", "/login", 1440, 1000, "light"],
  ["register-desktop", "/register", 1440, 1000, "light"],
  ["register-dark", "/register", 1440, 1000, "dark"],
  ["terms-desktop", "/terms", 1440, 1000, "light"],
  ["privacy-desktop", "/privacy", 1440, 1000, "light"],
];

const dashboardCaptures = [
  ["dashboard-overview-light", "/dashboard", 1440, 1000, "light"],
  ["dashboard-overview-dark", "/dashboard", 1440, 1000, "dark"],
  ["dashboard-overview-russian", "/dashboard", 1440, 1000, "dark", "ru"],
  ["dashboard-keys-light", "/dashboard?view=keys", 1440, 1000, "light"],
  ["dashboard-keys-dark", "/dashboard?view=keys", 1440, 1000, "dark"],
  ["dashboard-topup-light", "/dashboard?view=credits", 1440, 1000, "light"],
  ["dashboard-topup-dark", "/dashboard?view=credits", 1440, 1000, "dark"],
  ["dashboard-usage-light", "/dashboard?view=usage", 1440, 1000, "light"],
  ["dashboard-usage-dark", "/dashboard?view=usage", 1440, 1000, "dark"],
  ["dashboard-referral-light", "/dashboard?view=refer", 1440, 1000, "light"],
  ["dashboard-referral-dark", "/dashboard?view=refer", 1440, 1000, "dark"],
  ["dashboard-promos-light", "/dashboard?view=promos", 1440, 1000, "light"],
  ["dashboard-promos-dark", "/dashboard?view=promos", 1440, 1000, "dark"],
  ["dashboard-orders-light", "/dashboard?view=orders", 1440, 1000, "light"],
  ["dashboard-orders-dark", "/dashboard?view=orders", 1440, 1000, "dark"],
  ["dashboard-profile-light", "/dashboard?view=profile", 1440, 1000, "light"],
  ["dashboard-profile-dark", "/dashboard?view=profile", 1440, 1000, "dark"],
  ["dashboard-security-light", "/dashboard?view=security", 1440, 1000, "light"],
  ["dashboard-security-dark", "/dashboard?view=security", 1440, 1000, "dark"],
  ["dashboard-overview-mobile", "/dashboard", 390, 844, "light"],
  ["dashboard-keys-mobile-dark", "/dashboard?view=keys", 390, 844, "dark"],
];

const scopedCaptures = auditScope === "dashboard" ? dashboardCaptures :
  auditScope === "all" ? [...siteCaptures, ...dashboardCaptures] : siteCaptures;
const captures = auditFilter.size > 0 ? scopedCaptures.filter(([name]) => auditFilter.has(name)) : scopedCaptures;

if (captures.length === 0) throw new Error("No screenshots matched AUDIT_SCOPE/AUDIT_FILTER.");

const dashboardFixtureScript = `(() => {
  const originalFetch = window.fetch.bind(window);
  const apiBase = "https://backend.apitoken.sale/v1";
  const json = (body, status = 200) => Promise.resolve(new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  }));
  const user = {
    id: "9d3b0b02-b864-4e77-b690-e3c252c44a9e",
    email: "dashboard.audit@apitoken.sale",
    emailVerified: true,
    passwordEnabled: false,
    engineAccountStatus: "active",
    customerType: "b2c",
  };
  const account = {
    balanceNano: "4000000000",
    reservedNano: "0",
    spentNano: "12000000000",
    balanceUsd: "4.00",
    markupBasisPoints: 4000,
    status: "active",
    pricing: {
      customerType: "b2c",
      pricingMode: "progressive",
      monthStart: "2026-07-01T00:00:00.000Z",
      tier: "starter",
      discountPercent: 60,
      multiplierBp: 4000,
      spentNano: "12000000000",
      retentionSpendNano: "0",
      nextTier: {
        tier: "builder",
        discountPercent: 65,
        spendThresholdNano: "25000000000",
        remainingNano: "13000000000",
        visibleOfficialUsageUsd: "70.00",
      },
    },
  };
  const keys = [{
    id: "3df4f03d-e5e8-4811-bcea-d32e9f6f20c0",
    label: "Production",
    keyMasked: "sk-pool-a5b5••••••••eeb",
    status: "active",
    spentNano: "12000000000",
    spentUsd: "12.00",
    createdAt: "2026-07-15T08:30:00.000Z",
  }];
  const entries = [
    { id: "ledger-1", kind: "charge", amountNano: "185000000", amountUsd: "0.185", keyMasked: "sk-pool-a5b5••••••••eeb", reference: "req_01K0", balanceAfterNano: "4000000000", timestamp: "1784109600" },
    { id: "ledger-2", kind: "topup", amountNano: "4000000000", amountUsd: "4.00", keyMasked: null, reference: "welcome_credit", balanceAfterNano: "4185000000", timestamp: "1784106000" },
  ];
  window.fetch = (input, init = {}) => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    if (!url.startsWith(apiBase)) return originalFetch(input, init);
    const parsed = new URL(url);
    const path = parsed.pathname.slice("/v1".length);
    if (location.search.includes("audit-auth=1") && path === "/auth/me") return json({ user });
    if (!location.pathname.startsWith("/dashboard")) return originalFetch(input, init);
    if (path === "/auth/me") return json({ user });
    if (path === "/account") return json(account);
    if (path === "/api-keys") return json({ keys });
    if (path === "/account/ledger") return json({ entries });
    if (path === "/auth/logout") return Promise.resolve(new Response(null, { status: 204 }));
    return json({ message: "Fixture route not found" }, 404);
  };
})();`;

async function findChrome() {
  const { access } = await import("node:fs/promises");
  for (const candidate of chromeCandidates) {
    try {
      await access(candidate);
      return candidate;
    } catch {}
  }
  throw new Error("Chrome/Chromium was not found. Set CHROME_PATH to its executable.");
}

async function waitForJson(url, timeoutMs = 10_000) {
  const deadline = Date.now() + timeoutMs;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) return response.json();
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw lastError ?? new Error(`Timed out waiting for ${url}`);
}

function createCdpClient(webSocketUrl) {
  const socket = new WebSocket(webSocketUrl);
  const pending = new Map();
  const events = new Map();
  let sequence = 0;

  socket.addEventListener("message", (event) => {
    const message = JSON.parse(event.data);
    if (message.id) {
      const request = pending.get(message.id);
      if (!request) return;
      pending.delete(message.id);
      clearTimeout(request.timeout);
      if (message.error) request.reject(new Error(message.error.message));
      else request.resolve(message.result);
      return;
    }
    const listeners = events.get(message.method) ?? [];
    listeners.forEach((listener) => listener(message.params));
  });

  const ready = new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve, { once: true });
    socket.addEventListener("error", reject, { once: true });
  });

  return {
    ready,
    send(method, params = {}) {
      const id = ++sequence;
      return new Promise((resolve, reject) => {
        const timeout = setTimeout(() => {
          pending.delete(id);
          reject(new Error(`CDP command timed out: ${method}`));
        }, 30_000);
        pending.set(id, { resolve, reject, timeout });
        socket.send(JSON.stringify({ id, method, params }));
      });
    },
    once(method) {
      return new Promise((resolve) => {
        const listener = (params) => {
          events.set(method, (events.get(method) ?? []).filter((entry) => entry !== listener));
          resolve(params);
        };
        events.set(method, [...(events.get(method) ?? []), listener]);
      });
    },
    close() { socket.close(); },
  };
}

async function capturePage(client, [name, route, width, height, theme, language = "en"]) {
  await client.send("Emulation.setDeviceMetricsOverride", {
    width,
    height,
    deviceScaleFactor: 1,
    mobile: width < 600,
    screenWidth: width,
    screenHeight: height,
  });
  await client.send("Runtime.evaluate", {
    expression: `localStorage.setItem('theme', ${JSON.stringify(theme)}); localStorage.setItem('lang', ${JSON.stringify(language)});`,
  });
  const loaded = client.once("Page.loadEventFired");
  await client.send("Page.navigate", { url: new URL(route, baseUrl).href });
  await loaded;
  await client.send("Runtime.evaluate", {
    awaitPromise: true,
    expression: `(async () => {
      document.documentElement.dataset.theme = ${JSON.stringify(theme)};
      await new Promise((resolve) => setTimeout(resolve, 700));
      document.querySelector('.hero')?.classList.add('loaded');
      document.querySelectorAll('[data-reveal], [data-reveal-stagger], .reveal')
        .forEach((element) => element.classList.add('in'));
      document.querySelectorAll('nextjs-portal').forEach((element) => element.remove());
      document.documentElement.style.scrollBehavior = 'auto';
      await document.fonts.ready;
      await new Promise((resolve) => setTimeout(resolve, 850));
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    })()`,
  });
  const { cssContentSize, contentSize } = await client.send("Page.getLayoutMetrics");
  // Chrome reports the legacy contentSize in physical pixels on Retina displays.
  // cssContentSize keeps the clip in CSS pixels and avoids a half-empty 2x canvas.
  const measuredSize = cssContentSize ?? contentSize;
  const pageHeight = Math.ceil(measuredSize.height);
  const pageWidth = Math.ceil(measuredSize.width);
  const screenshot = await client.send("Page.captureScreenshot", {
    format: "png",
    fromSurface: true,
    captureBeyondViewport: true,
    clip: { x: 0, y: 0, width: pageWidth, height: pageHeight, scale: 1 },
  });
  const filename = `${name}.png`;
  await writeFile(path.join(outputDirectory, filename), Buffer.from(screenshot.data, "base64"));
  return { name, route, theme, language, width: pageWidth, height: pageHeight, file: filename };
}

const chrome = await findChrome();
const port = 9222 + Math.floor(Math.random() * 500);
await mkdir(outputDirectory, { recursive: true });

const browser = spawn(chrome, [
  "--headless=new",
  "--disable-gpu",
  "--hide-scrollbars",
  "--no-first-run",
  "--no-default-browser-check",
  `--remote-debugging-port=${port}`,
  `--user-data-dir=${path.join(outputDirectory, ".chrome-profile")}`,
  "about:blank",
], { stdio: "ignore" });

try {
  await waitForJson(`http://127.0.0.1:${port}/json/version`);
  const targetResponse = await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" });
  const target = await targetResponse.json();
  const client = createCdpClient(target.webSocketDebuggerUrl);
  await client.ready;
  await client.send("Page.enable");
  await client.send("Runtime.enable");
  await client.send("Page.addScriptToEvaluateOnNewDocument", { source: dashboardFixtureScript });
  const warmupLoaded = client.once("Page.loadEventFired");
  await client.send("Page.navigate", { url: baseUrl });
  await warmupLoaded;

  const manifest = [];
  for (const capture of captures) {
    manifest.push(await capturePage(client, capture));
    process.stdout.write(`Captured ${capture[0]}\n`);
  }
  await writeFile(path.join(outputDirectory, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  client.close();
  process.stdout.write(`Screenshots: ${outputDirectory}\n`);
} finally {
  browser.kill("SIGTERM");
}
