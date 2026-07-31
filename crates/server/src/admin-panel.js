'use strict';
const app=document.getElementById('app');
const errorCenter=document.getElementById('error-center');
// Единственный источник правды о страницах: сайдбар строится из этого списка.
const NAV=[
  {group:'Обзор',items:[['dashboard','Сводка','▣']]},
  {group:'Инфраструктура',items:[['subs','Подписки','◍'],['system','Система','⌘']]},
  {group:'Клиенты',items:[['users','Пользователи','◉'],['accounts','Аккаунты','▤'],['openkeys','OpenKeys','◈'],['business','B2B','◇']]},
  {group:'Деньги',items:[['topups','Пополнения','＄']]},
  {group:'Управление',items:[['admins','Админы','⚿'],['audit','Аудит','≡']]}
];
const validTabs=NAV.flatMap(group=>group.items.map(item=>item[0]));
const getTab=()=>validTabs.includes(location.hash.slice(1))?location.hash.slice(1):'dashboard';
let tab=getTab(),timer=null,recoveryTimer=null,refreshController=null,usersCache=[],adminDomainFilter='',lastRefreshAt=0;
let userPage={offset:0,limit:50,q:'',status:'',auth:''},partnerOffset=0,businessOffset=0;
let openkeysPage={offset:0,limit:50,q:'',batch:'',status:'',usage:''};
const failures=new Map();
const esc=value=>String(value??'').replace(/[&<>"']/g,char=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[char]));
// Встроенные модалки вместо window.prompt/confirm: браузеры (или галка «блокировать диалоги»)
// молча глушат нативные диалоги, и кнопки выглядят «мёртвыми». Промис → значения полей | null.
function dialog(options){return new Promise(resolve=>{const overlay=document.createElement('div'),previous=document.activeElement;overlay.className='overlay';
  const titleId='dialog-title-'+crypto.randomUUID();
  overlay.innerHTML='<div class="dialog" role="dialog" aria-modal="true" aria-labelledby="'+titleId+'"><h3 id="'+titleId+'">'+esc(options.title)+'</h3>'+(options.message?'<p class="dlg-sub">'+esc(options.message)+'</p>':'')+
    (options.fields||[]).map(field=>'<label class="dlg-label">'+esc(field.label)+'<input type="'+esc(field.type||'text')+'" name="'+esc(field.name)+'" value="'+esc(field.value||'')+'" autocomplete="off"></label>').join('')+
    '<div class="dlg-actions"><button type="button" class="btn ghost" data-dlg="cancel">Отмена</button><button type="button" class="btn '+(options.danger?'bad':'')+'" data-dlg="ok">'+esc(options.confirmLabel||'Подтвердить')+'</button></div></div>';
  const done=value=>{overlay.remove();if(previous instanceof HTMLElement)previous.focus();resolve(value)};
  overlay.addEventListener('mousedown',event=>{if(event.target===overlay)done(null)});
  overlay.querySelector('[data-dlg=cancel]').onclick=()=>done(null);
  overlay.querySelector('[data-dlg=ok]').onclick=()=>{const values={};overlay.querySelectorAll('input').forEach(input=>values[input.name]=input.value);done(values)};
  overlay.addEventListener('keydown',event=>{if(event.key==='Escape')done(null);
    if(event.key==='Enter'){event.preventDefault();overlay.querySelector('[data-dlg=ok]').click()}
    if(event.key==='Tab'){const focusable=[...overlay.querySelectorAll('button,input,select,a[href]')].filter(item=>!item.disabled);
      if(!focusable.length)return;const first=focusable[0],last=focusable.at(-1);
      if(event.shiftKey&&document.activeElement===first){event.preventDefault();last.focus()}
      else if(!event.shiftKey&&document.activeElement===last){event.preventDefault();first.focus()}}});
  document.body.appendChild(overlay);const first=overlay.querySelector('input');(first||overlay.querySelector('[data-dlg=ok]')).focus()})}
function toast(message,kind){const note=document.createElement('div');note.className='toast'+(kind==='bad'?' bad':'');
  note.setAttribute('role',kind==='bad'?'alert':'status');const text=document.createElement('span');text.textContent=message;note.appendChild(text);
  if(kind==='bad'){const close=document.createElement('button');close.type='button';close.className='icon-btn';close.setAttribute('aria-label','Закрыть сообщение');
    close.textContent='×';close.onclick=()=>note.remove();note.appendChild(close)}
  document.body.appendChild(note);setTimeout(()=>note.remove(),kind==='bad'?9000:5000)}
addEventListener('error',event=>toast('JS: '+event.message,'bad'));
const money=value=>'$'+Number(value||0).toLocaleString('en-US',{minimumFractionDigits:2,maximumFractionDigits:2});
const nanoMoney=value=>{try{let n=BigInt(String(value??'0')),neg=n<0n;if(neg)n=-n;const whole=n/1000000000n,cents=(n%1000000000n)/10000000n;
  return(neg?'−':'')+'$'+whole.toLocaleString('en-US')+'.'+cents.toString().padStart(2,'0')}catch{return'$0.00'}};
const date=(value,withTime=false)=>value?new Date(value).toLocaleString('ru-RU',withTime?{dateStyle:'short',timeStyle:'short'}:{dateStyle:'short'}):'—';
const ago=value=>{if(!value)return'—';const seconds=Math.max(0,(Date.now()-new Date(value).getTime())/1000|0);
  if(seconds<60)return'сейчас';if(seconds<3600)return(seconds/60|0)+'м';if(seconds<86400)return(seconds/3600|0)+'ч';return(seconds/86400|0)+'д'};
const pill=(value,kind='')=>'<span class="pill '+kind+'">'+esc(value)+'</span>';
const card=(label,value,hint,clickable)=>'<div class="card'+(clickable?' clickable':'')+'"'+(clickable?' data-spend-stats title="Разбивка: сутки / 7 дней / 30 дней"':'')+'><div class="label">'+esc(label)+'</div><div class="value">'+value+'</div><div class="hint">'+hint+'</div></div>';
const empty=columns=>'<tr><td colspan="'+columns+'" class="empty">данных нет</td></tr>';
const duration=seconds=>{seconds=Math.max(0,Number(seconds)||0);const days=seconds/86400|0,hours=(seconds%86400)/3600|0,minutes=(seconds%3600)/60|0;
  return days?days+'д '+hours+'ч':hours?hours+'ч '+minutes+'м':minutes+'м'};
const ratio=value=>value==null?'∞':'×'+Number(value).toLocaleString('en-US',{maximumFractionDigits:Number(value)<10?1:0});
// Русская плюрализация: plural(1,'подписка','подписки','подписок').
const plural=(n,one,few,many)=>{const a=Math.abs(n)%100,b=a%10;return a>10&&a<20?many:b>1&&b<5?few:b===1?one:many};
const count=(n,one,few,many)=>n+' '+plural(n,one,few,many);
const pager=(offset,limit,total,scope)=>'<div class="pager"><span>'+(total?offset+1:0)+'–'+Math.min(offset+limit,total)+' из '+total+'</span>'+
  '<button type="button" class="btn ghost" data-page="'+scope+'" data-offset="'+Math.max(0,offset-limit)+'" '+(offset<=0?'disabled':'')+'>Назад</button>'+
  '<button type="button" class="btn ghost" data-page="'+scope+'" data-offset="'+(offset+limit)+'" '+(offset+limit>=total?'disabled':'')+'>Дальше</button></div>';
const windowLabel=minutes=>{minutes=Number(minutes)||0;if(!minutes)return'окно';if(minutes<60)return minutes+' мин';if(minutes<1440)return Math.round(minutes/60)+' ч';return Math.round(minutes/1440)+' д'};
function shell(title,subtitle,body,badge=''){app.innerHTML='<div id="shell"><aside><div class="brand">api<i>Token</i>.sale<small>admin</small></div><nav aria-label="Разделы админ-панели">'+
  NAV.map(group=>'<div class="nav-group">'+esc(group.group)+'</div>'+group.items.map(item=>
    '<a class="nav-item'+(tab===item[0]?' on':'')+'" href="#'+item[0]+'"><span class="ico">'+item[2]+'</span>'+esc(item[1])+'</a>').join('')).join('')+
  '</nav><div class="side-foot"><span class="env">production</span><button type="button" class="theme" id="manual-refresh" title="Обновить" aria-label="Обновить текущую страницу">↻</button>'+
  '<button type="button" class="theme" id="theme" title="Сменить тему" aria-label="Сменить тему">◐</button></div></aside>'+
  '<main id="main-content"><div class="page-head"><div><h1>'+esc(title)+'</h1>'+(subtitle?'<p class="sub">'+subtitle+'</p>':'')+'</div><div class="badge">'+badge+'</div></div>'+
  body+'</main></div>';lastRefreshAt=Date.now();bindCommon()}
function bindCommon(){document.querySelectorAll('[data-spend-stats]').forEach(element=>element.onclick=()=>spendStats());
  const manual=document.getElementById('manual-refresh');if(manual)manual.onclick=()=>refresh({force:true});
  const theme=document.getElementById('theme');if(theme)theme.onclick=()=>{const next=document.documentElement.dataset.theme==='dark'?'light':'dark';
  document.documentElement.dataset.theme=next;localStorage.setItem('admin-theme',next)}}
function showLoading(){shell(NAV.flatMap(group=>group.items).find(item=>item[0]===tab)?.[1]||'Загрузка','данные загружаются, навигация уже доступна',
  '<div class="loading-grid" role="status" aria-label="Загрузка данных">'+Array.from({length:8},()=>'<div class="skeleton"></div>').join('')+'</div>')}
const sourceName=path=>({
  '/admin/dashboard':'Коммерческая сводка','/overview':'Движок','/capacity':'Ёмкость флота','/subs':'Claude-подписки',
  '/codex-subs':'GPT-подписки','/gemini-subs':'Gemini-подписки','/partner-admin/overview':'Партнёрская сводка','/partner-admin/partner-analytics':'Партнёрские аккаунты',
  '/admin/users':'Пользователи','/admin/topups':'Пополнения','/admin/audit':'Аудит','/admin/business-invites':'B2B-инвайты',
  '/openkeys-admin/keys':'Ключи OpenKeys',
  '/admin/admin-accounts':'Администраторы','/admin/admin-accounts/domains':'Домены администраторов','/spend-stats':'Статистика расхода'
})[path.split('?')[0]]||path.split('?')[0];
function renderFailures(){errorCenter.innerHTML=[...failures.entries()].filter(([,failure])=>!failure.dismissed).map(([key,failure])=>
  '<section class="error-note" role="alert"><span class="dot bad"></span><div><b>'+esc(sourceName(key))+' временно недоступен</b><p>'+esc(failure.message)+
  '<br>Панель продолжает работать. Проверка восстановления выполняется автоматически.</p></div><div class="error-actions">'+
  '<button type="button" class="icon-btn" data-retry="'+esc(key)+'" title="Повторить" aria-label="Повторить запрос">↻</button>'+
  '<button type="button" class="icon-btn" data-dismiss="'+esc(key)+'" title="Закрыть" aria-label="Закрыть сообщение">×</button></div></section>').join('');
  errorCenter.querySelectorAll('[data-dismiss]').forEach(button=>button.onclick=()=>{const failure=failures.get(button.dataset.dismiss);if(failure){failure.dismissed=true;renderFailures()}});
  errorCenter.querySelectorAll('[data-retry]').forEach(button=>button.onclick=()=>probeFailure(button.dataset.retry))}
function trackFailure(path,error){const key=path.split('?')[0],previous=failures.get(key);
  failures.set(key,{path,message:error.message||String(error),dismissed:previous?.dismissed||false});renderFailures();scheduleRecoveryProbe()}
function markHealthy(path){const key=path.split('?')[0];if(!failures.has(key))return;failures.delete(key);renderFailures();
  if(failures.size===0){toast('Соединение восстановлено. Панель сейчас обновится.');setTimeout(()=>{if(failures.size===0)location.reload()},700)}}
async function rawApi(path,options={}){const response=await fetch(path,{...options,headers:{'content-type':'application/json',...(options.headers||{})}});
  const payload=await response.json().catch(()=>({}));if(!response.ok){const message=Array.isArray(payload.message)?payload.message.join(', '):(payload.message||payload.error);
  throw new Error(message||('HTTP '+response.status))}return payload}
async function api(path,options={}){const method=String(options.method||'GET').toUpperCase(),tracked=method==='GET';
  try{const payload=await rawApi(path,{...options,signal:options.signal||(tracked?refreshController?.signal:undefined)});if(tracked)markHealthy(path);return payload}
  catch(error){if(error?.name==='AbortError')throw error;if(tracked)trackFailure(path,error);throw error}}
async function send(path,method,body){return api(path,{method,body:JSON.stringify(body)})}
async function copyText(value){if(navigator.clipboard?.writeText)return navigator.clipboard.writeText(value);
  const input=document.createElement('textarea');input.value=value;input.style.position='fixed';input.style.opacity='0';document.body.appendChild(input);input.select();
  const copied=document.execCommand('copy');input.remove();if(!copied)throw new Error('Не удалось скопировать ссылку')}
async function probeFailure(key){const failure=failures.get(key);if(!failure)return;try{await rawApi(failure.path);markHealthy(failure.path)}catch(error){failure.message=error.message||String(error);renderFailures()}}
function scheduleRecoveryProbe(){if(recoveryTimer)return;recoveryTimer=setTimeout(async()=>{recoveryTimer=null;
  if(document.hidden){scheduleRecoveryProbe();return}await Promise.all([...failures.keys()].map(probeFailure));if(failures.size)scheduleRecoveryProbe()},5000)}
function scheduleRefresh(){clearTimeout(timer);timer=null;const delay=tab==='system'||tab==='subs'?10000:tab==='dashboard'?30000:0;
  if(delay)timer=setTimeout(()=>{if(document.hidden){scheduleRefresh();return}refresh()},delay)}
async function refresh(options={}){clearTimeout(timer);refreshController?.abort();refreshController=new AbortController();tab=getTab();try{
  if(tab==='dashboard')await dashboard();if(tab==='subs')await subscriptions();if(tab==='system')await system();if(tab==='users')await users();
  if(tab==='accounts')await accounts();if(tab==='openkeys')await openkeys();if(tab==='business')await business();if(tab==='topups')await topups();if(tab==='admins')await admins();if(tab==='audit')await audit();
}catch(error){if(error?.name!=='AbortError'&&!document.getElementById('shell'))showLoading()}finally{scheduleRefresh()}}
addEventListener('hashchange',()=>{tab=getTab();showLoading();refresh()});
addEventListener('visibilitychange',()=>{if(!document.hidden&&Date.now()-lastRefreshAt>30000)refresh()});
const savedTheme=localStorage.getItem('admin-theme');
document.documentElement.dataset.theme=savedTheme||(matchMedia('(prefers-color-scheme:dark)').matches?'dark':'light');

/* ── Сводка ──────────────────────────────────────────────── */
async function dashboard(){const [data,engine,partners]=await Promise.all([
    api('/admin/dashboard').catch(()=>null),api('/overview').catch(()=>null),api('/partner-admin/overview').catch(()=>null)
  ]),u=data?.users||{},t=data?.topups||{},p=data?.platform||{};
  const engineAccounts=engine?.accounts||[],crm=engineAccounts.find(account=>String(account.handle||'').toLowerCase()==='crm-parsing');
  const degraded=!data||p.engine_error||!engine||!partners||!crm,state=degraded?'warn':'ok';
  const body='<div class="banner '+state+'"><span class="dot'+(degraded?' warn':'')+'"></span><div><b>'+(degraded?'Есть контуры, требующие внимания':'Все административные контуры доступны')+
    '</b><span class="muted">обновлено '+date(data?.generated_at,true)+' · сессий '+(p.active_sessions??'—')+' · engine errors '+(p.engine_error??'—')+
    ' · CRM '+(crm?esc(crm.status):'account missing')+'</span></div></div>'+
    '<div class="sect"><h2>Аккаунты по контурам</h2><span class="sect-sub">commerce · engine · partners · CRM</span></div><div class="cards">'+
    card('commerce accounts',u.total??'—',(u.active??'—')+' активны · '+(u.disabled??'—')+' отключены')+
    card('engine accounts',engine?engineAccounts.length:'—',engine?engineAccounts.filter(account=>account.status==='active').length+' active':'источник недоступен')+
    card('partner accounts',partners?partners.partners:'—',partners?partners.activePartners+' active · '+partners.referredUsers+' referrals':'источник недоступен')+
    card('CRM & Parsing',crm?esc(crm.status):'не найден',crm?esc(crm.handle)+' · '+money(crm.balance_usd):'нужен engine account crm-parsing')+'</div>'+
    '<div class="sect"><h2>Клиенты и регистрации</h2></div><div class="cards">'+
    card('всего клиентов',u.total??'—',(u.active??'—')+' активны · '+(u.disabled??'—')+' отключены')+
    card('OAuth-регистрации',u.registered_oauth??'—','сейчас OAuth-only '+(u.oauth_only??'—')+' · hybrid '+(u.hybrid??'—'))+
    card('обычная регистрация',u.registered_password??'—','сейчас password-only '+(u.password_only??'—'))+
    card('новые за 30 дней',u.registered_30d??'—','24ч '+(u.registered_24h??'—')+' · active 7д '+(u.active_7d??'—'))+'</div>'+
    '<div class="sect"><h2>Деньги и пополнения</h2></div><div class="cards">'+
    card('успешные пополнения',t.paid_count??'—',(t.paid_users??'—')+' платящих клиентов')+
    card('пополнено всего',data?money(t.paid_usd):'—','30д '+(data?money(t.paid_30d_usd):'—')+' · '+(t.paid_30d_count??'—')+' шт.')+
    card('ручные начисления',t.manual_count??'—',(data?money(t.manual_usd):'—')+' · 30д '+(t.manual_30d_count??'—'))+
    card('ожидают оплаты',t.pending_checkouts??'—','ошибок 30д '+(t.failed_30d??'—')+' · возвратов '+(t.refunded_count??'—'))+'</div>'+
    '<div class="sect"><h2>Платформа</h2></div><div class="cards">'+card('API-ключи',p.active_api_keys??'—','активны из '+(p.total_api_keys??'—'))+
    card('B2C / B2B',(p.b2c_users??'—')+' / '+(p.b2b_users??'—'),'клиенты по типу тарифа')+
    card('engine active',p.engine_active??'—','pending '+(p.engine_pending??'—')+' · disabled '+(p.engine_disabled??'—'))+
    card('защищены 2FA',u.totp??'—','email подтверждено '+(u.verified??'—'))+'</div>'+
    '<footer>Подробный единый список engine, commerce, partner и CRM-аккаунтов — на странице <a class="link" href="#accounts">«Аккаунты»</a>. Флоты подписок Claude, GPT и Gemini — на странице <a class="link" href="#subs">«Подписки»</a>.</footer>';
  shell('Сводка','все контуры одним взглядом',body,pill((u.active??'—')+' active',data?'ok':'warn'))}

/* ── Подписки: Claude + GPT + Gemini ─────────────────────── */
function capacityBar(util){const percent=Math.min(100,Math.max(0,Math.round((Number(util)||0)*100))),kind=percent>=95?'bad':percent>=70?'warn':'';
  return'<span class="bar"><i class="'+kind+'" style="width:'+percent+'%"></i></span><span class="bar-label">'+percent+'%</span>'}
function percentBar(percent){percent=Math.min(100,Math.max(0,Math.round(Number(percent)||0)));
  const kind=percent>=95?'bad':percent>=70?'warn':'';
  return'<span class="bar"><i class="'+kind+'" style="width:'+percent+'%"></i></span><span class="bar-label">'+percent+'%</span>'}
function remainingBar(fraction){const percent=Math.min(100,Math.max(0,Math.round((Number(fraction)||0)*100)));
  const kind=percent<=5?'bad':percent<=30?'warn':'';
  return'<span class="bar"><i class="'+kind+'" style="width:'+percent+'%"></i></span><span class="bar-label">'+percent+'%</span>'}
const deadLabel=r=>r==='permission_error'?'токен мёртв · бан':r==='authentication_error'?'токен мёртв · нужен re-auth':'токен мёртв';
async function subscriptions(){const result=await Promise.all([api('/subs').catch(()=>null),api('/capacity').catch(()=>null),api('/codex-subs').catch(()=>null),api('/gemini-subs').catch(()=>null)]);
  const subs=result[0]||{subs:[],lifetime_days:'—'},capacity=result[1],codex=result[2],gemini=result[3],subsDown=!result[0];
  const list=subs.subs||[],liveByEmail={};
  (capacity?.per_sub||[]).forEach(item=>{liveByEmail[item.email]=item});
  const dead=list.filter(item=>item.auth_state==='dead').length,suspect=list.filter(item=>item.auth_state==='suspect').length,
    cooling=(capacity?.per_sub||[]).filter(item=>item.cooling).length;
  const gptDown=codex===null,gptOff=codex&&codex.enabled===false,homes=codex?.homes||[];
  const gptAuthBad=homes.filter(h=>!h.auth_ok).length,gptProcDown=homes.filter(h=>!h.process_live).length;
  const geminiDown=gemini===null,geminiOff=gemini&&gemini.enabled===false,geminiProfiles=gemini?.profiles||[];
  const geminiEmpty=!geminiDown&&!geminiOff&&!geminiProfiles.length,geminiUnavailable=!geminiDown&&!geminiOff&&geminiProfiles.length>0&&Number(gemini.available||0)===0;
  const geminiAuthBad=geminiProfiles.filter(profile=>!profile.authenticated).length,geminiMissing=Number(gemini?.usage_metadata_missing||0);
  // Баннер: auth/fleet faults имеют приоритет над состоянием наблюдения.
  let banner='';
  if(dead)banner='<div class="banner bad"><span class="dot bad"></span><div><b>'+count(dead,'Claude-подписка с мёртвым токеном','Claude-подписки с мёртвым токеном','Claude-подписок с мёртвым токеном')+
    '</b><span class="muted">вне ротации — нужен свежий OAuth-токен (setup-token) на этот аккаунт'+(suspect?' · '+suspect+' под наблюдением':'')+'</span></div></div>';
  else if(subsDown)banner='<div class="banner warn"><span class="dot warn"></span><div><b>Claude lifecycle-источник недоступен</b><span class="muted">/subs не отвечает — GPT и Gemini ниже работают независимо</span></div></div>';
  else if(gptDown)banner='<div class="banner warn"><span class="dot warn"></span><div><b>GPT-контур (OpenAI Codex) не отвечает</b><span class="muted">данные по GPT-подпискам недоступны — проверьте openai-runtime</span></div></div>';
  else if(geminiDown)banner='<div class="banner warn"><span class="dot warn"></span><div><b>Gemini-контур не отвечает</b><span class="muted">/gemini-subs недоступен — проверьте Gemini runtime и stable origin :8794</span></div></div>';
  else if(geminiEmpty)banner='<div class="banner warn"><span class="dot warn"></span><div><b>В Gemini-пуле нет профилей</b><span class="muted">runtime работает, но Auth Bot ещё не опубликовал ни одной paid Code Assist подписки</span></div></div>';
  else if(gptAuthBad||gptProcDown)banner='<div class="banner warn"><span class="dot warn"></span><div><b>'+(gptAuthBad?count(gptAuthBad,'GPT-подписка','GPT-подписки','GPT-подписок')+' с ошибкой auth':'')+(gptAuthBad&&gptProcDown?' · ':'')+(gptProcDown?count(gptProcDown,'процесс','процесса','процессов')+' остановлен':'')+
    '</b><span class="muted">OpenAI Codex: часть homes вне ротации</span></div></div>';
  else if(geminiAuthBad||geminiUnavailable||geminiMissing)banner='<div class="banner warn"><span class="dot warn"></span><div><b>'+(geminiAuthBad?count(geminiAuthBad,'Gemini-профиль','Gemini-профиля','Gemini-профилей')+' с ошибкой auth':'')+(geminiAuthBad&&(geminiUnavailable||geminiMissing)?' · ':'')+(geminiUnavailable?'нет доступных профилей':geminiMissing?'нет usage metadata: '+geminiMissing:'')+
    '</b><span class="muted">Gemini: auth-профили исключаются из ротации; поток без финального usage списывает только консервативный hold</span></div></div>';
  else if(suspect)banner='<div class="banner warn"><span class="dot warn"></span><div><b>'+count(suspect,'подписка под наблюдением','подписки под наблюдением','подписок под наблюдением')+' (auth падает)</b><span class="muted">движок корроборирует чистыми probe; при подтверждении — пометит DEAD</span></div></div>';
  else banner='<div class="banner ok"><span class="dot"></span><div><b>Все три флота подписок в ротации</b><span class="muted">Claude '+(list.length)+' · GPT '+(gptOff?'выкл.':homes.length)+' · Gemini '+(geminiOff?'выкл.':geminiProfiles.length)+' · обновлено '+date(Date.now(),true)+'</span></div></div>';
  // Claude: lifecycle (/subs) + live ёмкость (/capacity) по маскированному email.
  const claudeRows=list.map(item=>{const live=liveByEmail[item.email]||{};
    const isDead=item.auth_state==='dead',isSuspect=item.auth_state==='suspect';
    const status=isDead?pill(deadLabel(item.dead_reason),'bad'):isSuspect?pill('под наблюдением (auth)','warn'):live.cooling?pill('cooling','warn'):pill(item.status,item.status==='active'?'ok':'warn');
    const days=Number(item.sub_days_left||0),dayKind=days<=0?'bad':days<7?'warn':'ok';
    const win=(util,resetIn,rem,avail)=>'<div>'+capacityBar(util)+'</div><div class="sub">сброс '+duration(resetIn)+'</div>';
    return '<tr><td class="left"><b>'+esc(item.email)+'</b>'+(live.calibrated===false?' <span class="pill">калибровка</span>':'')+'</td><td>'+status+
      '</td><td>'+win(live.util5h,live.reset5h_in)+'</td><td>'+win(live.util7d,live.reset7d_in)+
      '</td><td><b>'+money(live.rem5h_usd)+'</b><div class="sub">7д '+money(live.rem7d_usd)+'</div></td>'+
      '<td><span class="dot '+dayKind+'"></span> '+(days>0?days+' дн.':'—')+'<div class="sub">добавлена '+esc(String(item.added||'').slice(0,10)||'—')+'</div></td>'+
      '<td><b>'+(Number(item.peak_cap5h_usd)>0?money(item.peak_cap5h_usd):'—')+'</b><div class="sub">7д '+(Number(item.peak_cap7d_usd)>0?money(item.peak_cap7d_usd):'—')+'</div></td>'+
      '<td class="left mono" title="'+esc(item.proxy_host||'')+'">'+esc(String(item.proxy_host||'—').replace(/:[0-9]+$/,''))+'<div class="sub">до '+esc(String(item.proxy_expire||'').slice(0,10)||'—')+'</div></td></tr>'}).join('');
  const avail7d=capacity?.available_usd?.next_7d??0;
  const routableCaps=(capacity?.per_sub||[]).filter(item=>item.routable);
  const avgUtil7d=routableCaps.length?Math.round(routableCaps.reduce((sum,item)=>sum+(Number(item.util7d)||0),0)/routableCaps.length*100)+'%':'—';
  const claudeCards=card('Claude подписки',list.length,(list.length-dead-suspect)+' здоровы · '+cooling+' cooling')+
    card('Claude · доступно 7д',money(avail7d),'real-API эквивалент по флоту')+
    card('утилизация 7д средняя',avgUtil7d,'по routable подпискам')+
    card('dead / suspect',dead+' / '+suspect,dead?'нужна замена токена':suspect?'корроборация probe идёт':'флот чист');
  // GPT: per-home operational status из OpenAI-runtime.
  const totals=Array.isArray(codex?.window_totals)?codex.window_totals:[];
  const measuredTotals=totals.filter(item=>item.cap_usd!==null&&item.cap_usd!==undefined);
  const gptRemain=measuredTotals.reduce((sum,item)=>sum+Number(item.remaining_usd||0),0);
  const gptCap=measuredTotals.reduce((sum,item)=>sum+Number(item.cap_usd||0),0);
  const gptUnknown=totals.reduce((sum,item)=>sum+Math.max(0,Number(item.observed_homes||0)-Number(item.measured_homes||0)),0);
  const gptInflight=homes.reduce((sum,h)=>sum+(h.inflight||0),0);
  const gptSpend=homes.reduce((sum,h)=>sum+(Number(h.spend_usd_total)||0),0);
  const gptCards=gptOff?card('GPT подписки','выкл.','OpenAI runtime без codex-конфигурации'):
    card('GPT подписки',gptDown?'—':homes.length,gptDown?'источник недоступен':(codex.available+' доступно · '+homes.filter(h=>h.process_live).length+' live'))+
    card('GPT · остаток окон',gptDown||gptOff?'—':measuredTotals.length?money(gptRemain):'ждём Δused',gptDown||gptOff?'':measuredTotals.length?'из '+money(gptCap)+' измеренной ёмкости'+(gptUnknown?' · '+gptUnknown+' без оценки':''):'первый снимок — только якорь, без прайора')+
    card('GPT · в работе',gptDown?'—':gptInflight,'inflight turns сейчас')+
    card('GPT · потрачено',gptDown?'—':money(gptSpend),'official-price, накопительно');
  const homeStatus=h=>{const nowSec=Date.now()/1000|0;
    if(!h.process_live)return pill('процесс остановлен','bad');
    if(!h.auth_ok)return pill('ошибка auth','bad');
    if(h.cooling_until>nowSec)return pill('cooling '+duration(h.cooling_until-nowSec),'warn');
    if(h.limit_reached||h.rate_limits?.reached)return pill('лимит достигнут','warn');
    if(h.calibration_persistence_ok===false)return pill('active · calibration storage','warn');
    return pill('active','ok')};
  const gptRows=homes.map(h=>{const windows=h.windows||[];
    const bySlot=slot=>windows.find(w=>w.slot===slot);
    const winCell=w=>w?'<div>'+percentBar(w.used_percent)+'</div><div class="sub">'+windowLabel(w.window_minutes)+(w.source==='unknown'?' · ждём полный интервал':' · накоплено интервалов '+Number(w.samples||0)+' · confidence '+Math.round(Number(w.confidence||0)*100)+'%')+'</div>':'—';
    const budgetCell=w=>w?'<b>'+(w.remaining_usd==null?'—':money(w.remaining_usd))+'</b><div class="sub">остаток из '+(w.cap_usd==null?'—':money(w.cap_usd))+' · '+windowLabel(w.window_minutes)+'</div>':'—';
    const rl=h.rate_limits||{};
    const resetCell=w=>w&&w.resets_at?duration(w.resets_at-Date.now()/1000):'—';
    return '<tr><td class="left"><b class="mono">'+esc(h.id)+'</b></td><td>'+homeStatus(h)+'</td><td>'+h.inflight+'</td>'+
      '<td>'+winCell(bySlot('primary'))+(bySlot('primary')?'<div class="sub">сброс '+resetCell(rl.primary)+'</div>':'')+'</td>'+
      '<td>'+winCell(bySlot('secondary'))+(bySlot('secondary')?'<div class="sub">сброс '+resetCell(rl.secondary)+'</div>':'')+'</td>'+
      '<td>'+budgetCell(bySlot('primary'))+(bySlot('secondary')?'<div class="sub" style="margin-top:5px">secondary</div>'+budgetCell(bySlot('secondary')):'')+'</td>'+
      '<td><b>'+money(h.spend_usd_total)+'</b><div class="sub">official-price</div></td></tr>'}).join('');
  const gptTable=gptDown?'<div class="empty" style="padding:26px">OpenAI-runtime недоступен — /codex-subs не отвечает</div>':
    gptOff?'<div class="empty" style="padding:26px">Codex-контур выключен на этом runtime</div>':
    '<div class="tscroll"><table><thead><tr><th class="left">home</th><th>статус</th><th>в работе</th><th>primary (факт. окно)</th><th>secondary (факт. окно)</th><th>остаток / вместимость API $</th><th>потрачено</th></tr></thead><tbody>'+(gptRows||empty(7))+'</tbody></table></div>';
  // Gemini: официальный per-model quota catalogue и exact transport attestation.
  const geminiModels=gemini?.models||[],geminiNow=Number(gemini?.now||Date.now()/1000),geminiAffinity=gemini?.affinity||{};
  const geminiCards=geminiOff?card('Gemini подписки','выкл.','Gemini runtime без профилей'):
    card('Gemini профили',geminiDown?'—':geminiProfiles.length,geminiDown?'источник недоступен':gemini.authenticated+' authenticated')+
    card('Gemini · доступно',geminiDown||geminiOff?'—':gemini.available,'профилей готовы хотя бы к одной модели')+
    card('Gemini · модели',geminiDown||geminiOff?'—':geminiModels.length,geminiModels.map(model=>model.id+': '+model.available).join(' · '))+
    card('Gemini · в работе',geminiDown?'—':gemini.inflight,'inflight requests сейчас')+
    card('Gemini · missing usage',geminiDown?'—':geminiMissing,geminiMissing?'списан conservative hold':'authoritative settlement чист');
  const geminiRows=geminiProfiles.flatMap(profile=>(geminiModels.length?geminiModels:[{id:'—',available:0}]).map(model=>{
    const modelHealth=(profile.model_cooling||[]).find(item=>item.model_id===model.id)||{},modelCooling=modelHealth.cooling_until||0;
    const quotas=(profile.quotas||[]).filter(item=>item.model_id===model.id);
    const coolingUntil=Math.max(Number(profile.cooling_until||0),Number(modelCooling||0));
    const status=!profile.authenticated?pill('ошибка auth','bad'):coolingUntil>geminiNow?pill('cooling '+duration(coolingUntil-geminiNow),'warn'):
      Number(modelHealth.failure_streak||0)>0?pill('degraded · '+modelHealth.failure_streak,'warn'):
      Number(modelHealth.last_success_at||0)>0?pill('active','ok'):pill('не проверена','warn');
    const quotaCell=quotas.length?quotas.map(quota=>{const fraction=quota.remaining_fraction,amount=quota.remaining_amount;
      return'<div>'+(amount!==null&&amount!==undefined?'<b>'+esc(amount)+'</b>':'<b>официальный fraction</b>')+
        (fraction!==null&&fraction!==undefined?'<div>'+remainingBar(fraction)+'</div>':'')+'</div>'}).join(''):'—';
    const reset=quotas.some(quota=>quota.reset_time)?quotas.filter(quota=>quota.reset_time).map(quota=>
      '<div><b>'+duration((Date.parse(quota.reset_time)-Date.now())/1000)+'</b><div class="sub">'+date(quota.reset_time,true)+'</div></div>').join(''):'—';
    const quotaTypes=quotas.length?quotas.map(quota=>esc(quota.token_type||'—')).join('<br>'):'—';
    const readyHint=model.available+'/'+geminiProfiles.length+' профилей'+(model.soonest_ready?'<div class="sub">следующий '+duration(model.soonest_ready-geminiNow)+'</div>':'');
    const probe=profile.last_probe_at?duration(geminiNow-profile.last_probe_at)+' назад':'—';
    const quotaAge=profile.quota_updated_at?duration(geminiNow-profile.quota_updated_at)+' назад':'—';
    return '<tr><td class="left"><b class="mono">'+esc(profile.id)+'</b></td><td>'+status+'</td><td class="left"><b>'+esc(model.id)+'</b></td><td>'+readyHint+'</td><td>'+quotaCell+
      '</td><td>'+reset+'</td><td>'+quotaTypes+
      '</td><td>'+probe+'<div class="sub">quota '+quotaAge+'</div></td></tr>'})).join('');
  const geminiTable=geminiDown?'<div class="empty" style="padding:26px">Gemini runtime недоступен — /gemini-subs не отвечает</div>':
    geminiOff?'<div class="empty" style="padding:26px">Gemini-контур выключен на этом runtime</div>':
    '<div class="tscroll"><table><thead><tr><th class="left">профиль</th><th>статус</th><th class="left">модель</th><th>доступность</th><th>квота</th><th>сброс</th><th>тип</th><th>probe / quota</th></tr></thead><tbody>'+(geminiRows||empty(8))+'</tbody></table></div>';
  const transport=gemini?.transport||{};
  const geminiDetails=geminiDown||geminiOff?'':'<details><summary>Gemini transport fingerprint и cache/affinity</summary><div class="tcard"><div class="tscroll"><table><tbody>'+
    '<tr><th class="left">Antigravity</th><td class="left mono">'+esc(transport.antigravity_version||'—')+'</td><th class="left">Node</th><td class="left mono">'+esc(transport.node_version||'—')+' · '+esc(transport.http_version||'—')+'</td></tr>'+
    '<tr><th class="left">transport profile</th><td class="left mono">'+esc(transport.profile||'—')+'</td><th class="left">Node SHA-256</th><td class="left mono">'+esc(transport.node_sha256||'—')+'</td></tr>'+
    '<tr><th class="left">expected JA3</th><td class="left mono">'+esc(transport.expected_ja3||'—')+'</td><th class="left">expected JA4</th><td class="left mono">'+esc(transport.expected_ja4||'—')+'</td></tr>'+
    '<tr><th class="left">userinfo fetch</th><td class="left mono">'+esc(transport.userinfo_profile||'—')+' · '+esc(transport.userinfo_http_version||'—')+'</td><th class="left">Undici JA3 / JA4</th><td class="left mono">'+esc(transport.userinfo_expected_ja3||'—')+' / '+esc(transport.userinfo_expected_ja4||'—')+'</td></tr>'+
    '<tr><th class="left">affinity hits</th><td class="left mono">local '+esc(geminiAffinity.local_hits||0)+' · redis '+esc(geminiAffinity.redis_hits||0)+' · roots '+esc(geminiAffinity.cache_root_hits||0)+'</td><th class="left">affinity health</th><td class="left mono">miss '+esc(geminiAffinity.misses||0)+' · redis errors '+esc(geminiAffinity.redis_errors||0)+' · rebinds '+esc(geminiAffinity.rebinds||0)+'</td></tr>'+
    '</tbody></table></div></div></details>';
  const body=banner+
    '<div class="sect"><h2>Claude</h2><span class="sect-sub">Anthropic · OAuth-флот · замена через '+subs.lifetime_days+'д от добавления</span></div>'+
    '<div class="cards">'+claudeCards+'</div>'+
    '<div class="tcard" style="margin-top:12px"><div class="tscroll"><table><thead><tr><th class="left">подписка</th><th>статус</th><th>окно 5 ч</th><th>окно 7 д</th><th>остаток 5 ч</th><th>живёт ещё</th><th>пик 5 ч</th><th class="left">прокси</th></tr></thead><tbody>'+
    (claudeRows||empty(8))+'</tbody></table></div></div>'+
    '<div class="sect"><h2>GPT</h2><span class="sect-sub">OpenAI Codex · app-server homes</span></div>'+
    '<div class="cards">'+gptCards+'</div>'+
    '<div class="tcard" style="margin-top:12px">'+gptTable+'</div>'+
    '<div class="sect"><h2>Gemini</h2><span class="sect-sub">Antigravity OAuth · Cloud Code transport, quota catalogue и legacy-миграция</span></div>'+
    '<div class="cards">'+geminiCards+'</div>'+
    '<div class="tcard" style="margin-top:12px">'+geminiTable+'</div>'+geminiDetails+
    '<footer>Обновление каждые 10с, пока вкладка видима · GPT: null/«ждём Δused» означает, что есть только первый реальный якорь — прайор не подставляется · «пик» и прайор относятся только к отдельному Claude-контуру. Email и Google identity намеренно не выводятся.</footer>';
  const fleetTotal=list.length+(gptOff?0:homes.length)+(geminiOff?0:geminiProfiles.length),fleetWarn=dead||gptDown||geminiDown||geminiEmpty||geminiUnavailable||geminiAuthBad||geminiMissing;
  shell('Подписки','Claude, GPT и Gemini: здоровье, окна, quota и transport',body,pill(count(fleetTotal,'подписка','подписки','подписок'),fleetWarn?'warn':'ok'))}

/* ── Администраторы ──────────────────────────────────────── */
async function admins(){const suffix=adminDomainFilter?'?domain='+encodeURIComponent(adminDomainFilter):'',result=await Promise.all([
  api('/admin/admin-accounts'+suffix).catch(()=>null),api('/admin/admin-accounts/domains').catch(()=>null)]),data=result[0]||{},directory=result[1]||{},accounts=data.accounts||[],domains=directory.domains||[];
  const options='<option value="">все управляемые домены</option>'+domains.map(item=>'<option value="'+esc(item.domain)+'" '+(adminDomainFilter===item.domain?'selected':'')+'>'+esc(item.domain)+'</option>').join('');
  const checks=domains.map((item,index)=>'<label class="check"><input type="checkbox" name="domains" value="'+esc(item.domain)+'" '+(index===0?'checked':'')+'>'+esc(item.label)+'</label>').join('');
  const rows=accounts.map(account=>{const self=account.id===data.current_account_id,domainPills=(account.domains||[]).map(domain=>pill(domain,domain==='admin.apitoken.sale'?'info':'')).join(' ');
    const actions='<button class="btn" data-admin-action="password" data-id="'+esc(account.id)+'" data-username="'+esc(account.username)+'" data-self="'+(self?'1':'0')+'">пароль</button>'+
      '<button class="btn" data-admin-action="domains" data-id="'+esc(account.id)+'" data-username="'+esc(account.username)+'" data-domains="'+esc((account.domains||[]).join(','))+'">домены</button>'+
      '<button class="btn '+(account.status==='active'?'bad':'')+'" data-admin-action="status" data-id="'+esc(account.id)+'" data-username="'+esc(account.username)+'" data-status="'+esc(account.status)+'">'+(account.status==='active'?'отключить':'включить')+'</button>';
    return '<tr><td class="left"><b>'+esc(account.username)+'</b>'+(self?' '+pill('вы','info'):'')+'<div class="sub mono">'+esc(account.id)+'</div></td><td class="left domain-list">'+domainPills+
      '</td><td>'+pill(account.status,account.status==='active'?'ok':'bad')+'</td><td>'+date(account.password_changed_at,true)+'</td><td>'+date(account.created_at,true)+'</td><td><div class="actions">'+actions+'</div></td></tr>'}).join('');
  const external=(directory.external_domains||[]).map(item=>esc(item.domain)+' использует '+esc(item.account_system)).join(' · ');
  const body='<div class="banner ok"><span class="dot"></span><div><b>Центральное управление администраторами</b><span class="muted">Один логин можно назначить на один или несколько внутренних доменов. '+external+'.</span></div></div>'+
    '<div class="sect"><h2>Новый администратор</h2></div><form id="admin-create" class="form-card form admin-form"><div class="field"><label>Логин</label><input name="username" required maxlength="80" pattern="[A-Za-z0-9._@-]+" autocomplete="off" placeholder="new.admin"></div>'+
    '<div class="field"><label>Пароль (минимум 8)</label><input name="password" type="password" required minlength="8" maxlength="200" autocomplete="new-password"></div><div class="field"><label>Доступ к доменам</label><div class="checks">'+checks+'</div></div><button class="btn" type="submit">создать</button></form>'+
    '<div class="sect"><h2>Администраторы</h2><span class="sect-sub">точный фильтр по домену · найдено '+accounts.length+'</span></div>'+
    '<div class="toolbar"><select id="admin-domain">'+options+'</select></div>'+
    '<div class="tcard"><div class="tscroll"><table><thead><tr><th class="left">администратор</th><th class="left">домены</th><th>статус</th><th>пароль изменён</th><th>создан</th><th>действия</th></tr></thead><tbody>'+(rows||empty(6))+'</tbody></table></div></div>'+
    '<footer>Пароли хранятся только как Argon2id-хеши. Нельзя отключить или лишить main-admin доступа последнего активного администратора.</footer>';
  shell('Администраторы','identity и domain grants для управляемых доменов',body,pill(accounts.filter(account=>account.status==='active').length+' active','ok'));bindAdmins(domains)}
function bindAdmins(domains){const select=document.getElementById('admin-domain');select.onchange=()=>{adminDomainFilter=select.value;refresh({force:true})};
  const form=document.getElementById('admin-create');form.onsubmit=async event=>{event.preventDefault();const data=new FormData(form),selected=data.getAll('domains');
    if(!selected.length)return toast('Выберите хотя бы один домен.','bad');
    const values=await dialog({title:'Создать администратора',message:String(data.get('username')||''),confirmLabel:'Создать'});
    if(!values)return;const reason=PANEL_REASON;
    const button=form.querySelector('button[type=submit]');button.disabled=true;try{await send('/admin/admin-accounts','POST',{username:data.get('username'),password:data.get('password'),domains:selected,reason});
      form.reset();toast('Администратор создан.');await admins()}catch(error){toast(error.message,'bad');button.disabled=false}};
  document.querySelectorAll('[data-admin-action]').forEach(button=>button.onclick=()=>adminAction(button,domains))}
async function adminAction(button,domains){const action=button.dataset.adminAction,username=button.dataset.username,id=button.dataset.id;let body;
  if(action==='password'){const values=await dialog({title:'Сменить пароль '+username,confirmLabel:'Сменить',
      message:button.dataset.self==='1'?'Это ваш аккаунт: браузер запросит новые credentials.':'',
      fields:[{name:'first',label:'Новый пароль (минимум 8 символов)',type:'password'},{name:'second',label:'Повторите пароль',type:'password'}]});
    if(!values)return;if((values.first||'').length<8)return toast('Пароль слишком короткий.','bad');
    if(values.second!==values.first)return toast('Пароли не совпадают.','bad');body={password:values.first,reason:PANEL_REASON}}
  if(action==='domains'){const allowed=domains.map(item=>item.domain);
    const values=await dialog({title:'Домены для '+username,message:'Через запятую. Доступные: '+allowed.join(', '),confirmLabel:'Сохранить',
      fields:[{name:'value',label:'Домены',value:button.dataset.domains||''}]});
    if(!values)return;const selected=[...new Set((values.value||'').split(',').map(item=>item.trim()).filter(Boolean))];
    if(!selected.length||selected.some(item=>!allowed.includes(item)))return toast('Укажите один или несколько доменов ровно как в списке.','bad');
    body={domains:selected,reason:PANEL_REASON}}
  if(action==='status'){const next=button.dataset.status==='active'?'disabled':'active';
    const values=await dialog({title:(next==='disabled'?'Отключить ':'Включить ')+username,confirmLabel:'Выполнить',danger:next==='disabled'});
    if(!values)return;body={status:next,reason:PANEL_REASON}}
  button.disabled=true;let result;try{result=await send('/admin/admin-accounts/'+id+'/'+action,'PATCH',body)}catch(error){toast('Изменение не сохранено: '+error.message,'bad');button.disabled=false;return}
  toast('Изменение сохранено.'+(result.changed_self?' Введите новый пароль при следующем запросе.':''));
  try{await admins()}catch(error){toast('Изменение сохранено, но список не обновился: '+error.message,'bad');button.disabled=false}}

// Разбивка «кто тратит» по окнам 24ч/7д/30д: списано клиенту (с его множителем) vs real-API
// эквивалент + эффективная скидка. Открывается кликом по «потрачено» в таблицах/карточке.
// Аккаунты портала OpenKeys узнаются по handle: он задаётся при выпуске ключа
// и другого способа отличить их на стороне движка нет.
const isOpenkeys=handle=>/^openkeys-/i.test(String(handle||''));
const okBadge=handle=>isOpenkeys(handle)?'<span class="okb" title="Выпущен через OpenKeys">OpenKeys</span>':'';
// Контекст ключа (метка, номинал, продавец, профиль) по engine-аккаунту. Карта грузится
// лениво один раз за сессию вкладки; если портал недоступен — строки остаются без подписи.
let okDirPromise=null;
const okDirectory=()=>okDirPromise??=api('/openkeys-admin/lookup')
  .then(data=>new Map((data.rows||[]).map(row=>[row.engineAccountId,row])))
  .catch(()=>{okDirPromise=null;return new Map()});
const okTypeLabel=type=>type==='openai'?'OpenAI':'Claude';
const okInfo=(dir,accountId)=>{const meta=dir&&dir.get(accountId);if(!meta)return '';
  return '<div class="sub">'+esc(meta.batchLabel||'Без метки')+' · '+nanoMoney(meta.faceValueNano)+' · '+esc(meta.createdBy)+' · '+okTypeLabel(meta.apiType)+
    ' · <a class="link" href="'+esc(meta.viewUrl)+'" target="_blank" rel="noreferrer">профиль ↗</a></div>'};
async function spendStats(){let data,okDir;
  try{[data,okDir]=await Promise.all([api('/spend-stats'),okDirectory()])}
  catch(error){return toast('Статистика расхода не загрузилась: '+error.message,'bad')}
  const previous=document.activeElement;
  const displayName=handle=>handle||'—';
  const periods=[['d1','Сутки (24ч)'],['d7','7 дней'],['d30','30 дней']];
  const discount=(charge,real)=>real>0?Math.round((1-charge/real)*100)+'%':'—';
  const overlay=document.createElement('div');overlay.className='overlay';
  const render=key=>{const period=data.periods[key]||{accounts:[]};
    const providerLabel=name=>name==='openai'?'OpenAI (Codex)':name==='anthropic'?'Claude (подписки)':name;
    const providerRows=(period.providers||[]).map(item=>'<tr><td class="left"><b>'+esc(providerLabel(item.provider))+'</b></td><td>'+item.requests+
      '</td><td><b>'+money(item.charge_usd)+'</b></td><td>'+money(item.real_usd)+'</td><td>'+discount(item.charge_usd,item.real_usd)+'</td></tr>').join('');
    const rows=(period.accounts||[]).map(item=>'<tr><td class="left"><b>'+esc(displayName(item.handle))+'</b>'+okBadge(item.handle)+'<div class="sub mono">'+esc(item.handle&&displayName(item.handle)!==item.handle?item.handle:item.account)+'</div>'+okInfo(okDir,item.account)+'</td><td>'+item.requests+
      '</td><td><b>'+money(item.charge_usd)+'</b></td><td>'+money(item.real_usd)+'</td><td>'+discount(item.charge_usd,item.real_usd)+'</td><td>'+ago(item.last_ts*1000)+'</td></tr>').join('');
    // Отдельная сводка по OpenKeys: у портала своя экономика, и смешивать её
    // с обычными клиентами при разборе расхода бесполезно.
    const ok=(period.accounts||[]).filter(item=>isOpenkeys(item.handle));
    const okCharge=ok.reduce((sum,item)=>sum+(item.charge_usd||0),0);
    const okReal=ok.reduce((sum,item)=>sum+(item.real_usd||0),0);
    const okRequests=ok.reduce((sum,item)=>sum+(item.requests||0),0);
    return '<div class="cards">'+card('списано клиентам',money(period.charge_usd),period.requests+' запросов')+
      card('real-API эквивалент',money(period.real_usd),'средняя скидка '+discount(period.charge_usd,period.real_usd))+
      card('OpenKeys',money(okReal),ok.length+' ключей · '+okRequests+' запросов · списано '+money(okCharge))+'</div>'+
      '<div class="sect"><h2>По провайдерам</h2></div><div class="tcard"><div class="tscroll"><table><thead><tr><th class="left">провайдер</th><th>запросы</th><th>списано</th><th>real-API</th><th>скидка</th></tr></thead><tbody>'+
      (providerRows||empty(5))+'</tbody></table></div></div>'+
      '<div class="sect"><h2>По аккаунтам</h2></div><div class="tcard"><div class="tscroll"><table><thead><tr><th class="left">account</th><th>запросы</th><th>списано</th><th>real-API</th><th>скидка</th><th>активность</th></tr></thead><tbody>'+
      (rows||empty(6))+'</tbody></table></div></div>'};
  overlay.innerHTML='<div class="dialog wide" role="dialog" aria-modal="true" aria-labelledby="spend-title"><h3 id="spend-title">Кто тратит</h3><p class="dlg-sub">«списано» — по множителю аккаунта · «real-API» — полный эквивалент провайдера · топ-50 за окно</p>'+
    '<div class="spend-tabs">'+periods.map((item,index)=>'<button type="button" class="btn'+(index===0?' on':'')+'" data-period="'+item[0]+'">'+item[1]+'</button>').join('')+'</div>'+
    '<div id="spend-body">'+render('d1')+'</div><div class="dlg-actions"><button type="button" class="btn ghost" data-dlg="cancel">Закрыть</button></div></div>';
  const done=()=>{overlay.remove();if(previous instanceof HTMLElement)previous.focus()};
  overlay.addEventListener('mousedown',event=>{if(event.target===overlay)done()});
  overlay.querySelector('[data-dlg=cancel]').onclick=done;
  overlay.addEventListener('keydown',event=>{if(event.key==='Escape')done();
    if(event.key==='Tab'){const focusable=[...overlay.querySelectorAll('button,a[href]')].filter(item=>!item.disabled);
      if(!focusable.length)return;const first=focusable[0],last=focusable.at(-1);
      if(event.shiftKey&&document.activeElement===first){event.preventDefault();last.focus()}
      else if(!event.shiftKey&&document.activeElement===last){event.preventDefault();first.focus()}}});
  overlay.querySelectorAll('[data-period]').forEach(button=>button.onclick=()=>{overlay.querySelectorAll('[data-period]').forEach(other=>other.classList.remove('on'));
    button.classList.add('on');overlay.querySelector('#spend-body').innerHTML=render(button.dataset.period)});
  document.body.appendChild(overlay);overlay.querySelector('[data-dlg=cancel]').focus()}

/* ── Аккаунты ────────────────────────────────────────────── */
async function accounts(){const partnerLimit=50,[overview,dashboardData,partners,okDir]=await Promise.all([
    api('/overview').catch(()=>null),api('/admin/dashboard').catch(()=>null),
    api('/partner-admin/partner-analytics?sort=created_at&dir=desc&limit='+partnerLimit+'&offset='+partnerOffset).catch(()=>null),
    okDirectory()
  ]),commerceTotal=dashboardData?.users?.total||0,partnerItems=partners?.items||[],partnerTotal=partners?.totals?.total||partnerItems.length;
  if(partnerOffset>=partnerTotal&&partnerTotal>0){partnerOffset=Math.max(0,Math.floor((partnerTotal-1)/partnerLimit)*partnerLimit);return accounts()}
  const safeOverview=overview||{accounts:[]};
  const engine=safeOverview.accounts||[],crm=engine.find(account=>String(account.handle||'').toLowerCase()==='crm-parsing');
  const domains=[
    ['admin.apitoken.sale','central admin','commerce + engine + partner account control',''],
    ['admin.partners.apitoken.sale','partner admin','unchanged APIToken Partners operator console','https://admin.partners.apitoken.sale/admin'],
    ['crm.apitoken.sale','CRM & Parsing',crm?'engine account '+crm.handle+' · '+crm.status:'engine account crm-parsing is missing','https://crm.apitoken.sale'],
    ['content-studio.apitoken.sale','content studio','private editorial workspace','https://content-studio.apitoken.sale']
  ];
  const domainCards=domains.map(item=>'<div class="domain"><b>'+(item[3]?'<a class="link" target="_blank" rel="noreferrer" href="'+item[3]+'">'+esc(item[0])+'</a>':esc(item[0]))+
    '</b>'+pill(item[1],item[0]==='crm.apitoken.sale'?(crm?'ok':'warn'):'info')+'<div class="sub">'+esc(item[2])+'</div></div>').join('');
  const engineRows=engine.map(account=>{const isCrm=String(account.handle||'').toLowerCase()==='crm-parsing',domain=isCrm?'crm.apitoken.sale':'api.apitoken.sale';return '<tr><td class="left"><b>'+esc(account.handle||'—')+'</b>'+okBadge(account.handle)+
    '<div class="sub mono">'+esc(account.account)+'</div>'+okInfo(okDir,account.account)+'</td><td class="left">'+esc(domain)+'</td><td>'+pill(account.status,account.status==='active'?'ok':'bad')+'</td><td><b>'+money(account.balance_usd)+
    '</b></td><td>'+money(account.spent_usd)+'</td><td>×'+esc(account.mult)+'</td></tr>'}).join('');
  const partnerRows=partnerItems.map(partner=>'<tr><td class="left"><b>'+esc(partner.telegramUsername?'@'+partner.telegramUsername:(partner.email||partner.displayName||'—'))+
    '</b><div class="sub mono">'+esc(partner.id)+' · '+esc(partner.referralCode)+'</div></td><td>'+pill(partner.status,partner.status==='active'?'ok':partner.status==='suspended'?'bad':'warn')+
    '</td><td>'+partner.referredUsers+'</td><td>'+nanoMoney(partner.depositsTotalNano)+'</td><td>'+nanoMoney(partner.earnedTotalNano)+'</td><td>'+ago(partner.lastSeenAt)+'</td></tr>').join('');
  const body='<div class="banner '+(crm?'ok':'warn')+'"><span class="dot'+(crm?'':' warn')+'"></span><div><b>Единый реестр аккаунтов</b><span class="muted">commerce '+commerceTotal+
    ' · engine '+engine.length+' · partners '+partnerTotal+' · CRM '+(crm?'connected':'missing')+'</span></div></div><div class="sect"><h2>Внутренние домены</h2></div><div class="domain-grid">'+domainCards+
    '</div><div class="sect"><h2>Engine и service accounts</h2><span class="sect-sub">'+engine.length+'</span></div><div class="tcard"><div class="tscroll"><table><thead><tr><th class="left">account</th><th class="left">домен</th><th>статус</th><th>баланс</th><th><span data-spend-stats title="Разбивка: сутки / 7 дней / 30 дней">потрачено</span></th><th>множитель</th></tr></thead><tbody>'+
    (engineRows||empty(6))+'</tbody></table></div></div><div class="sect"><h2>Partner accounts</h2><span class="sect-sub">'+partnerTotal+'</span></div><div class="tcard"><div class="tscroll"><table><thead><tr><th class="left">партнёр</th><th>статус</th><th>рефералы</th><th>депозиты</th><th>заработано</th><th>был(а)</th></tr></thead><tbody>'+
    (partnerRows||empty(6))+'</tbody></table></div></div>'+pager(partnerOffset,partnerLimit,partnerTotal,'partners')+'<footer>Все '+commerceTotal+' commerce-аккаунтов доступны с действиями на странице <a class="link" href="#users">«Пользователи»</a>; полный partner workflow остаётся на admin.partners.apitoken.sale.</footer>';
  shell('Аккаунты','engine, commerce, partner и CRM в одном реестре',body,pill(count(commerceTotal+engine.length+partnerTotal,'запись','записи','записей'),crm?'ok':'warn'));
  document.querySelectorAll('[data-page=partners]').forEach(button=>button.onclick=()=>{partnerOffset=Number(button.dataset.offset)||0;refresh({force:true})})}

/* ── OpenKeys: партии, live-расход и управление ключами ──── */
async function openkeys(){const params=new URLSearchParams({limit:String(openkeysPage.limit),offset:String(openkeysPage.offset)});
  if(openkeysPage.q)params.set('q',openkeysPage.q);if(openkeysPage.batch)params.set('batch',openkeysPage.batch);
  if(openkeysPage.status)params.set('status',openkeysPage.status);if(openkeysPage.usage)params.set('usage',openkeysPage.usage);
  const data=await api('/openkeys-admin/keys?'+params).catch(()=>null);
  if(!data){shell('OpenKeys','выпущенные через OpenKeys ключи, партии и live-использование','<div class="banner warn"><span class="dot warn"></span><div><b>Каталог OpenKeys временно недоступен</b><span class="muted">Остальные разделы продолжают работать. Источник проверяется автоматически.</span></div></div>',pill('degraded','warn'));return}
  if(openkeysPage.offset>=data.total&&data.total>0){openkeysPage.offset=Math.max(0,Math.floor((data.total-1)/openkeysPage.limit)*openkeysPage.limit);return openkeys()}
  const summary=data.summary||{},usageLabel={unused:'не использовался',used:'используется',exhausted:'исчерпан',unavailable:'нет live-данных'};
  const batchOptions='<option value="">все партии</option>'+(data.batches||[]).map(batch=>'<option value="'+esc(batch.id)+'" '+(openkeysPage.batch===batch.id?'selected':'')+'>'+esc(batch.label||'Без метки')+' · '+esc(batch.createdBy)+' · '+date(batch.createdAt)+'</option>').join('');
  const rows=(data.rows||[]).map(item=>{const usage=item.usagePercent==null?pill(usageLabel[item.usageState]||'нет данных','warn'):
      '<div>'+percentBar(Math.min(100,item.usagePercent))+'</div><div class="sub">'+item.usagePercent+'% · '+esc(usageLabel[item.usageState]||item.usageState)+'</div>';
    const state=pill(item.enabled?'активен':'отключён',item.enabled?'ok':'bad')+'<div class="sub">'+(item.status==='delivered'?'выдан':'на складе')+'</div>';
    const action='<button class="btn '+(item.enabled?'bad':'')+'" data-openkey="'+esc(item.id)+'" data-enabled="'+(item.enabled?'1':'0')+'" data-label="'+esc(item.batchLabel||item.keyMasked)+'">'+(item.enabled?'отключить':'включить')+'</button>';
    return '<tr><td class="left"><b class="mono">'+esc(item.keyMasked)+'</b><div class="sub mono">'+esc(item.engineAccountId||item.id)+(item.apiType?' · '+okTypeLabel(item.apiType):'')+'</div></td><td class="left"><b>'+esc(item.batchLabel||'Без метки')+'</b><div class="sub mono">'+esc(item.batchId)+' · '+esc(item.createdBy)+'</div></td><td>'+state+'</td><td>'+usage+'</td><td><b>'+(item.spentNano==null?'—':nanoMoney(item.spentNano))+'</b></td><td>'+(item.remainingNano==null?'—':nanoMoney(item.remainingNano))+'</td><td>'+nanoMoney(item.faceValueNano)+'</td><td>'+date(item.deliveredAt||item.createdAt,true)+'</td><td><a class="link" href="'+esc(item.viewUrl)+'" target="_blank" rel="noreferrer">профиль ↗</a></td><td>'+action+'</td></tr>'}).join('');
  const statusOptions='<option value="">любой статус</option><option value="active" '+(openkeysPage.status==='active'?'selected':'')+'>активные</option><option value="disabled" '+(openkeysPage.status==='disabled'?'selected':'')+'>отключённые</option>';
  const usageOptions='<option value="">любое использование</option><option value="unused" '+(openkeysPage.usage==='unused'?'selected':'')+'>не использовались</option><option value="used" '+(openkeysPage.usage==='used'?'selected':'')+'>используются</option><option value="exhausted" '+(openkeysPage.usage==='exhausted'?'selected':'')+'>исчерпаны</option><option value="unavailable" '+(openkeysPage.usage==='unavailable'?'selected':'')+'>нет live-данных</option>';
  const warning=data.truncated?'<div class="banner warn"><span class="dot warn"></span><div><b>Каталог достиг защитного лимита</b><span class="muted">Фильтр применяется к 10 000 последним ключам. Уточните партию или поиск.</span></div></div>':'';
  const body=warning+'<div class="cards">'+card('ключи',data.total,'после текущих фильтров')+card('активны / отключены',(summary.active||0)+' / '+(summary.disabled||0),'управление обратимо')+
    card('используются',(summary.used||0),'не трогали '+(summary.unused||0)+' · исчерпаны '+(summary.exhausted||0))+card('потрачено',nanoMoney(summary.spentNano),'остаток '+nanoMoney(summary.remainingNano))+'</div>'+
    '<div class="sect"><h2>Каталог ключей</h2><span class="sect-sub">метка и партия всегда видны</span></div><form id="openkeys-filter" class="toolbar"><label class="sr-only" for="ok-search">Поиск OpenKeys</label><input id="ok-search" type="search" value="'+esc(openkeysPage.q)+'" placeholder="метка, маска ключа, acct_…, продавец или ID партии…">'+
    '<label class="sr-only" for="ok-batch">Партия</label><select id="ok-batch">'+batchOptions+'</select><label class="sr-only" for="ok-usage">Использование</label><select id="ok-usage">'+usageOptions+'</select><label class="sr-only" for="ok-status">Статус</label><select id="ok-status">'+statusOptions+'</select><button class="btn" type="submit">Применить</button></form>'+
    '<div class="tcard"><div class="tscroll"><table><thead><tr><th class="left">ключ</th><th class="left">метка · партия</th><th>состояние</th><th>использование</th><th>потрачено</th><th>остаток</th><th>номинал</th><th>событие</th><th>профиль</th><th>действие</th></tr></thead><tbody>'+(rows||empty(10))+'</tbody></table></div></div>'+pager(openkeysPage.offset,openkeysPage.limit,data.total,'openkeys')+
    '<footer>Live-балансы читаются bounded batch-запросами, поэтому раздел не создаёт по два запроса на каждый ключ. Полные секреты здесь никогда не передаются. Выпуск и выдача партий остаются в <a class="link" href="https://openkeys.apitoken.sale/admin" target="_blank" rel="noreferrer">OpenKeys admin ↗</a>.</footer>';
  shell('OpenKeys','отдельный реестр выпущенных ключей с фильтром по использованию',body,pill(count(data.total,'ключ','ключа','ключей'),(summary.disabled||summary.exhausted)?'warn':'ok'));bindOpenkeys()}
function bindOpenkeys(){const form=document.getElementById('openkeys-filter');form.onsubmit=event=>{event.preventDefault();openkeysPage={...openkeysPage,offset:0,q:document.getElementById('ok-search').value.trim(),batch:document.getElementById('ok-batch').value,usage:document.getElementById('ok-usage').value,status:document.getElementById('ok-status').value};refresh({force:true})};
  ['ok-batch','ok-usage','ok-status'].forEach(id=>document.getElementById(id).onchange=()=>form.requestSubmit());
  document.querySelectorAll('[data-page=openkeys]').forEach(button=>button.onclick=()=>{openkeysPage.offset=Number(button.dataset.offset)||0;refresh({force:true})});
  document.querySelectorAll('[data-openkey]').forEach(button=>button.onclick=async()=>{const enabled=button.dataset.enabled==='1',next=!enabled;
    const values=await dialog({title:(next?'Включить ':'Отключить ')+(button.dataset.label||'ключ'),message:next?'Ключ снова начнёт принимать запросы.':'Запросы перестанут проходить, но ключ и история останутся в системе.',confirmLabel:next?'Включить':'Отключить',danger:!next});
    if(!values)return;button.disabled=true;try{await send('/openkeys-admin/keys','POST',{id:button.dataset.openkey,enabled:next});toast(next?'Ключ включён.':'Ключ отключён.');await refresh({force:true})}catch(error){toast(error.message,'bad');button.disabled=false}})}

/* ── Пользователи ────────────────────────────────────────── */
async function users(){const params=new URLSearchParams({limit:String(userPage.limit),offset:String(userPage.offset)});
  if(userPage.q)params.set('q',userPage.q);if(userPage.status)params.set('status',userPage.status);if(userPage.auth)params.set('auth',userPage.auth);
  const [userData,dashboardData]=await Promise.all([
    api('/admin/users?'+params).catch(()=>null),api('/admin/dashboard').catch(()=>null)
  ]),page=userData||{users:[],total:0,limit:userPage.limit,offset:userPage.offset};
  if(userPage.offset>=page.total&&page.total>0){userPage.offset=Math.max(0,Math.floor((page.total-1)/userPage.limit)*userPage.limit);return users()}
  usersCache=page.users||[];renderUsers(dashboardData||{users:{}},page)}
function renderUsers(dashboard,page){const totalBalance=usersCache.reduce((sum,user)=>sum+Number(user.balance_usd||0),0);
  const totalSpent=usersCache.reduce((sum,user)=>sum+Number(user.spent_usd||0),0);
  const rows=usersCache.map(user=>{const pay=user.payments||{},keys=user.api_keys||{},methods=user.auth_methods||[];
    const statusKind=user.status==='disabled'?'bad':user.engine_live_status==='disabled'?'warn':'ok';
    const tier=user.customer_type==='b2b'?'B2B':(['Starter','Builder','Pro','Studio','Scale'][user.tier]||'—');
    const auth=methods.map(method=>pill(method)).join('')||'—';
    const actions=(user.engine_account_id&&user.status==='active'?'<button class="btn" data-credit="'+esc(user.id)+'" data-email="'+esc(user.email)+'">+ баланс</button>':'')+
      (user.engine_account_id&&user.customer_type==='b2c'?'<button class="btn" data-action="business" data-id="'+esc(user.id)+'" data-email="'+esc(user.email)+'" data-discount="'+(100-user.multiplier_bp/100)+'">→ B2B</button>':'')+
      (user.engine_account_id?'<button class="btn warn" data-action="bonus" data-id="'+esc(user.id)+'" data-email="'+esc(user.email)+'">− бонус</button>':'')+
      '<button class="btn ghost" data-action="sessions" data-id="'+esc(user.id)+'" data-email="'+esc(user.email)+'">сессии</button>'+
      (user.totp_enabled?'<button class="btn warn" data-action="totp" data-id="'+esc(user.id)+'" data-email="'+esc(user.email)+'">сброс 2FA</button>':'')+
      '<button class="btn '+(user.status==='active'?'bad':'')+'" data-action="'+(user.status==='active'?'disable':'enable')+'" data-id="'+esc(user.id)+'" data-email="'+esc(user.email)+'">'+(user.status==='active'?'отключить':'включить')+'</button>';
    return '<tr data-user-row data-search="'+esc((user.email+' '+(user.display_name||'')+' '+user.id).toLowerCase())+
      '" data-status="'+esc(user.status)+'" data-auth="'+esc(methods.join(' '))+'"><td class="left"><span class="dot '+statusKind+'"></span> <b>'+esc(user.email)+
      '</b><div class="sub">'+esc(user.display_name||'')+' · '+auth+(user.email_verified?pill('email ✓','ok'):pill('email ✗','warn'))+'</div></td>'+
      '<td class="left">'+esc(tier)+'<div class="sub">'+(user.multiplier_bp==null?'—':(100-user.multiplier_bp/100)+'% скидка')+'</div></td>'+
      '<td><b>'+(user.balance_usd==null?'—':money(user.balance_usd))+'</b></td><td>'+(user.spent_usd==null?'—':money(user.spent_usd))+
      '<div class="sub">30д '+money(user.spent_30d_usd)+'</div></td><td>'+(pay.paid_count?money(pay.paid_total_usd)+'<div class="sub">'+pay.paid_count+' шт.</div>':'—')+
      '</td><td>'+Number(keys.active||0)+'/'+Number(keys.total||0)+'</td><td>'+ago(user.last_seen_at)+'</td><td>'+date(user.created_at)+      '</td><td><div class="actions wrap">'+actions+'</div></td></tr>'}).join('');
  const stats=dashboard.users||{};
  const body='<div class="cards">'+card('клиенты',page.total,(stats.registered_oauth??'—')+' OAuth-рег. · '+(stats.registered_password??'—')+' обычных')+
    card('активны 7 дней',stats.active_7d??'—',(stats.disabled??'—')+' отключены')+card('баланс страницы',money(totalBalance),'только '+usersCache.length+' показанных записей')+
    card('расход страницы',money(totalSpent),'только текущая страница')+'</div><div class="sect"><h2>Все пользователи</h2></div>'+
    '<form id="user-filter" class="toolbar"><label class="sr-only" for="search">Поиск пользователей</label><input id="search" type="search" value="'+esc(userPage.q)+'" placeholder="email, имя или UUID…">'+
    '<label class="sr-only" for="status">Статус</label><select id="status"><option value="">все статусы</option>'+
    '<option value="active" '+(userPage.status==='active'?'selected':'')+'>active</option><option value="disabled" '+(userPage.status==='disabled'?'selected':'')+'>disabled</option></select>'+
    '<label class="sr-only" for="auth">Способ регистрации</label><select id="auth"><option value="">любая регистрация</option>'+
    '<option value="password" '+(userPage.auth==='password'?'selected':'')+'>password</option><option value="google" '+(userPage.auth==='google'?'selected':'')+'>Google</option>'+
    '<option value="github" '+(userPage.auth==='github'?'selected':'')+'>GitHub</option></select><button class="btn" type="submit">Найти</button></form>'+
    '<div class="tcard"><div class="tscroll"><table><thead><tr><th class="left">пользователь</th><th class="left">тариф</th><th>баланс</th>'+
    '<th><span data-spend-stats title="Разбивка: сутки / 7 дней / 30 дней">потрачено</span></th><th>пополнения</th><th>ключи</th><th>был(а)</th><th>регистрация</th><th>действия</th></tr></thead><tbody>'+
    (rows||empty(9))+'</tbody></table></div></div>'+pager(userPage.offset,userPage.limit,page.total,'users')+'<footer>Отключение синхронно блокирует engine-аккаунт и отзывает все сессии. Каждое действие аудируется.</footer>';
  shell('Пользователи','серверный поиск, балансы, ключи и действия по клиентам',body,pill(count(page.total,'клиент','клиента','клиентов'),'ok'));bindUserPage()}
function bindUserPage(){const form=document.getElementById('user-filter'),search=document.getElementById('search'),status=document.getElementById('status'),auth=document.getElementById('auth');
  form.onsubmit=event=>{event.preventDefault();userPage={...userPage,offset:0,q:search.value.trim(),status:status.value,auth:auth.value};refresh({force:true})};
  document.querySelectorAll('[data-page=users]').forEach(button=>button.onclick=()=>{userPage.offset=Number(button.dataset.offset)||0;refresh({force:true})});
  document.querySelectorAll('[data-credit]').forEach(button=>button.onclick=()=>creditUser(button));
  document.querySelectorAll('[data-action]').forEach(button=>button.onclick=()=>userAction(button))}
// Backend требует reason в каждом действии (audit_log) — панель шлёт стандартную причину,
// чтобы не заставлять оператора печатать её на каждый клик.
const PANEL_REASON='ручное действие из админ-панели';
async function creditUser(button){
  const values=await dialog({title:'Начислить баланс',message:button.dataset.email,confirmLabel:'Начислить',
    fields:[{name:'amount',label:'Сумма USD — целое число 1–99999'}]});
  if(!values)return;const value=(values.amount||'').trim(),reason=PANEL_REASON;
  if(!/^[1-9][0-9]{0,4}$/.test(value))return toast('Сумма: целое число от 1 до 99999.','bad');button.disabled=true;
  const pendingKey='admin-credit-pending:'+button.dataset.credit,payloadSignature=value+'\n'+reason;let idempotencyKey=crypto.randomUUID();
  try{const pending=JSON.parse(sessionStorage.getItem(pendingKey)||'null');if(pending?.signature===payloadSignature)idempotencyKey=pending.idempotencyKey}catch{}
  sessionStorage.setItem(pendingKey,JSON.stringify({signature:payloadSignature,idempotencyKey}));
  try{const result=await send('/admin/users/'+button.dataset.credit+'/balance-adjustments','POST',{amount_usd:value,reason,idempotency_key:idempotencyKey});
    sessionStorage.removeItem(pendingKey);toast('Готово. Новый баланс: '+money(result.balance_usd));await refresh()}
  catch(error){toast(error.message+' — idempotency key сохранён: повторите те же сумму и причину для безопасного retry.','bad');button.disabled=false}}
async function userAction(button){const action=button.dataset.action,labels={disable:'Отключить пользователя и отозвать сессии',enable:'Включить пользователя',
  sessions:'Отозвать все активные сессии',totp:'Сбросить 2FA и отозвать сессии',
  business:'Перевести в B2B и установить договорную скидку',
  bonus:'Отозвать welcome-бонус ($4). Идемпотентно: уже отозванный не спишется второй раз'};
  const values=await dialog({title:labels[action],message:button.dataset.email,confirmLabel:'Выполнить',
    fields:action==='business'?[{name:'discount',label:'Договорная скидка, % (целое 0–95)',value:button.dataset.discount}]:[],
    danger:action==='disable'||action==='bonus'});
  if(!values)return;const reason=PANEL_REASON;button.disabled=true;
  const discount=action==='business'?Number(values.discount):null;
  if(action==='business'&&(!Number.isInteger(discount)||discount<0||discount>95)){button.disabled=false;return toast('Нужно целое число от 0 до 95.','bad')}
  try{let result;if(action==='disable'||action==='enable')result=await send('/admin/users/'+button.dataset.id+'/status','PATCH',{status:action==='disable'?'disabled':'active',reason});
    if(action==='sessions')result=await send('/admin/users/'+button.dataset.id+'/sessions/revoke','POST',{reason});
    if(action==='totp')result=await send('/admin/users/'+button.dataset.id+'/totp/reset','POST',{reason});
    if(action==='business')result=await send('/admin/users/'+button.dataset.id+'/convert-to-business','POST',{reason,discountPercent:discount});
    if(action==='bonus')result=await send('/admin/users/'+button.dataset.id+'/bonus/revoke','POST',{reason});
    toast('Готово'+(result.sessions_revoked!=null?' · сессий отозвано: '+result.sessions_revoked:'')+
      (result.customer_type==='b2b'?' · B2B, скидка '+result.discount_percent+'%':'')+
      (result.balance_usd!=null?' · новый баланс: $'+result.balance_usd+(result.idempotent_replay?' (уже был отозван ранее)':''):''));
    await refresh()}catch(error){toast(error.message,'bad');button.disabled=false}}

/* ── Пополнения ──────────────────────────────────────────── */
async function topups(){const data=await api('/admin/topups?limit=200').catch(()=>({payments:[],checkouts:[]})),payments=data.payments||[],checkouts=data.checkouts||[];
  const paymentRows=payments.map(item=>'<tr><td class="left"><b>'+esc(item.email)+'</b><div class="sub mono">'+esc(item.user_id)+'</div></td><td>'+pill(item.provider)+
    '</td><td><b>'+money(item.amount_usd)+'</b></td><td>'+pill(item.status,item.status==='paid'?'ok':'warn')+'</td><td>'+
    pill(item.credit_status||'—',item.credit_status==='confirmed'?'ok':'warn')+'</td><td>'+date(item.paid_at,true)+'</td><td class="left mono muted">'+esc(item.provider_payment_id)+'</td></tr>').join('');
  const checkoutRows=checkouts.map(item=>'<tr><td class="left"><b>'+esc(item.email)+'</b></td><td>'+pill(item.provider)+'</td><td><b>'+money(item.amount_usd)+
    '</b></td><td>'+pill(item.status,item.status==='pending'?'warn':'bad')+'</td><td>'+date(item.created_at,true)+'</td><td>'+date(item.expires_at,true)+
    '</td><td class="left mono muted">'+esc(item.provider_payment_id||'—')+'</td></tr>').join('');
  const body='<div class="sect"><h2>Подтверждённые платежи</h2><span class="sect-sub">'+payments.length+'</span></div><div class="tcard"><div class="tscroll"><table><thead><tr><th class="left">клиент</th><th>провайдер</th><th>сумма</th>'+
    '<th>платёж</th><th>зачисление</th><th>оплачен</th><th class="left">provider id</th></tr></thead><tbody>'+(paymentRows||empty(7))+'</tbody></table></div></div>'+
    '<div class="sect"><h2>Незавершённые и проблемные checkout</h2><span class="sect-sub">'+checkouts.length+'</span></div><div class="tcard"><div class="tscroll"><table><thead><tr><th class="left">клиент</th><th>провайдер</th>'+
    '<th>сумма</th><th>статус</th><th>создан</th><th>истекает</th><th class="left">provider id</th></tr></thead><tbody>'+(checkoutRows||empty(7))+
    '</tbody></table></div></div><footer>Последние 200 записей. Worker зачисляет баланс только после верифицированного платежа.</footer>';
  shell('Пополнения','платежи и checkout-воронка',body,pill(count(payments.length,'платёж','платежа','платежей'),'ok'))}

/* ── B2B ─────────────────────────────────────────────────── */
async function business(){const clientLimit=50,[inviteData,userData]=await Promise.all([
    api('/admin/business-invites?limit=100').catch(()=>null),
    api('/admin/users?limit='+clientLimit+'&offset='+businessOffset+'&customer_type=b2b').catch(()=>null)
  ]),invites=inviteData?.invites||[],clients=userData?.users||[],clientTotal=userData?.total||clients.length;
  if(businessOffset>=clientTotal&&clientTotal>0){businessOffset=Math.max(0,Math.floor((clientTotal-1)/clientLimit)*clientLimit);return business()}
  const clientRows=clients.map(user=>'<tr><td class="left"><b>'+esc(user.email)+'</b><div class="sub mono">'+esc(user.id)+'</div></td><td><b>'+
    (100-user.multiplier_bp/100)+'%</b></td><td>'+money(user.balance_usd)+'</td><td>'+pill(user.engine_account_status||'—',user.engine_account_status==='active'?'ok':'warn')+
    '</td><td>'+pill(user.pricing_sync_status||'—',user.pricing_sync_status==='confirmed'?'ok':user.pricing_sync_status==='failed'?'bad':'warn')+
    (user.pricing_sync_error?'<div class="sub">'+esc(user.pricing_sync_error)+'</div>':'')+'</td><td><button class="btn" data-pricing="'+esc(user.id)+'" data-email="'+esc(user.email)+'" data-discount="'+(100-user.multiplier_bp/100)+'">изменить скидку</button></td></tr>').join('');
  const inviteRows=invites.map(invite=>{const active=!invite.consumed_at&&!invite.revoked_at&&new Date(invite.expires_at)>new Date();
    const state=invite.consumed_at?pill('использован','ok'):invite.revoked_at?pill('отозван','bad'):new Date(invite.expires_at)<new Date()?pill('истёк','bad'):pill('активен','warn');
    const delivery=invite.email?pill(invite.delivery_status,invite.delivery_status==='sent'?'ok':invite.delivery_status==='failed'?'bad':'warn'):pill('copy only','info');
    const actions=active?'<div class="actions wrap"><button class="btn" data-invite-copy="'+esc(invite.id)+'">копировать</button>'+
      (invite.email?'<button class="btn" data-invite-resend="'+esc(invite.id)+'">отправить заново</button>':'')+
      '<button class="btn bad" data-invite-revoke="'+esc(invite.id)+'">отозвать</button></div>':'';
    return '<tr><td class="left"><b>'+esc(invite.email||'Без привязки к email')+'</b><div class="sub mono">'+esc(invite.id)+'</div></td><td>'+invite.discount_percent+'%</td><td>'+state+
    '</td><td>'+delivery+(invite.delivery_error?'<div class="sub">'+esc(invite.delivery_error)+'</div>':'')+'</td><td>'+date(invite.expires_at,true)+'</td><td>'+actions+'</td></tr>'}).join('');
  const body='<div class="sect"><h2>Новый B2B-инвайт</h2></div><form id="invite" class="form-card form"><div class="field"><label>Email (необязательно)</label><input name="email" type="email" placeholder="client@company.com"><div class="sub">Есть email — письмо уйдёт автоматически. Нет email — ссылка скопируется.</div></div>'+
    '<div class="field"><label>Скидка, %</label><input name="discount" type="number" min="0" max="95" value="70" required></div>'+
    '<div class="field"><label>Срок, дней</label><input name="days" type="number" min="1" max="30" value="7" required></div><button class="btn" type="submit">создать инвайт</button></form>'+
    '<div class="sect"><h2>B2B-клиенты</h2><span class="sect-sub">'+clientTotal+'</span></div><div class="tcard"><div class="tscroll"><table><thead><tr><th class="left">клиент</th><th>скидка</th><th>баланс</th><th>engine</th><th>синхронизация цены</th><th></th></tr></thead><tbody>'+
    (clientRows||empty(6))+'</tbody></table></div></div><div class="sect"><h2>Последние инвайты</h2><span class="sect-sub">'+invites.length+'</span></div><div class="tcard"><div class="tscroll"><table><thead><tr><th class="left">получатель</th>'+
    '<th>скидка</th><th>статус</th><th>доставка</th><th>истекает</th><th>действия</th></tr></thead><tbody>'+(inviteRows||empty(6))+'</tbody></table></div></div>';
  shell('B2B','инвайты и индивидуальные скидки',body+pager(businessOffset,clientLimit,clientTotal,'business'),pill(count(clientTotal,'клиент','клиента','клиентов'),'ok'));bindBusiness()}
function bindBusiness(){const form=document.getElementById('invite');form.onsubmit=async event=>{event.preventDefault();const data=new FormData(form),button=form.querySelector('button');button.disabled=true;
  const email=String(data.get('email')||'').trim(),discountPercent=Number(data.get('discount')),expiresInDays=Number(data.get('days')),reason=PANEL_REASON;
  const signature=[email,discountPercent,expiresInDays,reason].join('\n'),keyName='business-invite-pending';let idempotencyKey=crypto.randomUUID();
  try{const pending=JSON.parse(sessionStorage.getItem(keyName)||'null');if(pending?.signature===signature)idempotencyKey=pending.idempotencyKey}catch{}
  sessionStorage.setItem(keyName,JSON.stringify({signature,idempotencyKey}));
  const payload={discountPercent,expiresInDays,reason,idempotencyKey};if(email)payload.email=email;
  try{const result=await send('/admin/business-invites','POST',payload);sessionStorage.removeItem(keyName);
    if(email)toast('Инвайт создан: письмо поставлено в очередь для '+email);
    else{await copyText(result.inviteUrl);toast('Инвайт создан, ссылка скопирована.')}
    await refresh()}catch(error){toast(error.message+' — безопасный ключ повтора сохранён.','bad');button.disabled=false}};
  document.querySelectorAll('[data-page=business]').forEach(button=>button.onclick=()=>{businessOffset=Number(button.dataset.offset)||0;refresh({force:true})});
  document.querySelectorAll('[data-invite-copy]').forEach(button=>button.onclick=async()=>{button.disabled=true;try{const result=await api('/admin/business-invites/'+button.dataset.inviteCopy+'/link');await copyText(result.inviteUrl);toast('Ссылка скопирована.')}catch(error){toast(error.message,'bad')}finally{button.disabled=false}});
  document.querySelectorAll('[data-invite-revoke]').forEach(button=>button.onclick=async()=>{const values=await dialog({title:'Отозвать B2B-инвайт',confirmLabel:'Отозвать',danger:true});if(!values)return;button.disabled=true;try{await send('/admin/business-invites/'+button.dataset.inviteRevoke+'/revoke','POST',{reason:PANEL_REASON});toast('Инвайт отозван.');await refresh()}catch(error){toast(error.message,'bad');button.disabled=false}});
  document.querySelectorAll('[data-invite-resend]').forEach(button=>button.onclick=async()=>{const values=await dialog({title:'Заменить ссылку и отправить новое письмо',confirmLabel:'Отправить',fields:[{name:'days',label:'Новый срок, дней (1–30)',value:'7'}]});if(!values)return;const days=Number(values.days);if(!Number.isInteger(days)||days<1||days>30)return toast('Срок: целое число от 1 до 30.','bad');button.disabled=true;
    const pendingKey='business-invite-resend:'+button.dataset.inviteResend,signature=String(days);let idempotencyKey=crypto.randomUUID();try{const pending=JSON.parse(sessionStorage.getItem(pendingKey)||'null');if(pending?.signature===signature)idempotencyKey=pending.idempotencyKey}catch{}sessionStorage.setItem(pendingKey,JSON.stringify({signature,idempotencyKey}));
    try{await send('/admin/business-invites/'+button.dataset.inviteResend+'/resend','POST',{reason:PANEL_REASON,expiresInDays:days,idempotencyKey});sessionStorage.removeItem(pendingKey);toast('Старая ссылка отозвана, новое письмо поставлено в очередь.');await refresh()}catch(error){toast(error.message+' — безопасный ключ повтора сохранён.','bad');button.disabled=false}});
  document.querySelectorAll('[data-pricing]').forEach(button=>button.onclick=async()=>{
    const values=await dialog({title:'Скидка для '+button.dataset.email,confirmLabel:'Установить',
      fields:[{name:'value',label:'Скидка, % (целое 0–95)',value:button.dataset.discount}]});
    if(!values)return;const discount=Number(values.value);if(!Number.isInteger(discount)||discount<0||discount>95)return toast('Нужно целое число от 0 до 95.','bad');
    button.disabled=true;try{await send('/admin/business-users/'+button.dataset.pricing+'/pricing','PATCH',{discountPercent:discount,reason:PANEL_REASON});
    toast('Изменение поставлено в очередь синхронизации.');await refresh()}catch(error){toast(error.message,'bad');button.disabled=false}})}

/* ── Система: ёмкость флота, спрос, рекомендации ─────────── */
function systemVerdict(overview){const gap=overview.recommend.gap,h5=overview.headroom['5h'],h7=overview.headroom['7d'],target=overview.target_headroom;
  const coverage=overview.coverage['7d'],cooling=overview.supply.health.cooling,total=overview.subs,critical=value=>value!=null&&value<1,tight=value=>value!=null&&value<target;
  if(critical(h5)||critical(h7)||(total>0&&cooling>=total))return{kind:'bad',title:'Дефицит ёмкости — нужно +'+Math.max(1,gap)+' подписок',
    detail:'headroom 5h '+ratio(h5)+' / 7d '+ratio(h7)+' · потребление близко к потолку'};
  if(gap>0||tight(h5)||tight(h7)||coverage>1||cooling>0){const why=gap>0?'рекомендуется +'+gap+' подписок':tight(h5)||tight(h7)?'запас ниже цели ×'+target:
    coverage>1?'балансы клиентов ×'+coverage+' к ёмкости':cooling+' подписок остывают';return{kind:'warn',title:'Под контролем, но нужно внимание',detail:why+' · headroom 5h '+ratio(h5)+' / 7d '+ratio(h7)}}
  return{kind:'ok',title:'Запаса ёмкости хватает',detail:'headroom 5h '+ratio(h5)+' / 7d '+ratio(h7)+' · подписок '+total+', цель ×'+target+' выдержана'}}
async function system(){const result=await Promise.all([api('/overview').catch(()=>null),api('/capacity').catch(()=>null),okDirectory()]),overview=result[0],capacity=result[1],okDir=result[2];
  if(!overview){shell('Система','ёмкость, спрос и рекомендации по флоту','<div class="banner warn"><span class="dot warn"></span><div><b>Свежая системная сводка недоступна</b><span class="muted">Остальные разделы работают. Панель автоматически проверяет восстановление источника.</span></div></div>',pill('degraded','warn'));return}
  const supply=overview.supply,health=supply.health,demand=overview.demand,recommend=overview.recommend,verdict=systemVerdict(overview),mult=overview.ref_mult;
  const horizonCards=[['7d','7 дней','7d'],['1d','1 день',null],['5h','5 часов (burst)','5h']].map(item=>{const available=supply.avail_usd[item[0]]||0,head=item[2]?overview.headroom[item[2]]:null;
    return card('доступно · '+item[1],money(available),'клиентам ×'+mult+' = '+money(available*mult)+(item[2]?' · запас '+ratio(head):''))}).join('');
  const accountRows=(overview.accounts||[]).map(account=>'<tr><td class="left mono muted">'+esc(account.account)+'</td><td class="left"><b>'+esc(account.handle||'—')+'</b>'+okBadge(account.handle)+okInfo(okDir,account.account)+'</td><td>'+
    pill(account.status,account.status==='active'?'ok':'bad')+'</td><td><b>'+money(account.balance_usd)+'</b></td><td><b>'+money(account.spent_usd)+'</b></td><td>×'+esc(account.mult)+'</td></tr>').join('');
  const body='<div class="banner '+verdict.kind+'"><span class="dot'+(verdict.kind==='ok'?'':' '+verdict.kind)+'"></span><div><b>'+esc(verdict.title)+'</b><span class="muted">'+esc(verdict.detail)+'</span></div></div>'+
    '<div class="sect"><h2>Предложение — real-API USD</h2></div><div class="cards">'+horizonCards+card('балансы клиентов',money(demand.balance_usd),'резерв '+money(demand.reserved_usd)+' · coverage 7d ×'+overview.coverage['7d'])+
    '</div><div class="sect"><h2>Флот и спрос</h2></div><div class="cards">'+card('подписки',overview.subs,health.healthy+' живых · '+health.cooling+' cooling')+
    card('утилизация средняя',Math.round(supply.util['7d']*100)+'%','7d · '+Math.round(supply.util['5h']*100)+'% за 5h')+
    card('всего потрачено',money(demand.spent_usd),'потенциальный спрос '+money(demand.potential_realapi_usd)+' real-API',true)+
    card('рекомендация',recommend.gap>0?'+'+recommend.gap:'ok','нужно '+recommend.subs_needed+' подписок · есть '+overview.subs)+'</div>'+
    '<div class="sect"><h2>Подписки</h2><span class="sect-sub">живой статус флота</span></div>'+
    '<div class="banner ok" style="margin-bottom:0"><span class="dot"></span><div><b>Детальный статус подписок — на отдельной странице</b><span class="muted"><a class="link" href="#subs">Открыть «Подписки»</a> — окна, cooling, quota, lifecycle и transport по Claude, GPT и Gemini.</span></div></div>'+
    '<div class="sect"><h2>Аккаунты движка · '+(overview.accounts||[]).length+'</h2></div><div class="tcard"><div class="tscroll"><table><thead><tr><th class="left">account</th><th class="left">handle</th><th>статус</th><th>баланс</th><th><span data-spend-stats title="Разбивка: сутки / 7 дней / 30 дней">потрачено</span></th><th>множитель</th></tr></thead><tbody>'+
    (accountRows||empty(6))+'</tbody></table></div></div><footer>Обновление каждые 10с, пока вкладка видима · «доступно» учитывает сбросы окон · «запас» = доступно ÷ текущее потребление · клиентам ×'+mult+'</footer>';
  shell('Система','ёмкость, спрос и рекомендации по флоту',body,pill(count(overview.subs,'подписка','подписки','подписок'),verdict.kind))}

/* ── Аудит ───────────────────────────────────────────────── */
async function audit(){const rows=(await api('/admin/audit?limit=200').catch(()=>({rows:[]}))).rows||[],bodyRows=rows.map(item=>'<tr><td>'+date(item.created_at,true)+'</td><td class="left">'+
  pill(item.action,item.action.startsWith('admin.')?'warn':'')+'</td><td class="left">'+esc(item.actor_type)+'<div class="sub mono">'+esc(item.actor_id||'system')+'</div></td><td class="left">'+
  esc(item.target_type)+' · '+esc(item.target_id)+'</td><td class="left"><div class="json" title="'+esc(JSON.stringify(item.metadata||{}))+'">'+esc(JSON.stringify(item.metadata||{}))+'</div></td></tr>').join('');
  shell('Аудит','operator/user/provider события и причины действий','<div class="tcard"><div class="tscroll"><table><thead><tr><th>время</th><th class="left">действие</th><th class="left">актор</th>'+
  '<th class="left">цель</th><th class="left">метаданные</th></tr></thead><tbody>'+(bodyRows||empty(5))+'</tbody></table></div></div><footer>Последние 200 событий. Секреты и полные API-ключи не записываются.</footer>',pill(count(rows.length,'событие','события','событий'),'ok'))}
showLoading();
refresh();
