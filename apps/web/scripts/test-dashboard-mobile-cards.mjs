/* Local preview fixtures only. No account or payment mutations. */
import { createRequire } from 'node:module';
import assert from 'node:assert/strict';
import fs from 'node:fs';

const load = createRequire(import.meta.url);
const engine = process.env.BROWSER || 'chromium';
const browserType = load(process.env.PLAYWRIGHT_MODULE || 'playwright-core')[engine];
const origin = process.env.SITE_URL || 'http://localhost:3044';
if (!['localhost', '127.0.0.1'].includes(new URL(origin).hostname)) throw new Error('Use local preview fixtures');
const output = process.env.AUDIT_OUTPUT || `.artifacts/dashboard-cards-${engine}`;
fs.mkdirSync(output, { recursive: true });
const routes = ['overview', 'keys', 'credits', 'usage', 'referral', 'support', 'profile',
  ...['referrals', 'team', 'payouts', 'docs'].map(tab => `referral&tab=${tab}`)];
const sizes = (process.env.CARD_WIDTHS || '320,390,430,768,1440').split(',').map(Number);
const browser = await browserType.launch({ headless: true, ...(engine === 'chromium' && process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {}) });
let passed = 0;
try {
  for (const width of sizes) for (const [lang, theme] of [['ru', 'light'], ['en', 'dark']]) {
    const page = await browser.newPage({ viewport: { width, height: 844 }, isMobile: width < 768, hasTouch: width < 768, reducedMotion: 'reduce' });
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.route(/google-analytics|mc\.yandex|vitals\.vercel|va\.vercel/, route => route.abort());
    await page.addInitScript(({ lang, theme }) => { localStorage.setItem('lang:v1', lang); localStorage.setItem('theme:v1', theme); }, { lang, theme });
    for (const view of routes) {
      const label = `${engine} ${view} ${width} ${lang} ${theme}`;
      const response = await page.goto(`${origin}${lang === 'ru' ? '/ru' : ''}/dashboard?view=${view}`);
      assert.equal(response.status(), 200, label);
      const ready = { overview: '.overview-panel', keys: '.key-table', credits: '.topup-simple', usage: '.usage-kpis', referral: '.rp-stats', support: '.support-bot', profile: '.prof-grid', 'referral&tab=referrals': '.referral-directory-table', 'referral&tab=team': '.rp-invite', 'referral&tab=payouts': '.rp-wallet', 'referral&tab=docs': '.rp-docs-stack' };
      await page.locator(ready[view]).first().waitFor();
      await page.evaluate(() => document.fonts.ready);
      await page.waitForTimeout(150);
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true, `${label}: page width`);

      // Check actual value bounds: a table's scroll container can mask broken mobile cards.
      const records = view === 'keys' ? '.key-table tbody tr' : view === 'credits' ? '.topup-history-table tbody tr' : view.startsWith('referral') ? '.rp-table tbody tr, .referral-directory-table tbody tr' : null;
      if (records && (width <= 640 || (view === 'credits' && width <= 700) || (view === 'keys' && width <= 900))) {
        const rows = page.locator(records);
        for (let i = 0; i < await rows.count(); i++) {
          const row = rows.nth(i);
          await row.scrollIntoViewIfNeeded();
          await page.waitForTimeout(80); // Allow content-visibility to render the real row.
          const issues = await row.evaluate(el => {
            const issues = [], outer = el.getBoundingClientRect();
            for (const cell of el.querySelectorAll('td')) {
              const r = cell.getBoundingClientRect();
              if (r.left < outer.left - 1 || r.right > outer.right + 1) issues.push('cell outside card');
              for (const value of cell.children) {
                const v = value.getBoundingClientRect();
                if (v.width && (v.left < r.left - 1 || v.right > r.right + 1)) issues.push(`value outside cell: ${value.className}`);
              }
            }
            return issues;
          });
          assert.deepEqual(issues, [], label);
        }
      }
      if (width <= 700 && view === 'credits') {
        assert.equal(await page.locator('.topup-history-table').evaluate(el => el.scrollWidth <= el.clientWidth + 1), true, `${label}: history must not scroll sideways`);
      }
      if (width <= 640 && (view === 'usage' || view === 'referral')) {
        const stats = page.locator(view === 'usage' ? '.usage-kpis .ovstat' : '.rp-stat');
        assert.equal(await stats.count(), 4, label);
        assert.equal(await stats.evaluateAll(els => els.every(el => getComputedStyle(el).minHeight === '0px')), true, `${label}: no empty reserved KPI rows`);
      }
      if (view === 'usage' && width <= 640) {
        const ledger = page.locator('.txh-charges').first();
        await ledger.locator('summary').click();
        const row = ledger.locator('.txh-row').first();
        await row.scrollIntoViewIfNeeded();
        await page.waitForTimeout(100);
        assert.equal(await row.locator('.txh-ref').isVisible(), true, `${label}: show model/provider`);
        assert.equal(await row.evaluate(el => el.scrollWidth <= el.clientWidth + 1), true, `${label}: ledger row fits`);
        if (width === 390) await page.screenshot({ path: `${output}/ledger-${lang}.png` });
      }
      if (view === 'keys') {
        await page.locator('[data-key-action="edit"]').first().click();
        const dialog = page.getByRole('dialog');
        await dialog.waitFor();
        assert.equal(await dialog.evaluate(el => el.scrollWidth <= el.clientWidth + 1), true, `${label}: edit dialog`);
        await page.keyboard.press('Escape');
        await dialog.waitFor({ state: 'hidden' });
        if (width === 1440) assert.equal(await page.locator('.key-table').evaluate(el => getComputedStyle(el).display), 'table', `${label}: preserve desktop table`);
      }
      if (width <= 640 && view === 'profile') {
        assert.equal(await page.locator('.uid-wrap').evaluate(el => getComputedStyle(el).gridTemplateColumns.split(' ').length), 1, `${label}: full-width ID`);
      }
      if (width === 390) {
        // Viewport captures avoid false blank cards from off-screen content-visibility.
        await page.evaluate(() => window.scrollTo({ top: 0, behavior: 'instant' }));
        await page.waitForTimeout(100);
        await page.screenshot({ path: `${output}/${view.replace('&tab=', '-')}-${lang}.png` });
        if (records && await page.locator(records).count()) {
          const row = page.locator(records).first();
          await row.evaluate(el => window.scrollTo({ top: el.getBoundingClientRect().top + scrollY - 80, behavior: 'instant' }));
          await page.waitForTimeout(150);
          await row.screenshot({ path: `${output}/${view.replace('&tab=', '-')}-card-${lang}.png` });
        }
      }
      assert.deepEqual(errors, [], label);
      console.log(`PASS ${label}`);
      passed++;
    }
    await page.close();
  }
} finally {
  await browser.close();
}
console.log(`Passed ${passed} dashboard card cases`);
