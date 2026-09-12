// Run against a local build with NEXT_PUBLIC_PREVIEW_FIXTURES=1.
import { createRequire } from 'node:module';
import assert from 'node:assert/strict';
import fs from 'node:fs';

const load = createRequire(import.meta.url);
const engine = process.env.BROWSER || 'chromium';
const browserType = load(process.env.PLAYWRIGHT_MODULE || 'playwright-core')[engine];
const origin = process.env.SITE_URL || 'http://127.0.0.1:3052';
assert.ok(['localhost', '127.0.0.1'].includes(new URL(origin).hostname), 'Local fixtures only');
const output = process.env.AUDIT_OUTPUT || '.artifacts/chart-cards';
fs.mkdirSync(output, { recursive: true });
const browser = await browserType.launch({ headless: true, ...(engine === 'chromium' && process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {}) });
try {
  for (const width of [320, 390, 768, 1440]) for (const theme of ['light', 'dark']) for (const lang of ['en', 'ru']) {
    const page = await browser.newPage({ viewport: { width, height: 1000 }, reducedMotion: 'reduce' });
    await page.route(/mc\.yandex|google-analytics|vitals\.vercel|va\.vercel/, route => route.abort());
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.addInitScript(({ theme, lang }) => {
      if (window.top !== window) return;
      localStorage.setItem('theme:v1', theme);
      localStorage.setItem('lang:v1', lang);
    }, { theme, lang });
    for (const view of ['usage', 'referral']) {
      const label = `${view} ${width}px ${theme} ${lang}`;
      await page.goto(`${origin}${lang === 'ru' ? '/ru' : ''}/dashboard?view=${view}`);
      const card = page.locator(view === 'usage' ? '.usage-analytics-card .uchart' : '.referral-earnings-graph .uchart');
      await card.waitFor();
      await page.evaluate(() => document.fonts.ready);
      const surfaces = page.locator(view === 'usage' ? '.usage-kpis .ovstat:first-child, .usage-models-section .tokb:first-child, .usage-analytics-card .uchart' : '.rp-stat:first-child, .referral-earnings-graph .uchart');
      assert.ok(await surfaces.evaluateAll(els => els.every(el => getComputedStyle(el).backgroundColor !== 'rgb(255, 89, 73)')), `${label}: neutral cards`);
      assert.equal(await card.evaluate(el => getComputedStyle(el, '::after').content), 'none', `${label}: no decorative circles`);
      const columns = card.locator('.uchart-col').filter({ has: page.locator('.uchart-seg') });
      assert.ok(await columns.count() > 0, `${label}: populated chart`);
      const checkBounds = async () => {
        const tooltip = card.getByRole('tooltip');
        await tooltip.waitFor();
        assert.equal(await tooltip.evaluate(el => {
          const tip = el.getBoundingClientRect(), card = el.closest('.uchart').getBoundingClientRect();
          return tip.top >= card.top && tip.bottom <= card.bottom && tip.left >= card.left && tip.right <= card.right;
        }), true, `${label}: tooltip inside card`);
      };
      // Every populated column includes the peak day and both populated edges.
      for (const column of await columns.all()) {
        await column.focus();
        await checkBounds();
      }
      await page.setViewportSize({ width: width + 30, height: 1000 });
      await page.waitForTimeout(100);
      await checkBounds();
      await page.setViewportSize({ width, height: 1000 });
      await columns.first().press('Escape');
      await card.getByRole('tooltip').waitFor({ state: 'hidden' });
      await columns.last().hover();
      await checkBounds();
      if ((width === 1440 || width === 390) && lang === 'en') await card.screenshot({ path: `${output}/${view}-${width}-${theme}.png` });
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true, `${label}: no page overflow`);
      assert.deepEqual(errors, [], `${label}: browser errors`);
      console.log(`PASS ${label}`);
    }
    await page.close();
  }
} finally {
  await browser.close();
}
