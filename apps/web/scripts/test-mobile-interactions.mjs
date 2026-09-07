/* Run against a local preview-fixture build. No live account writes. */
import { createRequire } from 'node:module';
import assert from 'node:assert/strict';
import fs from 'node:fs';
const load = createRequire(import.meta.url);
const { chromium } = load(process.env.PLAYWRIGHT_MODULE || 'playwright-core');
const origin = process.env.SITE_URL || 'http://localhost:3031';
const output = process.env.AUDIT_OUTPUT || '.artifacts/mobile-interactions';
if (!['localhost', '127.0.0.1'].includes(new URL(origin).hostname)) throw new Error('Use a local fixture server, not production');
const menus = [
  ['/landing/index.html', '#burger', '#mobNav', '#mobNav a[href="#models"]', '.mobile-nav-scrim'],
  ['/landing/en.html', '#burger', '#mobNav', '#mobNav a[href="#models"]', '.mobile-nav-scrim'],
  ['/landing/docs.html', '#docsSideTgl', '.docs-sidebar', '#docsNav a[href="#quickstart"]', '.mobile-nav-scrim'],
  ['/landing/docs-en.html', '#docsSideTgl', '.docs-sidebar', '#docsNav a[href="#quickstart"]', '.mobile-nav-scrim'],
  ['/landing/b2b.html', '#b2bBurger', '#b2bSide', '.b2b-side__link[href="#support"]', '#b2bScrim'],
  ['/landing/b2b-en.html', '#b2bBurger', '#b2bSide', '.b2b-side__link[href="#support"]', '#b2bScrim'],
  ['/ru/dashboard?view=usage', '.app-burger', '.side', '[data-dashboard-section="profile"]', '.side-scrim'],
  ['/dashboard?view=usage', '.app-burger', '.side', '[data-dashboard-section="profile"]', '.side-scrim'],
  ['/ru/models', '.nav-burger', '#site-navigation', '#site-navigation a[href="/ru/plans"]', '.site-nav-scrim'],
  ['/models', '.nav-burger', '#site-navigation', '#site-navigation a[href="/plans"]', '.site-nav-scrim'],
];
(async () => {
  fs.mkdirSync(output, { recursive: true });
  const browser = await chromium.launch({headless:true, executablePath:process.env.CHROME_PATH || undefined});
  try {
    for (const [width,height] of [[320,568],[390,844],[844,390]]) {
      for (const [route,toggle,panel,link,scrim] of process.env.MOBILE_FORMS_ONLY ? [] : menus) {
        const page = await browser.newPage({viewport:{width,height},isMobile:true,hasTouch:true});
        await page.addInitScript(lang => localStorage.setItem('lang:v1',lang), /\/ru\/|\/index.html|\/docs.html|\/b2b.html/.test(route)?'ru':'en');
        await page.goto(origin+route);
        const button=page.locator(toggle);
        await button.waitFor();
        const expectClosed = async () => {
          await page.waitForFunction(({toggle,panel})=>document.querySelector(toggle)?.getAttribute('aria-expanded')==='false'&&document.querySelector(panel)?.inert,{toggle,panel});
          assert.equal(await page.locator(panel).evaluate(el=>el.inert),true);
          assert.notEqual(await page.evaluate(()=>document.body.style.overflow),'hidden');
          assert.notEqual(await page.evaluate(()=>document.documentElement.style.overflow),'hidden');
        };
        await expectClosed();
        await button.click();
        await page.waitForFunction(()=>document.body.style.overflow==='hidden');
        assert.equal(await button.getAttribute('aria-expanded'),'true');
        // Cycle the complete keyboard order: hidden content must never receive focus.
        for(let n=0;n<35;n++) {
          await page.keyboard.press('Tab');
          assert.equal(await page.evaluate(({panel,toggle})=>document.querySelector(panel).contains(document.activeElement)||document.activeElement===document.querySelector(toggle),{panel,toggle}),true);
        }
        await page.keyboard.press('Escape');
        await expectClosed();
        assert.equal(await button.evaluate(el=>el===document.activeElement),true);
        await button.click();
        if(scrim) {
          await page.locator(scrim).click({position:{x:width-8,y:height-8}});
          await expectClosed();
          await button.click();
        }
        await page.setViewportSize({width:1440,height:900});
        await page.waitForFunction(t=>document.querySelector(t)?.getAttribute('aria-expanded')==='false',toggle);
        assert.equal(await page.locator(panel).evaluate(el=>el.inert),false);
        assert.notEqual(await page.evaluate(()=>document.body.style.overflow),'hidden');
        await page.setViewportSize({width,height});
        await expectClosed();
        await button.click();
        await page.locator(link).click();
        await expectClosed();
        console.log(`PASS menu ${route} ${width}x${height}`);
        await page.close();
      }
    }
    for(const width of [320,390]) {
      const page=await browser.newPage({viewport:{width,height:568},isMobile:true,hasTouch:true});
      await page.addInitScript(()=>localStorage.setItem('lang:v1','en'));
      await page.goto(origin+'/dashboard?view=keys');
      await page.locator('.keys-create-button').click();
      const dialog=page.getByRole('dialog');
      await dialog.waitFor();
      assert.equal(await dialog.evaluate(el=>el.scrollWidth<=el.clientWidth+1),true);
      assert.equal(await dialog.locator('input').evaluateAll(els=>els.every(el=>parseFloat(getComputedStyle(el).fontSize)>=16)),true);
      await page.screenshot({path:`${output}/key-dialog-${width}.png`});
      await page.setViewportSize({width,height:320});
      await dialog.locator('button[type="submit"],.key-modal-actions button').last().scrollIntoViewIfNeeded();
      await page.screenshot({path:`${output}/key-dialog-keyboard-${width}.png`});
      await page.keyboard.press('Escape');
      await dialog.waitFor({state:'hidden'});
      assert.notEqual(await page.evaluate(()=>document.body.style.overflow),'hidden');
      console.log(`PASS key dialog ${width}`);
      await page.goto(origin+'/dashboard?view=profile');
      await page.locator('#profile-display-name').fill('Mobile audit fixture');
      await page.getByRole('button',{name:'Save',exact:true}).click();
      await page.locator('.profile-save-success').waitFor();
      console.log(`PASS profile fixture ${width}`);
      await page.goto(origin+'/dashboard?view=credits');
      const amount=page.locator('input[name="topup-amount"]');
      await amount.fill('0');
      assert.equal(await amount.getAttribute('aria-invalid'),'true');
      await amount.fill('-1');
      assert.equal(await amount.getAttribute('aria-invalid'),'true');
      console.log(`PASS credit validation ${width}`);
      await page.close();
    }
    console.log('Mobile interaction regression checks passed');
  } finally { await browser.close(); }
})().catch(error=>{console.error(error);process.exitCode=1;});
