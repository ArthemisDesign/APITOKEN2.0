import { createRequire } from 'node:module';
import assert from 'node:assert/strict';
import fs from 'node:fs';

const load = createRequire(import.meta.url);
const engine = process.env.BROWSER || 'chromium';
const browserType = load(process.env.PLAYWRIGHT_MODULE || 'playwright-core')[engine];
const origin = process.env.SITE_URL || 'http://127.0.0.1:3045';
const output = process.env.AUDIT_OUTPUT || `.artifacts/video-covers-${engine}`;
const widths = (process.env.COVER_WIDTHS || '320,390,768,1440').split(',').map(Number);
fs.mkdirSync(output, { recursive: true });
const browser = await browserType.launch({ headless: true, ...(engine === 'chromium' && process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {}) });
let cases = 0;
try {
  for (const width of widths) for (const language of ['ru', 'en']) for (const theme of ['light', 'dark']) {
    const label = `${engine} ${width}px ${language} ${theme}`;
    const page = await browser.newPage({ viewport: { width, height: 1000 }, isMobile: width < 768, hasTouch: width < 768, reducedMotion: 'reduce' });
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.addInitScript(({ language, theme }) => { localStorage.setItem('lang:v1', language); localStorage.setItem('theme:v1', theme); }, { language, theme });
    await page.route(/google-analytics|mc\.yandex|vitals\.vercel|va\.vercel/, route => route.abort());
    const response = await page.goto(`${origin}/landing/${language === 'ru' ? 'index' : 'en'}.html`);
    assert.equal(response.status(), 200, label);
    await page.locator('.vcover').first().waitFor();
    await page.evaluate(() => document.fonts.ready);
    assert.equal(await page.locator('.vcover').count(), 8, label);
    assert.equal(await page.locator('[data-video] .vcover-title').innerText(), language === 'ru' ? 'Первый\nAPI-запрос' : 'Your first\nAPI request', label);
    assert.deepEqual(await page.locator('#lib .vcover').evaluateAll(els => els.map(el => el.dataset.cover)), ['start', 'claude-code', 'cursor', 'models', 'direct', 'billing'], label);

    const checkCover = async locator => {
      await locator.scrollIntoViewIfNeeded();
      await page.waitForTimeout(120);
      const issues = await locator.evaluate(el => {
        const issues = [], box = el.getBoundingClientRect();
        for (const child of el.querySelectorAll('.vcover-brand,.vcover-title,.vcover-subtitle,.vcover-caption,.vcover-art')) {
          const r = child.getBoundingClientRect();
          if (!r.width || !r.height) continue;
          if (r.left < box.left - 1 || r.right > box.right + 1 || r.top < box.top - 1 || r.bottom > box.bottom + 1) issues.push(`outside cover: ${child.className}`);
          // Tilted illustration cards extend past their wrapper, but must stay inside the cover.
          if (!child.matches('.vcover-art') && child.scrollWidth > child.clientWidth + 1) issues.push(`clipped: ${child.className}`);
        }
        const text = el.querySelector('.vcover-copy').getBoundingClientRect();
        const control = (el.querySelector('.vcover-caption') || el.parentElement.querySelector('.vid__play')).getBoundingClientRect();
        if (text.bottom > control.top - 2) issues.push('copy overlaps playback caption');
        return issues;
      });
      assert.deepEqual(issues, [], label);
    };
    for (const cover of await page.locator('.vcover').all()) await checkCover(cover);
    for (const [index, id] of ['claude-code', 'cursor', 'codex', 'opencode', 'direct'].entries()) {
      await page.locator('#intTabs button').nth(index).click();
      const cover = page.locator('#intVideo .vcover');
      assert.equal(await cover.getAttribute('data-cover'), id, label);
      await checkCover(cover);
      const stage = page.locator('#intVideo .vid__stage');
      await stage.focus();
      await page.keyboard.press('Enter');
      assert.equal(await stage.getAttribute('aria-pressed'), 'true', label);
      await page.waitForTimeout(300);
      assert.equal(await cover.evaluate(el => getComputedStyle(el).opacity), '0', label);
      await page.keyboard.press('Space');
      assert.equal(await stage.getAttribute('aria-pressed'), 'false', label);
      await page.waitForTimeout(300);
      assert.equal(await cover.evaluate(el => getComputedStyle(el).opacity), '1', label);
      if (width === 390 && language === 'ru' && theme === 'light') await cover.screenshot({ path: `${output}/${id}.png` });
      // Exercise replacing a playing slot as well as switching from its cover.
      if (index < 4) await stage.click();
    }
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true, label);
    for (const asset of ['/assets/tools/claude-code.svg', '/assets/tools/codex.svg', '/assets/tools/opencode.svg', '/assets/providers/gemini.svg']) {
      assert.equal((await page.request.get(origin + asset)).status(), 200, `${label} ${asset}`);
    }
    if (width === 1440) {
      const library = page.locator('#lib');
      await library.scrollIntoViewIfNeeded();
      await page.waitForTimeout(150);
      await library.screenshot({ path: `${output}/library-${width}-${language}-${theme}.png` });
    }
    if (width === 390) {
      for (const [index, card] of (await page.locator('#lib .tut').all()).entries()) {
        await card.evaluate(el => window.scrollTo({ top: el.getBoundingClientRect().top + scrollY - 120, behavior: 'instant' }));
        await page.waitForTimeout(200);
        await card.screenshot({ path: `${output}/card-${index + 1}-${language}-${theme}.png` });
      }
    }
    assert.deepEqual(errors, [], label);
    console.log(`PASS ${label}`);
    cases++;
    await page.close();
  }
} finally {
  await browser.close();
}
console.log(`Passed ${cases} cover layouts, localization and interaction cases`);
