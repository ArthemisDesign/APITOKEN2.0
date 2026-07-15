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
  ["plans-russian", "/plans", 1440, 1000, "light", "ru"],
  ["models-desktop", "/models", 1440, 1000, "light"],
  ["models-dark", "/models", 1440, 1000, "dark"],
  ["docs-desktop", "/docs", 1440, 1000, "light"],
  ["docs-dark", "/docs", 1440, 1000, "dark"],
  ["docs-mobile", "/docs", 390, 844, "light"],
  ["docs-mobile-dark", "/docs", 390, 844, "dark"],
  ["integrations-desktop", "/integrations", 1440, 1000, "light"],
  ["integration-guide-desktop", "/int-claude-code", 1440, 1000, "light"],
  ["login-desktop", "/login", 1440, 1000, "light"],
  ["register-desktop", "/register", 1440, 1000, "light"],
  ["register-dark", "/register", 1440, 1000, "dark"],
  ["terms-desktop", "/terms", 1440, 1000, "light"],
  ["terms-dark", "/terms", 1440, 1000, "dark"],
  ["terms-russian", "/terms", 1440, 1000, "light", "ru"],
  ["privacy-desktop", "/privacy", 1440, 1000, "light"],
  ["privacy-dark", "/privacy", 1440, 1000, "dark"],
  ["privacy-russian", "/privacy", 1440, 1000, "light", "ru"],
  ["support-desktop", "/support", 1440, 1000, "light"],
  ["support-mobile", "/support", 390, 844, "light"],
  ["support-dark", "/support", 1440, 1000, "dark"],
  ["support-russian", "/support", 1440, 1000, "light", "ru"],
];

const dashboardCaptures = [
  ["dashboard-overview-light", "/dashboard", 1440, 1000, "light"],
  ["dashboard-overview-dark", "/dashboard", 1440, 1000, "dark"],
  ["dashboard-overview-russian", "/dashboard", 1440, 1000, "dark", "ru"],
  ["dashboard-keys-light", "/dashboard?view=keys", 1440, 1000, "light"],
  ["dashboard-keys-dark", "/dashboard?view=keys", 1440, 1000, "dark"],
  ["dashboard-topup-light", "/dashboard?view=credits", 1440, 1000, "light"],
  ["dashboard-topup-dark", "/dashboard?view=credits", 1440, 1000, "dark"],
  ["dashboard-topup-tablet-light", "/dashboard?view=credits", 768, 1024, "light"],
  ["dashboard-topup-mobile-light", "/dashboard?view=credits", 390, 844, "light"],
  ["dashboard-topup-mobile-dark", "/dashboard?view=credits", 390, 844, "dark"],
  ["dashboard-topup-mobile-russian", "/dashboard?view=credits", 390, 844, "light", "ru"],
  ["dashboard-usage-light", "/dashboard?view=usage", 1440, 1000, "light"],
  ["dashboard-usage-dark", "/dashboard?view=usage", 1440, 1000, "dark"],
  ["dashboard-promos-light", "/dashboard?view=promos", 1440, 1000, "light"],
  ["dashboard-promos-dark", "/dashboard?view=promos", 1440, 1000, "dark"],
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
const shouldVerifyCredits = process.env.AUDIT_VERIFY_CREDITS === "1" ||
  (process.env.AUDIT_VERIFY_CREDITS !== "0" && captures.some(([name]) => name.startsWith("dashboard-topup-")));
const shouldVerifyDocsTheme = process.env.AUDIT_VERIFY_DOCS_THEME === "1" ||
  (process.env.AUDIT_VERIFY_DOCS_THEME !== "0" && captures.some(([name]) => name.startsWith("docs-")));

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
    displayName: "Dashboard Audit",
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
        spendThresholdNano: "100000000000",
        remainingNano: "88000000000",
        visibleOfficialUsageUsd: "286.00",
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
  const nowS = Math.floor(Date.now() / 1000), DAY = 86400;
  // реальный формат движка: amountUsd со знаком "$" и 6 знаками (раньше ломал график через Number())
  const chg = [
    [0, "1246000000", "claude-opus-4-8"], [0, "742000000", "claude-sonnet-5"], [0, "180000000", "claude-haiku-4-5-20251001"],
    [1, "918000000", "claude-opus-4-8"], [1, "410000000", "claude-sonnet-5"],
    [2, "655000000", "claude-sonnet-5"], [2, "300000000", "claude-opus-4-8"],
    [3, "1330000000", "claude-opus-4-8"], [3, "520000000", "claude-sonnet-5"], [3, "90000000", "claude-haiku-4-5-20251001"],
    [4, "540000000", "claude-sonnet-5"],
    [6, "805000000", "claude-opus-4-8"], [6, "260000000", "claude-haiku-4-5-20251001"],
    [8, "1050000000", "claude-opus-4-8"], [8, "300000000", "claude-sonnet-5"],
  ];
  const entries = chg.map((c, i) => ({ id: "c" + i, kind: "charge", amountNano: c[1], amountUsd: "$" + (Number(c[1]) / 1e9).toFixed(6), keyMasked: "sk-pool-a5b5••••••••eeb", reference: "req_0" + i, model: c[2], balanceAfterNano: null, timestamp: String(nowS - c[0] * DAY - i * 137) }));
  entries.push({ id: "t0", kind: "topup", amountNano: "12000000000", amountUsd: "$12.000000", discountPercent: 60, keyMasked: null, reference: "cryptomus_9f2c1a", balanceAfterNano: null, timestamp: String(nowS - 3 * DAY) });
  const usage = {
    window: "30d", requests: 59, totalOfficialNano: "20234893050", totalChargedNano: "8093957220",
    buckets: {
      input: { tokens: 3781269, officialNano: "15124021000" },
      output: { tokens: 15168, officialNano: "228560000" },
      cacheRead: { tokens: 4866858, officialNano: "1840525800" },
      cacheWrite: { tokens: 741129, officialNano: "3041786250" },
      webSearch: { requests: 0, officialNano: "0" },
    },
    models: [
      { model: "claude-opus-4-8", requests: 27, inputTokens: 1890211, outputTokens: 5100, cacheReadTokens: 2256400, cacheWrite5mTokens: 282050, cacheWrite1hTokens: 0, webSearchRequests: 0, officialNano: "12469567500", chargedNano: "4987827000" },
      { model: "claude-sonnet-5", requests: 27, inputTokens: 1890954, outputTokens: 5072, cacheReadTokens: 2256400, cacheWrite5mTokens: 282050, cacheWrite1hTokens: 0, webSearchRequests: 0, officialNano: "7483549500", chargedNano: "2993419800" },
      { model: "claude-haiku-4-5-20251001", requests: 5, inputTokens: 104, outputTokens: 4996, cacheReadTokens: 354058, cacheWrite5mTokens: 177029, cacheWrite1hTokens: 0, webSearchRequests: 0, officialNano: "281776050", chargedNano: "112710420" },
    ],
  };
  window.fetch = (input, init = {}) => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    if (!url.startsWith(apiBase)) return originalFetch(input, init);
    const parsed = new URL(url);
    const path = parsed.pathname.slice("/v1".length);
    if (location.search.includes("audit-auth=1") && path === "/auth/me") return json({ user });
    if (!location.pathname.startsWith("/dashboard")) return originalFetch(input, init);
    if (path === "/auth/me") {
      if ((init.method || "GET").toUpperCase() === "PATCH") {
        user.displayName = JSON.parse(String(init.body || "{}")).displayName || user.displayName;
      }
      return json({ user });
    }
    if (path === "/account") return json(account);
    if (path === "/api-keys") return json({ keys });
    if (path === "/account/ledger") return json({ entries });
    if (path === "/account/usage") return json(usage);
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
  const captureUrl = new URL(route, baseUrl);
  // Force a real navigation even when consecutive captures use the same route.
  // Without this cache-buster Chrome can reuse the mounted English page after
  // localStorage is changed, producing a mislabeled language screenshot.
  captureUrl.searchParams.set("__audit", `${name}-${Date.now()}`);
  const loaded = client.once("Page.loadEventFired");
  await client.send("Page.navigate", { url: captureUrl.href });
  await loaded;
  await client.send("Runtime.evaluate", {
    awaitPromise: true,
    expression: `new Promise((resolve) => setTimeout(resolve, 500))`,
  });
  await client.send("Runtime.evaluate", {
    expression: `(() => {
      if (document.documentElement.lang === ${JSON.stringify(language)}) return;
      const label = ${JSON.stringify(language.toUpperCase())};
      const control = [...document.querySelectorAll('.lang button')]
        .find((button) => button.textContent?.trim() === label);
      control?.click();
    })()`,
  });
  try {
    await waitForCondition(
      client,
      `document.documentElement.lang === ${JSON.stringify(language)}`,
      `${name} language state`,
    );
  } catch (error) {
    const state = await client.send("Runtime.evaluate", {
      expression: `JSON.stringify({
        documentLanguage: document.documentElement.lang,
        storedLanguage: localStorage.getItem('lang'),
        controls: [...document.querySelectorAll('.lang button')].map((button) => ({
          label: button.textContent?.trim(),
          active: button.classList.contains('active'),
        })),
      })`,
      returnByValue: true,
    });
    throw new Error(`${error instanceof Error ? error.message : error} Browser state: ${state.result.value}`);
  }
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
      // A language hydration can replace translated reveal nodes after the
      // first pass. Stabilize the final DOM immediately before capture.
      document.querySelector('.hero')?.classList.add('loaded');
      document.querySelectorAll('[data-reveal], [data-reveal-stagger], .reveal')
        .forEach((element) => element.classList.add('in'));
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

async function waitForCondition(client, expression, description, timeoutMs = 8_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const result = await client.send("Runtime.evaluate", { expression, returnByValue: true });
    if (result.result.value === true) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`Timed out waiting for ${description}.`);
}

async function setViewport(client, width, height) {
  await client.send("Emulation.setDeviceMetricsOverride", {
    width,
    height,
    deviceScaleFactor: 1,
    mobile: width < 600,
    screenWidth: width,
    screenHeight: height,
  });
}

async function clickSelector(client, selector) {
  const result = await client.send("Runtime.evaluate", {
    expression: `(() => {
      const element = document.querySelector(${JSON.stringify(selector)});
      const rect = element?.getBoundingClientRect();
      return rect && { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    })()`,
    returnByValue: true,
  });
  const rect = result.result.value;
  if (!rect) throw new Error(`Browser audit control was not found: ${selector}`);
  await client.send("Page.bringToFront");
  const x = rect.x + rect.width / 2;
  const y = rect.y + rect.height / 2;
  await client.send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", clickCount: 1 });
  await client.send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", clickCount: 1 });
}

async function verifyCreditsLayout(client) {
  const cases = [
    { name: "desktop", width: 1440, height: 1000, statRows: 1, statusRows: 1, converterRow: true, mobileHistory: false },
    { name: "tablet", width: 768, height: 1024, statRows: 1, statusRows: 3, converterRow: true, mobileHistory: false },
    { name: "mobile", width: 390, height: 844, statRows: 3, statusRows: 3, converterRow: false, mobileHistory: true },
  ];

  for (const layoutCase of cases) {
    await setViewport(client, layoutCase.width, layoutCase.height);
    await client.send("Runtime.evaluate", {
      expression: `localStorage.setItem('theme', 'light'); localStorage.setItem('lang', 'en');`,
    });
    const url = new URL("/dashboard?view=credits", baseUrl);
    url.searchParams.set("__auditCredits", layoutCase.name);
    const loaded = client.once("Page.loadEventFired");
    await client.send("Page.navigate", { url: url.href });
    await loaded;
    await waitForCondition(
      client,
      `Boolean(document.querySelector('.credits-stack .topup-convert')) && document.querySelectorAll('.pricing-status-item').length === 3 && Boolean(document.querySelector('.topup-history-table tbody tr'))`,
      `${layoutCase.name} Credits layout`,
    );
    await client.send("Runtime.evaluate", {
      awaitPromise: true,
      expression: `new Promise((resolve) => setTimeout(resolve, 500))`,
    });

    const result = await client.send("Runtime.evaluate", {
      expression: `(() => {
        const rects = (selector) => [...document.querySelectorAll(selector)].map((element) => element.getBoundingClientRect());
        const rowCount = (items) => new Set(items.map((rect) => Math.round(rect.top))).size;
        const stats = rects('.credits-stack .tc-stats .ovstat');
        const statuses = rects('.pricing-milestone-status .pricing-status-item');
        const input = document.querySelector('.tc-input')?.getBoundingClientRect();
        const receive = document.querySelector('.tc-receive')?.getBoundingClientRect();
        const rail = ['.credits-stack .tc-stats', '.credits-stack .topup-convert', '.credits-stack .pricing-banner', '.credits-history']
          .map((selector) => document.querySelector(selector)?.getBoundingClientRect())
          .filter(Boolean);
        const history = document.querySelector('.credits-history .table-scroll');
        const historyTable = document.querySelector('.topup-history-table');
        const historyCells = [...document.querySelectorAll('.topup-history-table td:not(.empty-cell)')];
        return JSON.stringify({
          overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
          statRows: rowCount(stats),
          statusRows: rowCount(statuses),
          converterRow: Boolean(input && receive && Math.abs(input.top - receive.top) < 2),
          aligned: rail.length === 4 && Math.max(...rail.map((rect) => rect.left)) - Math.min(...rail.map((rect) => rect.left)) < 2 && Math.max(...rail.map((rect) => rect.right)) - Math.min(...rail.map((rect) => rect.right)) < 2,
          historyFits: Boolean(history && history.scrollWidth <= history.clientWidth + 1),
          mobileHistory: Boolean(historyTable && historyCells.length === 5 && getComputedStyle(historyTable).display === 'block' && historyCells.every((cell) => cell.dataset.label && !['none', '""'].includes(getComputedStyle(cell, '::before').content))),
          receiveText: document.querySelector('.tc-recv-value')?.textContent?.trim(),
        });
      })()`,
      returnByValue: true,
    });
    const state = JSON.parse(result.result.value);
    if (state.overflow > 1 || state.statRows !== layoutCase.statRows || state.statusRows !== layoutCase.statusRows || state.converterRow !== layoutCase.converterRow) {
      throw new Error(`Credits ${layoutCase.name} responsive layout failed: ${JSON.stringify(state)}`);
    }
    if (layoutCase.name === "desktop" && !state.aligned) {
      throw new Error(`Credits desktop rail is not aligned: ${JSON.stringify(state)}`);
    }
    if (!state.historyFits || state.mobileHistory !== layoutCase.mobileHistory) {
      throw new Error(`Credits ${layoutCase.name} history layout failed: ${JSON.stringify(state)}`);
    }

    if (layoutCase.name === "desktop") {
      await clickSelector(client, '[data-topup-preset="500"]');
      await waitForCondition(
        client,
        `document.querySelector('.tc-field input')?.value === '500' && document.querySelector('.tc-preset.on b')?.textContent?.trim() === '$500'`,
        "the Credits preset to update the converter",
      );
      const updated = await client.send("Runtime.evaluate", {
        expression: `document.querySelector('.tc-recv-value')?.textContent?.trim()`,
        returnByValue: true,
      });
      if (!updated.result.value || updated.result.value === state.receiveText) {
        throw new Error(`The Credits receive value did not update: ${state.receiveText} -> ${updated.result.value}`);
      }
    }
  }
  process.stdout.write("Verified Credits alignment, responsive stacking, history layout, and preset interaction\n");
}

async function verifyDocsTheme(client) {
  await setViewport(client, 1440, 1000);
  await client.send("Emulation.setEmulatedMedia", {
    media: "screen",
    features: [{ name: "prefers-reduced-motion", value: "no-preference" }],
  });
  await client.send("Runtime.evaluate", {
    expression: `localStorage.setItem('theme', 'light'); localStorage.setItem('lang', 'en');`,
  });
  const url = new URL("/docs", baseUrl);
  url.searchParams.set("__auditDocsTheme", "1");
  const loaded = client.once("Page.loadEventFired");
  await client.send("Page.navigate", { url: url.href });
  await loaded;
  await waitForCondition(
    client,
    `document.querySelector('.theme-tgl')?.getAttribute('aria-label') === 'Switch to dark theme' && !document.documentElement.hasAttribute('data-theme')`,
    "the light docs theme",
  );

  const lightResult = await client.send("Runtime.evaluate", {
    expression: `(() => {
      const style = (selector) => getComputedStyle(document.querySelector(selector));
      return JSON.stringify({
        site: style('.docs-site').backgroundColor,
        header: style('.docs-header').backgroundColor,
        code: style('.docs-code-card pre').backgroundColor,
        durations: ['.docs-site', '.docs-header', '.docs-sidebar', '.docs-endpoint', '.docs-notice', '.docs-code-card pre', '.docs-auth-flow', '.docs-checklist', '.docs-footer'].map((selector) => style(selector).transitionDuration),
      });
    })()`,
    returnByValue: true,
  });
  const light = JSON.parse(lightResult.result.value);
  if (light.durations.some((duration) => Number.parseFloat(duration) < 0.3)) {
    throw new Error(`Docs theme surfaces do not use the shared transition timing: ${JSON.stringify(light)}`);
  }

  await clickSelector(client, ".theme-tgl");
  await waitForCondition(
    client,
    `document.documentElement.dataset.theme === 'dark' && localStorage.getItem('theme') === 'dark' && document.querySelector('.theme-tgl')?.getAttribute('aria-label') === 'Switch to light theme'`,
    "the dark docs theme after a real toggle click",
  );
  await client.send("Runtime.evaluate", { awaitPromise: true, expression: `new Promise((resolve) => setTimeout(resolve, 450))` });
  const darkResult = await client.send("Runtime.evaluate", {
    expression: `(() => {
      const style = (selector) => getComputedStyle(document.querySelector(selector));
      return JSON.stringify({
        site: style('.docs-site').backgroundColor,
        header: style('.docs-header').backgroundColor,
        code: style('.docs-code-card pre').backgroundColor,
      });
    })()`,
    returnByValue: true,
  });
  const dark = JSON.parse(darkResult.result.value);
  if (dark.site === light.site || dark.header === light.header || dark.code === light.code) {
    throw new Error(`Docs theme-sensitive surfaces did not change together: ${JSON.stringify({ light, dark })}`);
  }

  await clickSelector(client, ".theme-tgl");
  await waitForCondition(
    client,
    `!document.documentElement.hasAttribute('data-theme') && localStorage.getItem('theme') === 'light' && document.querySelector('.theme-tgl')?.getAttribute('aria-label') === 'Switch to dark theme'`,
    "the light docs theme after switching back",
  );
  await clickSelector(client, ".theme-tgl");
  await waitForCondition(client, `document.documentElement.dataset.theme === 'dark' && localStorage.getItem('theme') === 'dark'`, "the persisted dark docs theme");

  const reloaded = client.once("Page.loadEventFired");
  await client.send("Page.reload");
  await reloaded;
  await waitForCondition(
    client,
    `document.documentElement.dataset.theme === 'dark' && localStorage.getItem('theme') === 'dark' && document.querySelector('.theme-tgl')?.getAttribute('aria-label') === 'Switch to light theme'`,
    "the dark docs theme after reload",
  );

  await client.send("Emulation.setEmulatedMedia", {
    media: "screen",
    features: [{ name: "prefers-reduced-motion", value: "reduce" }],
  });
  const reducedResult = await client.send("Runtime.evaluate", {
    expression: `JSON.stringify(['.docs-site', '.docs-header', '.docs-sidebar', '.docs-endpoint', '.docs-notice', '.docs-code-card pre', '.docs-auth-flow', '.docs-checklist', '.docs-footer'].map((selector) => getComputedStyle(document.querySelector(selector)).transitionDuration))`,
    returnByValue: true,
  });
  const reducedDurations = JSON.parse(reducedResult.result.value);
  if (reducedDurations.some((duration) => Number.parseFloat(duration) > 0.002)) {
    throw new Error(`Reduced-motion docs transitions are too long: ${JSON.stringify(reducedDurations)}`);
  }
  await clickSelector(client, ".theme-tgl");
  await waitForCondition(
    client,
    `!document.documentElement.hasAttribute('data-theme') && localStorage.getItem('theme') === 'light' && document.querySelector('.theme-tgl')?.getAttribute('aria-label') === 'Switch to dark theme'`,
    "the reduced-motion docs theme toggle",
  );
  await client.send("Emulation.setEmulatedMedia", {
    media: "screen",
    features: [{ name: "prefers-reduced-motion", value: "no-preference" }],
  });
  process.stdout.write("Verified real docs theme toggles, persistence, shared transitions, and reduced motion\n");
}

async function verifyDashboardRouting(client) {
  await client.send("Runtime.evaluate", { expression: `localStorage.setItem('lang', 'en');` });
  for (const removedView of ["refer", "orders"]) {
    const removedLoaded = client.once("Page.loadEventFired");
    await client.send("Page.navigate", { url: new URL(`/dashboard?view=${removedView}`, baseUrl).href });
    await removedLoaded;
    await waitForCondition(
      client,
      `document.querySelector('[data-dashboard-section="overview"]')?.getAttribute('aria-current') === 'page' && Boolean(document.querySelector('.overview-core'))`,
      `the removed ${removedView} route to fall back to Overview`,
    );
  }
  const loaded = client.once("Page.loadEventFired");
  await client.send("Page.navigate", { url: new URL("/dashboard?view=credits", baseUrl).href });
  await loaded;
  await waitForCondition(
    client,
    `Boolean(document.querySelector('[data-dashboard-section="overview"]')) && document.querySelector('.p-h1')?.textContent?.trim() === 'Top up balance'`,
    "the reloaded top-up view",
  );
  await client.send("Runtime.evaluate", {
    expression: `document.querySelector('[data-dashboard-section="overview"]')?.click()`,
  });
  try {
    await waitForCondition(
      client,
      `location.pathname === '/dashboard' && location.search === '' && document.querySelector('.app-title')?.textContent?.trim() === 'Overview' && Boolean(document.querySelector('.overview-core'))`,
      "direct navigation from a reloaded subview to Overview",
    );
  } catch (error) {
    const state = await client.send("Runtime.evaluate", {
      expression: `JSON.stringify({ href: location.href, heading: document.querySelector('.app-title')?.textContent?.trim(), active: document.querySelector('[data-dashboard-section][aria-current="page"]')?.dataset.dashboardSection })`,
      returnByValue: true,
    });
    throw new Error(`${error instanceof Error ? error.message : error} Browser state: ${state.result.value}`);
  }
  await client.send("Runtime.evaluate", {
    expression: `document.querySelector('[data-dashboard-section="keys"]')?.click()`,
  });
  await waitForCondition(
    client,
    `location.search === '?view=keys' && document.querySelector('.p-h1')?.textContent?.trim() === 'API keys'`,
    "dashboard navigation to API keys",
  );
  await client.send("Runtime.evaluate", { expression: "history.back()" });
  await waitForCondition(
    client,
    `location.pathname === '/dashboard' && location.search === '' && document.querySelector('.app-title')?.textContent?.trim() === 'Overview' && Boolean(document.querySelector('.overview-core'))`,
    "Back navigation to Overview",
  );
  await client.send("Runtime.evaluate", { expression: "history.forward()" });
  await waitForCondition(
    client,
    `location.search === '?view=keys' && document.querySelector('.p-h1')?.textContent?.trim() === 'API keys'`,
    "Forward navigation to API keys",
  );
  process.stdout.write("Verified removed-view fallbacks, reload, direct Overview, and Back/Forward dashboard routing\n");
}

async function verifyProfileBehavior(client) {
  await client.send("Runtime.evaluate", { expression: `localStorage.setItem('lang', 'en');` });
  const loaded = client.once("Page.loadEventFired");
  await client.send("Page.navigate", { url: new URL("/dashboard?view=profile", baseUrl).href });
  await loaded;
  await waitForCondition(
    client,
    `Boolean(document.querySelector('#profile-display-name')) && Boolean(document.querySelector('.uid-copy-button'))`,
    "the editable profile form",
  );
  await client.send("Browser.grantPermissions", {
    origin: new URL(baseUrl).origin,
    permissions: ["clipboardReadWrite"],
  });
  const before = await client.send("Runtime.evaluate", {
    expression: `(() => {
      const input = document.querySelector('.profile-id-row .set-in');
      const rect = input?.getBoundingClientRect();
      const style = input ? getComputedStyle(input) : null;
      return JSON.stringify({
        value: input?.value,
        disabled: input?.disabled,
        readOnly: input?.readOnly,
        className: input?.className,
        rect: rect && { width: rect.width, height: rect.height },
        border: style?.border,
        background: style?.backgroundColor,
      });
    })()`,
    returnByValue: true,
  });
  const copyRect = await client.send("Runtime.evaluate", {
    expression: `(() => {
      const rect = document.querySelector('.uid-copy-button')?.getBoundingClientRect();
      return rect && { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    })()`,
    returnByValue: true,
  });
  await client.send("Page.bringToFront");
  const copyX = copyRect.result.value.x + copyRect.result.value.width / 2;
  const copyY = copyRect.result.value.y + copyRect.result.value.height / 2;
  await client.send("Input.dispatchMouseEvent", { type: "mousePressed", x: copyX, y: copyY, button: "left", clickCount: 1 });
  await client.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: copyX, y: copyY, button: "left", clickCount: 1 });
  await waitForCondition(
    client,
    `document.querySelector('.uid-copy-button')?.textContent?.trim() === 'Copied'`,
    "independent user-ID copy feedback",
  );
  const after = await client.send("Runtime.evaluate", {
    expression: `(() => {
      const input = document.querySelector('.profile-id-row .set-in');
      const rect = input?.getBoundingClientRect();
      const style = input ? getComputedStyle(input) : null;
      return JSON.stringify({
        value: input?.value,
        disabled: input?.disabled,
        readOnly: input?.readOnly,
        className: input?.className,
        rect: rect && { width: rect.width, height: rect.height },
        border: style?.border,
        background: style?.backgroundColor,
      });
    })()`,
    returnByValue: true,
  });
  if (before.result.value !== after.result.value) {
    throw new Error(`Copy feedback changed the user-ID field: ${before.result.value} -> ${after.result.value}`);
  }
  const state = JSON.parse(before.result.value);
  if (!state.disabled || !state.readOnly || !state.value) {
    throw new Error(`User ID is not immutable: ${before.result.value}`);
  }
  await client.send("Runtime.evaluate", {
    expression: `(() => {
      const input = document.querySelector('#profile-display-name');
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
      setter?.call(input, 'Updated Dashboard');
      input?.dispatchEvent(new Event('input', { bubbles: true }));
      document.querySelector('.prof-save button')?.click();
    })()`,
  });
  await waitForCondition(
    client,
    `document.querySelector('.side-uinfo b')?.textContent?.trim() === 'Updated Dashboard' && Boolean(document.querySelector('.profile-save-success'))`,
    "the saved display name to update the authenticated profile shell",
  );
  process.stdout.write("Verified editable display name and immutable, independently copied user ID\n");
}

async function verifyPersistentSiteRouting(client) {
  await client.send("Runtime.evaluate", {
    expression: `localStorage.setItem('lang', 'en'); localStorage.setItem('theme', 'dark');`,
  });
  const loaded = client.once("Page.loadEventFired");
  await client.send("Page.navigate", { url: new URL("/", baseUrl).href });
  await loaded;
  await waitForCondition(
    client,
    `document.documentElement.lang === 'en' && document.documentElement.dataset.theme === 'dark' && Boolean(document.querySelector('header.nav')) && Boolean(document.querySelector('footer')) && Boolean(document.querySelector('.bg-decor'))`,
    "the public site shell",
  );
  await client.send("Runtime.evaluate", {
    awaitPromise: true,
    expression: `new Promise((resolve) => setTimeout(resolve, 800))`,
  });
  await client.send("Runtime.evaluate", {
    expression: `window.__siteAuditSentinel = 'persistent-site-shell';
      window.__siteAuditHeader = document.querySelector('header.nav');
      window.__siteAuditFooter = document.querySelector('footer');
      window.__siteAuditBackground = document.querySelector('.bg-decor');
      window.__siteAuditAuthChecks = 0;
      window.__siteAuditOriginalFetch = window.fetch;
      window.fetch = (...args) => {
        const input = args[0];
        const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
        if (url.includes('/auth/me')) window.__siteAuditAuthChecks += 1;
        return window.__siteAuditOriginalFetch(...args);
      };`,
  });

  const transitions = [
    [`.prod a[href="/models"]`, "/models"],
    [`header.nav a[href="/integrations"]`, "/integrations"],
    [`.steps a[href="/int-claude-code"]`, "/int-claude-code"],
    [`.auth-back[href="/integrations"]`, "/integrations"],
    [`header.nav a[href="/models"]`, "/models"],
    [`footer a[href="/privacy"]`, "/privacy"],
    [`.compliance-nav a[href="/terms"]`, "/terms"],
    [`.compliance-nav a[href="/support"]`, "/support"],
    [`.compliance-nav a[href="/plans"]`, "/plans"],
    [`header.nav a.brand[href="/"]`, "/"],
  ];

  for (const [selector, pathname] of transitions) {
    const clicked = await client.send("Runtime.evaluate", {
      expression: `(() => { const link = document.querySelector(${JSON.stringify(selector)}); link?.click(); return Boolean(link); })()`,
      returnByValue: true,
    });
    if (!clicked.result.value) throw new Error(`Navigation audit link was not found: ${selector}`);
    await waitForCondition(client, `location.pathname === ${JSON.stringify(pathname)}`, `client navigation to ${pathname}`);
    const result = await client.send("Runtime.evaluate", {
      expression: `JSON.stringify({
        sentinel: window.__siteAuditSentinel === 'persistent-site-shell',
        sameHeader: window.__siteAuditHeader === document.querySelector('header.nav'),
        sameFooter: window.__siteAuditFooter === document.querySelector('footer'),
        sameBackground: window.__siteAuditBackground === document.querySelector('.bg-decor'),
        authChecks: window.__siteAuditAuthChecks,
        language: document.documentElement.lang,
        theme: document.documentElement.dataset.theme,
      })`,
      returnByValue: true,
    });
    const state = JSON.parse(result.result.value);
    if (!state.sentinel || !state.sameHeader || !state.sameFooter || !state.sameBackground || state.authChecks !== 0 || state.language !== "en" || state.theme !== "dark") {
      throw new Error(`Public shell changed while navigating to ${pathname}: ${JSON.stringify(state)}`);
    }
  }
  process.stdout.write("Verified persistent public shell across landing, marketing, integration, and compliance routes\n");
}

async function verifyComplianceRouting(client) {
  await client.send("Runtime.evaluate", {
    expression: `localStorage.setItem('lang', 'ru'); localStorage.setItem('theme', 'dark');`,
  });
  const loaded = client.once("Page.loadEventFired");
  await client.send("Page.navigate", { url: new URL("/privacy", baseUrl).href });
  await loaded;
  await waitForCondition(
    client,
    `document.documentElement.lang === 'ru' && document.documentElement.dataset.theme === 'dark' && document.querySelector('h1')?.textContent?.trim() === 'Политика конфиденциальности'`,
    "the Russian dark-mode compliance state",
  );
  await client.send("Runtime.evaluate", {
    awaitPromise: true,
    expression: `new Promise((resolve) => setTimeout(resolve, 800))`,
  });
  await client.send("Runtime.evaluate", {
    expression: `window.__complianceHeader = document.querySelector('header.nav');
      window.__complianceAuthText = document.querySelector('.nav-actions')?.textContent;
      window.__complianceAuthChecks = 0;
      window.__complianceOriginalFetch = window.fetch;
      window.fetch = (...args) => {
        const input = args[0];
        const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
        if (url.includes('/auth/me')) window.__complianceAuthChecks += 1;
        return window.__complianceOriginalFetch(...args);
      };
      document.querySelector('.compliance-nav a[href="/terms"]')?.click();`,
  });
  await waitForCondition(
    client,
    `location.pathname === '/terms' && document.querySelector('h1')?.textContent?.trim() === 'Пользовательское соглашение'`,
    "client navigation to the User Agreement",
  );
  await client.send("Runtime.evaluate", { expression: `document.querySelector('.compliance-nav a[href="/support"]')?.click()` });
  await waitForCondition(
    client,
    `location.pathname === '/support' && document.querySelector('h1')?.textContent?.trim() === 'Связаться с apiToken.sale'`,
    "client navigation to Support",
  );
  await client.send("Runtime.evaluate", { expression: `document.querySelector('.compliance-nav a[href="/plans"]')?.click()` });
  await waitForCondition(
    client,
    `location.pathname === '/plans' && document.querySelector('h1')?.textContent?.trim() === 'Тарифы и цены'`,
    "client navigation to Pricing",
  );
  const result = await client.send("Runtime.evaluate", {
    expression: `JSON.stringify({
      sameHeader: window.__complianceHeader === document.querySelector('header.nav'),
      authChecks: window.__complianceAuthChecks,
      sameAuthText: window.__complianceAuthText === document.querySelector('.nav-actions')?.textContent,
      language: document.documentElement.lang,
      storedLanguage: localStorage.getItem('lang'),
      theme: document.documentElement.dataset.theme,
      storedTheme: localStorage.getItem('theme'),
    })`,
    returnByValue: true,
  });
  const state = JSON.parse(result.result.value);
  if (!state.sameHeader || state.authChecks !== 0 || !state.sameAuthText || state.language !== "ru" || state.storedLanguage !== "ru" || state.theme !== "dark" || state.storedTheme !== "dark") {
    throw new Error(`Compliance shell was not preserved: ${JSON.stringify(state)}`);
  }
  process.stdout.write("Verified persistent compliance shell, language, theme, and authentication menu state\n");
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
  if (process.env.AUDIT_VERIFY_ROUTING === "1") await verifyDashboardRouting(client);
  if (process.env.AUDIT_VERIFY_PROFILE === "1") await verifyProfileBehavior(client);
  if (shouldVerifyCredits) await verifyCreditsLayout(client);
  if (shouldVerifyDocsTheme) await verifyDocsTheme(client);
  if (process.env.AUDIT_VERIFY_SITE_ROUTING === "1") await verifyPersistentSiteRouting(client);
  if (process.env.AUDIT_VERIFY_COMPLIANCE === "1") await verifyComplianceRouting(client);
  await writeFile(path.join(outputDirectory, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  client.close();
  process.stdout.write(`Screenshots: ${outputDirectory}\n`);
} finally {
  browser.kill("SIGTERM");
}
