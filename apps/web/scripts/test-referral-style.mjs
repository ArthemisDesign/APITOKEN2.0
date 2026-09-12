import {createRequire} from 'node:module';
import fs from 'node:fs';
import assert from 'node:assert/strict';
const load=createRequire(import.meta.url);
const {chromium}=load(process.env.PLAYWRIGHT_MODULE || 'playwright-core');
const origin=process.env.SITE_URL||'http://localhost:3034';
const out=process.env.AUDIT_OUTPUT||'.artifacts/referral-review';
fs.mkdirSync(out,{recursive:true});
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_PATH || undefined});
const results=[];
try {
for(const width of process.env.QUICK?[1440,390]:[1440,1024,768,390,320])for(const lang of ['ru','en'])for(const theme of ['dark','light']){
 const page=await browser.newPage({viewport:{width,height:1000},isMobile:width<768,hasTouch:width<768,reducedMotion:'reduce'});
 await page.addInitScript(({lang,theme})=>{if(window.top!==window)return;localStorage.setItem('lang:v1',lang);localStorage.setItem('theme:v1',theme)}, {lang,theme});
 const errors=[];page.on('pageerror',e=>errors.push(e.message));
 const rootPath=origin+(lang==='ru'?'/ru':'')+'/dashboard';
 await page.goto(rootPath+'?view=usage');
 await page.locator('.usage-kpis').waitFor();
 const sample=el=>{const s=getComputedStyle(el);return {background:s.backgroundColor,radius:s.borderRadius}};
 const usage=await page.locator('.usage-kpis .ovstat').evaluateAll(els=>els.map(el=>{const s=getComputedStyle(el);return {background:s.backgroundColor,radius:s.borderRadius}}));
 await page.goto(rootPath+'?view=referral');
 await page.locator('.rp-stats').waitFor();await page.evaluate(()=>document.fonts.ready);
 const referral=await page.locator('.rp-stats .rp-stat').evaluateAll(els=>els.map(el=>{const s=getComputedStyle(el);return {background:s.backgroundColor,radius:s.borderRadius}}));
 assert.deepEqual(referral,usage,'KPI surfaces must match Usage');
 assert.equal((await page.locator('.referral-earnings-graph .uchart').evaluate(sample)).background,usage[2].background);
 assert.equal(await page.locator('.rp-reflink input').evaluate(el=>el.getBoundingClientRect().height<70),true,'Referral link must remain a single-line input');
 const column=page.locator('.referral-earnings-graph .uchart-col').filter({has:page.locator('.uchart-seg')}).first();
 await column.focus();
 await page.getByRole('tooltip').waitFor();
 await page.keyboard.press('Escape');
 await page.getByRole('tooltip').waitFor({state:'hidden'});
 for(const [index,tab] of ['overview','referrals','team','payouts','docs'].entries()){
  await page.locator('.referral-subnav button').nth(index).click();
  await page.waitForFunction(tab=>new URL(location.href).searchParams.get('tab')===(tab==='overview'?null:tab),tab);
  if(tab==='referrals'){
    const input=page.locator('.referral-search input');
    await input.fill('no-match-for-style-audit');
    assert.equal(await page.locator('.referral-directory-table .referral-email').count(),0);
    await input.fill('');
  }
  if(tab==='referrals'||tab==='team'){
    const edit=page.locator(tab==='team'?'.team-table tbody button':'.referral-directory-table tbody button').first();
    await edit.click();
    const dialog=page.getByRole('dialog');
    await dialog.waitFor();
    assert.equal(await dialog.evaluate(el=>el.scrollWidth<=el.clientWidth+1),true,`${tab} dialog must not overflow at ${width}px (${lang}, ${theme})`);
    await page.keyboard.press('Escape');
    await dialog.waitFor({state:'hidden'});
  }
  const issues=await page.locator('.rp').evaluate(root=>{const found=[];for(const el of root.querySelectorAll('*')){if(el.closest('svg,[hidden],[inert]'))continue;const r=el.getBoundingClientRect(),s=getComputedStyle(el);if(!r.width||!r.height||s.visibility==='hidden')continue;let scroll=false;for(let p=el.parentElement;p&&p!==root;p=p.parentElement){if(['auto','scroll','hidden','clip'].includes(getComputedStyle(p).overflowX)){scroll=true;break;}}if(!scroll&&(r.left< -1||r.right>innerWidth+1))found.push({cls:el.className,text:el.textContent.slice(0,50),type:'overflow'});if(el.matches('input,select,textarea')&&!el.disabled&&innerWidth<821&&parseFloat(s.fontSize)<16)found.push({cls:el.className,type:'small-input'});}return found;});
  results.push({width,lang,theme,tab,issues,errors:[...errors]});
  if(lang==='ru'&&(width===1440||width===390))await page.screenshot({path:`${out}/${tab}-${width}-${theme}.png`,fullPage:true});
 }
 console.log(`PASS Referral ${width}px ${lang} ${theme}`);
 await page.close();
}
}finally{await browser.close();fs.writeFileSync(out+'/results.json',JSON.stringify(results,null,2))}
const failed=results.filter(r=>r.issues.length||r.errors.length);
console.log(JSON.stringify({total:results.length,failed},null,2));if(failed.length)process.exitCode=1;
