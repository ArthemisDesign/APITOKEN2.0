// Read-only UI checks: no sign-in, form submission, or account changes.
import { createRequire } from 'node:module';
import assert from 'node:assert/strict';
import fs from 'node:fs';
const load = createRequire(import.meta.url);
const playwright = load(process.env.PLAYWRIGHT_MODULE || 'playwright-core');
const engine = process.env.BROWSER || 'chromium';
const origin = process.env.SITE_URL || 'http://localhost:3036';
const output = process.env.AUDIT_OUTPUT || '.artifacts/mobile-chrome';
const widths = process.env.QUICK ? [320,390] : [320,390,768,844,1440];
const routes = process.env.THEME_ONLY ? [] : ['index.html','en.html','docs.html','docs-en.html','b2b.html','b2b-en.html'];
fs.mkdirSync(output, {recursive:true});
const browser = await playwright[engine].launch({headless:true, ...(engine === 'chromium' && process.env.CHROME_PATH ? {executablePath:process.env.CHROME_PATH} : {})});
const results = [];
try {
  for (const width of widths) for (const file of routes) for (const theme of ['light','dark']) {
    const lang = file.includes('en.') ? 'en' : 'ru';
    const page = await browser.newPage({viewport:{width,height:width === 844 ? 390 : 844},isMobile:width<1025,hasTouch:width<1025});
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.addInitScript(({lang,theme}) => {if(window.top!==window || sessionStorage.getItem('mobile-chrome-init'))return;sessionStorage.setItem('mobile-chrome-init','1');localStorage.setItem('lang:v1',lang);localStorage.setItem('apitoken-theme',theme);localStorage.setItem('theme:v1',theme)}, {lang,theme});
    try {
      await page.goto(`${origin}/landing/${file}`);
      await page.evaluate(() => document.fonts.ready);
      const header = page.locator('.hdr');
      const isLanding = await header.count() > 0;
      if(isLanding) {
        const box = await header.boundingBox();
        assert.equal(box.y,0,'Header must start at the viewport top');
        if(width<1025) {
          assert.equal(Math.round(box.height),68,'Mobile header has one stable height');
          await page.evaluate(() => scrollTo({top:500,behavior:'instant'}));
          await page.waitForTimeout(350);
          assert.equal(Math.round((await header.boundingBox()).height),68,'Scrolling must not resize the mobile header');
          await page.evaluate(() => scrollTo({top:0,behavior:'instant'}));
        }
      } else {
        const button = page.locator(file.startsWith('docs') ? '#docsSideTgl' : '#b2bBurger');
        if(await button.isVisible()) await button.click();
      }
      const toggle = page.locator('#themeTgl');
      await toggle.scrollIntoViewIfNeeded();
      const symbol = () => toggle.locator('.theme-icon:visible');
      assert.equal(await symbol().count(),1,'Exactly one desktop theme symbol is visible');
      assert.equal(await symbol().textContent(),theme==='dark'?'\u263E\uFE0E':'\u2600\uFE0E','Icon indicates the current theme with forced text presentation');
      await toggle.click();
      await page.waitForTimeout(350);
      assert.equal(await page.evaluate(() => document.documentElement.dataset.theme || 'light'),theme==='dark'?'light':'dark');
      assert.equal(await symbol().textContent(),theme==='dark'?'\u2600\uFE0E':'\u263E\uFE0E','Icon follows the new current theme after a click');
      await toggle.click();
      await page.waitForTimeout(350);
      const geometry = await page.locator('.lang').evaluate(el => {
        const box=el.getBoundingClientRect(), c=getComputedStyle(el), thumb=getComputedStyle(el,'::before');
        const active=el.querySelector('.is-active').getBoundingClientRect();
        const tx=thumb.transform==='none'?0:new DOMMatrixReadOnly(thumb.transform).m41;
        return {center:box.left+parseFloat(c.borderLeftWidth)+parseFloat(thumb.left)+tx+parseFloat(thumb.width)/2,active:active.left+active.width/2,width:parseFloat(thumb.width),activeWidth:active.width};
      });
      assert.ok(Math.abs(geometry.center-geometry.active)<1,'Language thumb must be centred on its label');
      assert.ok(Math.abs(geometry.width-geometry.activeWidth)<1,'Language thumb must match its segment');
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth<=innerWidth+1),true,'No page overflow');
      if(isLanding && width<1025) {
        assert.equal(await header.locator('.brand').evaluate(el=>getComputedStyle(el).color===getComputedStyle(document.body).color),true,'Wordmark follows the header foreground');
        await page.locator('#burger').click();
        await page.waitForTimeout(250);
        const menu=page.locator('#mobNav');
        const box=await menu.boundingBox();
        assert.ok(box.y>=67 && box.y+box.height<=page.viewportSize().height+1,'Menu fits below the header');
        if(width===390)await page.screenshot({path:`${output}/${file}-${theme}-menu.png`});
        await page.keyboard.press('Escape');
        assert.equal(await page.locator('#burger').getAttribute('aria-expanded'),'false');
        await page.waitForTimeout(250);
      }
      if(width===390)await page.screenshot({path:`${output}/${file}-${theme}.png`});
      const other=page.locator('.lang__link:not(.is-active)');
      const target=await other.getAttribute('href');
      await other.click();
      await page.waitForURL(url=>url.pathname===target);
      assert.equal(await page.locator('.lang .is-active').textContent(),lang==='en'?'RU':'EN');
      assert.equal(await page.evaluate(()=>localStorage.getItem('apitoken-theme')),theme,'Language navigation retains theme');
      assert.deepEqual(errors,[]);
      results.push({file,width,theme,ok:true});
      console.log(`PASS ${engine} ${file} ${width}px ${theme}`);
    } catch(error) {
      await page.screenshot({path:`${output}/FAIL-${file}-${width}-${theme}.png`});
      results.push({file,width,theme,error:error.message});
      console.error(`FAIL ${file} ${width}px ${theme}: ${error.message}`);
    } finally { await page.close(); }
  }
  // Shared app/landing preference and symbol mapping must survive navigation,
  // reloads, a stale legacy landing value, and desktop/mobile breakpoints.
  for (const width of [320,390,1440]) for (const initial of ['light','dark']) {
    const page = await browser.newPage({viewport:{width,height:900},isMobile:width<1025,hasTouch:width<1025});
    await page.addInitScript(initial => {
      if(window.top!==window || sessionStorage.getItem('theme-parity-init'))return;
      sessionStorage.setItem('theme-parity-init','1');
      localStorage.setItem('lang:v1','en');
      localStorage.setItem('theme:v1',initial);
      localStorage.setItem('apitoken-theme',initial==='dark'?'light':'dark');
    }, initial);
    const check = async theme => {
      await page.waitForFunction(theme=>(document.documentElement.dataset.theme||'light')===theme,theme);
      const icon=page.locator('.theme-symbol,.theme-icon:visible');
      await page.waitForFunction(({theme}) => {
        const icon=document.querySelector('.theme-symbol') || [...document.querySelectorAll('.theme-icon')].find(el=>getComputedStyle(el).display!=='none');
        return icon?.textContent===(theme==='dark'?'\u263E\uFE0E':'\u2600\uFE0E');
      }, {theme});
      assert.equal(await icon.textContent(),theme==='dark'?'\u263E\uFE0E':'\u2600\uFE0E');
    };
    try {
      await page.goto(`${origin}/landing/en.html`);
      await check(initial);
      await page.goto(`${origin}/models`);
      await check(initial);
      await page.locator('.theme-tgl').click();
      const next=initial==='dark'?'light':'dark';
      await check(next);
      await page.goto(`${origin}/landing/en.html`);
      await check(next);
      await page.reload();
      await check(next);
      await page.goto(`${origin}/dashboard`);
      if(width<1025)await page.locator('.app-burger').click();
      await check(next);
      await page.locator('.theme-tgl').click();
      await check(initial);
      await page.goto(`${origin}/landing/en.html`);
      await check(initial);
      results.push({file:'cross-page-theme',width,theme:initial,ok:true});
      console.log(`PASS ${engine} cross-page theme ${width}px ${initial}`);
    } catch(error) {
      results.push({file:'cross-page-theme',width,theme:initial,error:error.message});
      console.error(error.message);
    } finally { await page.close(); }
  }
} finally {
  await browser.close();
  fs.writeFileSync(`${output}/results.json`,JSON.stringify(results,null,2));
}
const failed=results.filter(result=>!result.ok);
console.log(JSON.stringify({total:results.length,failed:failed.length}));
if(failed.length)process.exitCode=1;
