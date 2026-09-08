// Boundary-handler and paint regression checks, not a physical Safari UI test.
import {createRequire} from 'node:module';
import fs from 'node:fs';
import assert from 'node:assert/strict';
const load=createRequire(import.meta.url);
const playwright=load(process.env.PLAYWRIGHT_MODULE || 'playwright-core');
const engine=process.env.BROWSER || 'chromium';
const origin=process.env.SITE_URL || 'http://localhost:3039';
const output=process.env.AUDIT_OUTPUT || '.artifacts/overscroll';
fs.mkdirSync(output,{recursive:true});
const browser=await playwright[engine].launch({headless:true,...(engine==='chromium'&&process.env.CHROME_PATH?{executablePath:process.env.CHROME_PATH}:{})});
try {
  for(const width of [390,844])for(const theme of ['light','dark'])for(const lang of ['en','ru']) {
    const page=await browser.newPage({viewport:{width,height:844},isMobile:true,hasTouch:true,reducedMotion:'reduce'});
    await page.addInitScript(({theme,lang})=>{if(window.top!==window)return;localStorage.setItem('theme:v1',theme);localStorage.setItem('lang:v1',lang)},{theme,lang});
    await page.goto(`${origin}/landing/${lang==='ru'?'index':'en'}.html`);
    await page.evaluate(()=>document.fonts.ready);
    const results=await page.evaluate(async()=>{
      const root=document.scrollingElement;
      const pause=()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
      const moveTo=async top=>{scrollTo({top,behavior:'instant'});await pause()};
      const target=document.querySelector('.plate__h');
      const event=(type,el,points)=>{
        const e=new Event(type,{bubbles:true,cancelable:true});
        Object.defineProperty(e,'touches',{value:points.map(([clientX,clientY])=>({clientX,clientY}))});
        el.dispatchEvent(e);return e.defaultPrevented;
      };
      const drag=(el,dx,dy,pinch=false)=>{
        event('touchstart',el,[[100,200]]);
        const blocked=event('touchmove',el,pinch?[[100+dx,200+dy],[160,240]]:[[100+dx,200+dy]]);
        event('touchend',el,[]);return blocked;
      };
      await moveTo(0);
      const out={cssSupported:CSS.supports('overscroll-behavior-y','none'),rootPolicy:getComputedStyle(root).getPropertyValue('overscroll-behavior-y'),bodyPolicy:getComputedStyle(document.body).getPropertyValue('overscroll-behavior-y'),
        topOut:drag(target,0,30),topIn:drag(target,0,-30),horizontal:drag(target,30,2),pinch:drag(target,0,30,true)};
      await moveTo(250);
      out.middleDown=drag(target,0,30);out.middleUp=drag(target,0,-30);
      await moveTo(root.scrollHeight);
      out.bottomOut=drag(target,0,-30);out.bottomIn=drag(target,0,30);
      await moveTo(0);
      const nested=document.createElement('div');
      nested.style.cssText='position:fixed;top:100px;width:150px;height:100px;overflow:auto';
      const content=document.createElement('div');content.style.height='300px';nested.append(content);document.body.append(nested);
      nested.scrollTop=80;
      out.innerDown=drag(content,0,30);out.innerUp=drag(content,0,-30);
      nested.scrollTop=0;out.innerAtTop=drag(content,0,30);
      const input=document.createElement('input');nested.append(input);out.form=drag(input,0,30);
      nested.remove();
      return out;
    });
    const {cssSupported,rootPolicy,bodyPolicy,...gestures}=results;
    if(cssSupported){assert.equal(rootPolicy,'none');assert.equal(bodyPolicy,'none')}
    assert.deepEqual(gestures,{topOut:true,topIn:false,horizontal:false,pinch:false,
      middleDown:false,middleUp:false,bottomOut:true,bottomIn:false,innerDown:false,innerUp:false,innerAtTop:true,form:false});
    for(const scrolled of [false,true]) {
      await page.evaluate(scrolled=>scrollTo({top:scrolled?300:0,behavior:'instant'}),scrolled);
      await page.waitForTimeout(100);
      const paint=await page.locator('.hdr').evaluate(el=>{
        const bar=getComputedStyle(el),extension=getComputedStyle(el,'::after');
        return {bar:bar.backgroundColor,extension:extension.backgroundColor,height:parseFloat(extension.height),bottom:extension.bottom,pointer:extension.pointerEvents,viewport:innerHeight};
      });
      assert.equal(paint.extension,paint.bar);
      assert.ok(paint.height>=paint.viewport-1,'Extension covers one viewport, allowing subpixel rounding');
      assert.equal(paint.pointer,'none');
      // Expose 44px above the header to inspect its extension inside the screenshot.
      await page.locator('.hdr').evaluate(el=>el.style.transform='translateY(44px)');
      if(width===390&&lang==='en')await page.screenshot({path:`${output}/${theme}-${scrolled?'scrolled':'top'}-exposed.png`});
      await page.locator('.hdr').evaluate(el=>el.style.removeProperty('transform'));
    }
    console.log(`PASS ${engine} ${width}px ${lang} ${theme}: boundary handler, preserved gestures, opaque extension; CSS support=${cssSupported}`);
    await page.close();
  }
}finally{await browser.close()}
