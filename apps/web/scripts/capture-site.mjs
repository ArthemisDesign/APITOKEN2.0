import { spawn } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";

const baseUrl = process.env.SITE_URL ?? "http://localhost:3001";
const outputDirectory = path.resolve(process.env.SCREENSHOT_DIR ?? ".artifacts/site-audit");
const chromeCandidates = [
  process.env.CHROME_PATH,
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/Applications/Chromium.app/Contents/MacOS/Chromium",
  "/usr/bin/google-chrome",
  "/usr/bin/chromium",
].filter(Boolean);

const captures = [
  ["home-desktop", "/", 1440, 1000, "light"],
  ["home-mobile", "/", 390, 844, "light"],
  ["home-dark", "/", 1440, 1000, "dark"],
  ["plans-desktop", "/plans", 1440, 1000, "light"],
  ["plans-mobile", "/plans", 390, 844, "light"],
  ["plans-dark", "/plans", 1440, 1000, "dark"],
  ["models-desktop", "/models", 1440, 1000, "light"],
  ["models-dark", "/models", 1440, 1000, "dark"],
  ["docs-desktop", "/docs", 1440, 1000, "light"],
  ["docs-dark", "/docs", 1440, 1000, "dark"],
  ["integrations-desktop", "/integrations", 1440, 1000, "light"],
  ["integration-guide-desktop", "/int-claude-code", 1440, 1000, "light"],
  ["login-desktop", "/login", 1440, 1000, "light"],
  ["register-desktop", "/register", 1440, 1000, "light"],
  ["register-dark", "/register", 1440, 1000, "dark"],
  ["terms-desktop", "/terms", 1440, 1000, "light"],
  ["privacy-desktop", "/privacy", 1440, 1000, "light"],
];

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
        pending.set(id, { resolve, reject });
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

async function capturePage(client, [name, route, width, height, theme]) {
  await client.send("Emulation.setDeviceMetricsOverride", {
    width,
    height,
    deviceScaleFactor: 1,
    mobile: width < 600,
    screenWidth: width,
    screenHeight: height,
  });
  const loaded = client.once("Page.loadEventFired");
  await client.send("Page.navigate", { url: new URL(route, baseUrl).href });
  await loaded;
  await client.send("Runtime.evaluate", {
    awaitPromise: true,
    expression: `(async () => {
      document.documentElement.dataset.theme = ${JSON.stringify(theme)};
      localStorage.setItem('theme', ${JSON.stringify(theme)});
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
  return { name, route, theme, width: pageWidth, height: pageHeight, file: filename };
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
