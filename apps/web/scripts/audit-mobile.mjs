import { createRequire } from 'node:module';
import fs from 'node:fs';
import path from 'node:path';
const load = createRequire(import.meta.url);
const {chromium}=load(process.env.PLAYWRIGHT_MODULE || 'playwright-core');
const origin=process.env.SITE_URL||'http://localhost:3031';
const output=process.env.AUDIT_OUTPUT||'.artifacts/mobile-audit';
const routes=[
 ['landing','/landing/en.html','/landing/index.html'],['landing-docs','/landing/docs-en.html','/landing/docs.html'],['b2b','/landing/b2b-en.html','/landing/b2b.html'],
 ...['overview','keys','credits','usage','referral','support','profile'].map(s=>['dashboard-'+s,`/dashboard?view=${s}`,`/ru/dashboard?view=${s}`]),
 ['docs','/docs','/ru/docs'],['docs-errors','/docs/errors','/ru/docs/errors'],
 ...['plans','models','integrations','int-claude-code','int-codex','int-cursor','int-cline','int-opencode','int-continue','int-zed','int-sdk','terms','privacy','support'].map(s=>[s,'/'+s,'/ru/'+s]),
 ...['about','contacts','changelog','blog','status','tools/claude-api-cost-calculator'].map(s=>[s.replaceAll('/','-'),'/'+s,'/'+s]),
 ['learn','/docs/learn','/ru/docs/learn'],['learn-article','/docs/learn/how-to-buy-claude-api-key','/ru/docs/learn/how-to-buy-claude-api-key'],
 ['model','/models/claude-opus-4-8','/models/claude-opus-4-8'],['errors','/errors','/ru/errors'],['error-tool','/errors/claude-code','/ru/errors/claude-code'],['error-article','/errors/claude-code/api-error-429','/ru/errors/claude-code/api-error-429'],
 ['learn-ko','/ko/docs/learn','/ko/docs/learn'],['learn-zh','/zh/docs/learn','/zh/docs/learn'],['not-found','/audit-page-not-found','/audit-page-not-found']
];
const filter=process.env.AUDIT_FILTER?.split(',');
const sizes=process.env.AUDIT_QUICK?[[320,'ru','light']]:[[320,'ru','light'],[390,'en','light'],[390,'ru','dark'],[768,'en','dark']];
(async()=>{
 fs.mkdirSync(output,{recursive:true});
 const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_PATH || undefined});
 const results=[],jobs=routes.filter(r=>!filter||filter.includes(r[0])).flatMap(r=>sizes.map(s=>({route:r,width:s[0],lang:s[1],theme:s[2]})));
 let index=0;
 async function worker(){while(index<jobs.length){const {route,width,lang,theme}=jobs[index++];const [name,en,ru]=route;const url=origin+(lang==='ru'?ru:en);const page=await browser.newPage({viewport:{width,height:844},isMobile:width<768,hasTouch:true,deviceScaleFactor:1,reducedMotion:'reduce'});const errors=[];
  page.on('pageerror',e=>errors.push(e.message));
  await page.addInitScript(({lang,theme})=>{localStorage.setItem('lang:v1',lang);localStorage.setItem('theme:v1',theme);localStorage.setItem('apitoken-theme',theme);},{lang,theme});
  await page.route(/google-analytics|mc\.yandex|vitals\.vercel|va\.vercel/,r=>r.abort());
  try{
   const response=await page.goto(url,{waitUntil:'domcontentloaded',timeout:30000});
   await page.waitForFunction(()=>!document.querySelector('.dashboard-loading'),{timeout:10000});
   await page.evaluate(()=>document.fonts.ready);
   await page.waitForTimeout(300);
   // Activate lazy content before measuring; do not replace real layout or hide overflow.
   await page.evaluate(async()=>{for(let y=0;y<document.documentElement.scrollHeight;y+=700){window.scrollTo({top:y,behavior:'instant'});await new Promise(r=>setTimeout(r,30));}window.scrollTo({top:0,behavior:'instant'})});
   await page.waitForTimeout(100);
   const findings=await page.evaluate(()=>{
    const visible=e=>{const s=getComputedStyle(e),r=e.getBoundingClientRect();return r.width>0&&r.height>0&&s.visibility!=='hidden'&&s.display!=='none'&&s.opacity!=='0'&&!e.closest('[hidden],[inert]')};
    const describe=e=>({tag:e.tagName,cls:String(e.className).slice(0,100),text:(e.innerText||e.getAttribute('aria-label')||'').trim().slice(0,100)});
    const overflow=[],clipped=[],smallControls=[],inputs=[],missingAnchors=[];
    for(const e of document.querySelectorAll('body *')){if(!visible(e)||e.closest('svg,nextjs-portal,.sr-only,.skip,.skip-link'))continue;const r=e.getBoundingClientRect(),s=getComputedStyle(e);
     if(r.right>innerWidth+1||r.left< -1){let intentional=false;for(let p=e.parentElement;p&&p!==document.body;p=p.parentElement){const ps=getComputedStyle(p);if(['auto','scroll','hidden','clip'].includes(ps.overflowX)||ps.transform!=='none'){intentional=true;break;}}if(!intentional&&s.position!=='fixed'&&!e.matches('.side,.sidebar,.nav'))overflow.push({...describe(e),left:r.left,right:r.right});}
     if(e.childElementCount===0&&e.textContent.trim()&&e.scrollWidth>e.clientWidth+2&&['hidden','clip'].includes(s.overflowX)&&s.textOverflow!=='ellipsis')clipped.push({...describe(e),width:e.clientWidth,scroll:e.scrollWidth});
     if(e.matches('input:not([type=hidden]):not([type=range]):not([type=checkbox]):not([type=radio]),select,textarea')&&!e.disabled&&parseFloat(s.fontSize)<16)inputs.push({...describe(e),font:s.fontSize});
     if(e.matches('button,a[href],input')&&!e.disabled&&r.width<24&&r.height<24&&!e.closest('p,pre,code,.uchart-plot,.mdist'))smallControls.push({...describe(e),width:r.width,height:r.height});
    }
    for(const a of document.querySelectorAll('a[href^="#"]')){const id=a.getAttribute('href').slice(1);if(id&&!document.getElementById(id))missingAnchors.push({text:a.innerText,href:a.getAttribute('href')});}
    return {title:document.title,lang:document.documentElement.lang,theme:document.documentElement.dataset.theme||'light',pageOverflow:document.documentElement.scrollWidth>innerWidth+1,overflow:overflow.slice(0,15),clipped:clipped.slice(0,15),smallControls:smallControls.slice(0,15),inputs:inputs.slice(0,15),missingAnchors,links:[...new Set([...document.querySelectorAll('a[href]')].map(a=>a.getAttribute('href')).filter(h=>h.startsWith('/')&&!h.startsWith('//')))]};
   });
   const result={name,width,lang,theme,status:response.status(),url:page.url(),errors,...findings};results.push(result);
   if(width===390&&lang==='ru')await page.screenshot({path:path.join(output,name+'.png'),fullPage:true});
   console.log(JSON.stringify({name,width,lang,status:result.status,overflow:findings.overflow.length,clipped:findings.clipped.length,inputs:findings.inputs.length,errors:errors.length,missing:findings.missingAnchors.length}));
  }catch(e){results.push({name,width,lang,theme,error:e.message});console.log(JSON.stringify({name,width,error:e.message.slice(0,150)}));}finally{await page.close();fs.writeFileSync(path.join(output,'results.json'),JSON.stringify(results,null,2));}
 }}
 await Promise.all([worker(),worker(),worker()]);await browser.close();
 console.log('Completed '+results.length+' cases');
 const failed=results.filter(r=>r.error||r.errors?.length||r.pageOverflow||r.overflow?.length||r.clipped?.length||r.inputs?.length||r.missingAnchors?.length||(r.status!==200&&r.name!=='not-found'));
 if(failed.length){console.error('Failed cases: '+failed.map(r=>r.name+'@'+r.width).join(', '));process.exitCode=1;}
})().catch(e=>{console.error(e);process.exit(1)});
