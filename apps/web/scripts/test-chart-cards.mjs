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
  for (const width of (process.env.CHART_WIDTHS || '320,390,768,1440').split(',').map(Number)) for (const theme of ['light', 'dark']) for (const lang of ['en', 'ru']) {
    const page = await browser.newPage({ viewport: { width, height: 1000 }, isMobile: width < 768, hasTouch: width < 768, reducedMotion: 'reduce' });
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
      if (view === 'usage') {
        const card = page.locator('.usage-trend');
        await card.waitFor();
        await page.evaluate(() => document.fonts.ready);
        const plot = card.getByRole('slider');
        await plot.focus();
        await plot.press('Home');
        assert.equal(await plot.getAttribute('aria-valuenow'), '0', `${label}: first day`);
        await plot.press('ArrowRight');
        assert.equal(await plot.getAttribute('aria-valuenow'), '1', `${label}: keyboard navigation`);
        assert.equal(await card.locator('.usage-trend-detail').getAttribute('data-day-index'), '1');
        await plot.press('End');
        assert.equal(await plot.getAttribute('aria-valuenow'), await plot.getAttribute('aria-valuemax'));
        const bounds = await plot.boundingBox();
        await page.mouse.move(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2);
        assert.ok(Number(await plot.getAttribute('aria-valuenow')) > 1);
        if (width < 768) {
          await page.touchscreen.tap(bounds.x + bounds.width / 4, bounds.y + bounds.height / 2);
          assert.ok(Number(await plot.getAttribute('aria-valuenow')) < Number(await plot.getAttribute('aria-valuemax')) / 2, `${label}: touch selection`);
        }
        await page.setViewportSize({ width: width + 30, height: 1000 });
        await page.setViewportSize({ width, height: 1000 });
        assert.equal(await card.locator('.usage-trend-detail').evaluate(el => {
          const detail = el.getBoundingClientRect(), card = el.closest('.usage-trend').getBoundingClientRect();
          return detail.left >= card.left && detail.right <= card.right && detail.bottom <= card.bottom;
        }), true, `${label}: details inside card`);
        await plot.press('End');
        await plot.press('Escape');
        assert.equal(await plot.evaluate(el => el === document.activeElement), false, `${label}: keyboard dismissal`);
        const providers = await card.locator('.usage-trend-legend span').allTextContents();
        assert.ok(providers.includes('Claude') && providers.includes('GPT'), `${label}: original provider series`);
        assert.equal(await card.locator('g[data-provider]').count(), providers.length);
        assert.equal(await card.locator('.trend-provider-area').count(), providers.length);
        assert.equal(await card.locator('.trend-line').count(), providers.length);
        assert.equal(await card.locator('.trend-charged, .trend-official').count(), 0, `${label}: no comparison series`);
        assert.equal(await card.locator('.usage-trend-overview strong').textContent(), await page.locator('.usage-kpis .ovstat:first-child .num').textContent(), `${label}: original official total`);
        assert.match(await plot.getAttribute('aria-valuetext'), /Claude|GPT/, `${label}: accessible provider details`);
        assert.ok(await page.locator('.usage-model-bars li').count() > 0);
        assert.equal(await page.locator('.mdist, .usage-analytics-card').count(), 0);
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true, label);
        assert.deepEqual(errors, [], label);
        if ((width === 1440 || width === 390) && lang === 'en') {
          // Keep the sticky app header outside the component capture.
          await card.evaluate(el => window.scrollTo(0, el.getBoundingClientRect().top + scrollY - 100));
          await card.screenshot({ path: `${output}/${view}-${width}-${theme}.png` });
          await page.locator('.usage-model-ranking').screenshot({ path: `${output}/models-${width}-${theme}.png` });
        }
        console.log(`PASS ${label}`);
        continue;
      }
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
