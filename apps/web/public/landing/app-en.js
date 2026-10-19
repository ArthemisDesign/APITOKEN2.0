/* =========================================================
   apiToken landing — English prototype logic
   ========================================================= */

const CAPS = [
  { id: 'all',      name: 'All' },
  { id: 'coding',   name: 'Code' },
  { id: 'reasoning',name: 'Reasoning' },
  { id: 'agents',   name: 'Agents' },
  { id: 'image',    name: 'Images' },
  { id: 'fast',     name: 'Fast' },
  { id: 'long',     name: 'Long context' },
  { id: 'cheap',    name: 'Cheap' }
];

const PROVIDERS = [
  {
    name: 'Anthropic', note: 'Claude',
    models: [
      { name: 'Claude Opus 5',   id: 'claude-opus-5',   ctx: '200K', in: '$2.50', out: '$12.50', caps: ['coding','reasoning','agents','long'], edge: 'Strongest at complex code and long agent runs' },
      { name: 'Claude Sonnet 5', id: 'claude-sonnet-5', ctx: '200K', in: '$1.00', out: '$5.00',  caps: ['coding','reasoning','agents'], edge: 'Near-Opus quality at a 2.5× lower price', isNew: true },
      { name: 'Claude Haiku 4.5',id: 'claude-haiku-4-5',ctx: '200K', in: '$0.50', out: '$2.50',  caps: ['fast','cheap','agents'], edge: 'Fast replies for chat and high-volume work' },
      { name: 'Claude Fable 5',  id: 'claude-fable-5',  ctx: '200K', in: '$5.00', out: '$25.00', caps: ['reasoning','long'], edge: 'Deep reasoning over long documents' }
    ]
  },
  {
    name: 'OpenAI', note: 'GPT',
    models: [
      { name: 'GPT-5.6 Sol',  id: 'gpt-5-6-sol',  ctx: '400K', in: '$1.75', out: '$9.00',  caps: ['coding','reasoning','agents','long'], edge: 'OpenAI flagship: code, agents, 400K context' },
      { name: 'GPT-5.6 Terra',id: 'gpt-5-6-terra',ctx: '400K', in: '$0.90', out: '$4.50',  caps: ['coding','agents'], edge: 'Dependable workhorse for code and agents' },
      { name: 'GPT-5.6 Luna', id: 'gpt-5-6-luna', ctx: '128K', in: '$0.30', out: '$1.50',  caps: ['fast','cheap'], edge: 'Cheap and fast for simple requests' },
      { name: 'GPT-5.5',      id: 'gpt-5-5',      ctx: '256K', in: '$1.20', out: '$6.00',  caps: ['reasoning'], edge: 'Proven model for reasoning' }
    ]
  },
  {
    name: 'Google', note: 'Gemini',
    models: [
      { name: 'Gemini 3 Pro',   id: 'gemini-3-pro',   ctx: '1M',   in: '$1.10', out: '$5.50', caps: ['reasoning','long','image'], edge: '1M context plus image understanding' },
      { name: 'Gemini 3 Flash', id: 'gemini-3-flash', ctx: '1M',   in: '$0.20', out: '$1.00', caps: ['fast','cheap','long'], edge: '1M context at a light-model price' },
      { name: 'Gemini Image',   id: 'gemini-image',   ctx: '32K',  in: '$0.40', out: '$2.00', caps: ['image'], edge: 'Image generation and editing' }
    ]
  }
];

const INTEGRATIONS = [
  { id:'claude-code', name:'Claude Code', dur:'1:42',
    steps:['Get API key','Open terminal','Set endpoint','Launch Claude Code','First request'],
    lines:[['$ ','export ANTHROPIC_BASE_URL=https://router.apitoken.sale'],['$ ','export ANTHROPIC_API_KEY=sk-pool-••••'],['$ ','claude'],['','Claude Code v2.4 · connected to apiToken'],['> ','refactor src/api/client.ts'],['','✓ done · claude-opus-5 · 8.4s · $0.031']] },
  { id:'cursor', name:'Cursor', dur:'2:16',
    steps:['Open settings','Pick provider','Enter endpoint','Add key','Select model'],
    lines:[['','Settings → Models → Custom provider'],['','Base URL: https://router.apitoken.sale/v1'],['','API key: sk-pool-••••'],['','Model: claude-opus-5'],['','✓ connection verified'],['','Cursor routes all requests through apiToken']] },
  { id:'codex', name:'Codex CLI', dur:'1:58',
    steps:['Install CLI','Set endpoint','Add key','Select model','Run task'],
    lines:[['$ ','npm i -g @openai/codex'],['$ ','export OPENAI_BASE_URL=https://router.apitoken.sale/v1'],['$ ','export OPENAI_API_KEY=sk-pool-••••'],['$ ','codex "add tests to utils/date.ts"'],['','✓ 6 files changed · gpt-5-6-sol'],['','request cost: $0.024']] },
  { id:'opencode', name:'opencode', dur:'1:34',
    steps:['Install','Open config','Set provider','Paste key','Launch'],
    lines:[['$ ','opencode auth login'],['','Provider: custom (OpenAI-compatible)'],['','URL: https://router.apitoken.sale/v1'],['','Key: sk-pool-••••'],['$ ','opencode'],['','✓ catalog models available']] },
  { id:'direct', name:'Direct API', dur:'2:35',
    steps:['Create key','Pick format','Send request','Parse response','Enable streaming'],
    lines:[['','POST https://router.apitoken.sale/v1/messages'],['','x-api-key: sk-pool-••••'],['','{ "model": "claude-opus-5", ... }'],['','200 OK · 1.24s'],['','{ "content": [{ "type": "text", ... }] }'],['','streaming: "stream": true → SSE']] }
];

const PROFILES = [
  { id:'product', name:'AI product',        base:14000 },
  { id:'devteam', name:'Dev team',          base:20000, seats:true },
  { id:'support', name:'Support bot',       base:9000 },
  { id:'content', name:'Content platform',  base:12000 },
  { id:'agents',  name:'Agents',            base:26000 },
  { id:'custom',  name:'Other',             base:20000 }
];

const GUIDE_SLUGS = { start: 'claude-api-quick-setup', 'claude-code': 'claude-code-api-key', cursor: 'claude-api-key-for-cursor', models: 'claude-api-gateway', direct: 'anthropic-sdk-base-url', billing: 'how-billing-works' };
const TUTORIALS = [
  ['01','Getting started','2:04','start'],['02','Claude Code','1:42','claude-code'],['03','Cursor','2:16','cursor'],
  ['04','Switching models','1:18','models'],['05','Direct API','2:35','direct'],['06','Billing & spend','1:26','billing']
];

const FAQ = [
  ['How can you offer discounts of 50% or more?','We purchase model access in volume and redistribute it across clients. You pay the provider\'s official rate minus the discount — the request still goes to the same official API.'],
  ['Which models are available?','Models from Anthropic, OpenAI, Google and other providers. The catalog grows as new models are released, with no changes needed on your side.'],
  ['Can I use apiToken instead of my existing OpenAI API?','Yes. Just change the base URL and key — request and response formats stay the same, including streaming and tool calls.'],
  ['How fast can I connect?','About two minutes: sign up, create a key, swap the endpoint. No separate provider accounts needed.'],
  ['Are Claude Code and Cursor supported?','Yes, as well as Codex CLI, opencode, Cline and any SDK that supports Anthropic-, OpenAI- or Gemini-compatible routes.'],
  ['How are requests billed?','Prepay: top up your balance by any amount, and charges are deducted proportionally to actual token consumption across all models.'],
  ['Are there special terms for business?','Yes. Companies get volume pricing, centralized billing and a dedicated support channel — terms are discussed individually.'],
  ['What happens when new models are added?','A new model appears in the catalog and is immediately available through the same key and endpoint. No changes on your side are required.'],
  ['Can I use several models in one product?','Yes, that is the main scenario: an expensive model for complex steps, a cheap one for high-volume steps, with a shared balance and unified statistics.'],
  ['How do I get a B2B quote?','Fill out the short form in the Business terms section and we will return a calculation tailored to your volume.']
];

const $  = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const el = (tag, cls, html) => {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (html != null) n.innerHTML = html;
  return n;
};
const money = n => '$' + Math.round(n).toLocaleString('en-US').replace(/,/g, ' ');
const ALL_MODELS = PROVIDERS.flatMap(p => p.models.map(m => ({ ...m, provider: p.name })));

const hdr = $('#hdr');
addEventListener('scroll', () => hdr.classList.toggle('small', scrollY > 40), { passive: true });

const io = new IntersectionObserver(entries => {
  entries.forEach(e => { if (e.isIntersecting) { e.target.classList.add('in'); io.unobserve(e.target); } });
}, { threshold: .12, rootMargin: '0px 0px -60px' });
const observeReveals = () => $$('.reveal:not(.in)').forEach(n => io.observe(n));

(() => {
  const t = $('#ticker');
  if (!t) return;
  let i = 0;
  const ids = ALL_MODELS.map(m => m.id);
  setInterval(() => {
    i = (i + 1) % ids.length;
    t.style.opacity = 0;
    setTimeout(() => { t.textContent = ids[i]; t.style.opacity = 1; }, 180);
  }, 1900);
  t.style.transition = 'opacity .18s';
})();

(() => {
  const wrapF = $('#filters'), wrapU = $('#universe');
  if (!wrapU) return;
  let active = 'all';

  CAPS.forEach(c => {
    const b = el('button', 'chip' + (c.id === active ? ' on' : ''), c.name);
    b.dataset.cap = c.id;
    b.onclick = () => {
      active = c.id;
      $$('.chip', wrapF).forEach(x => x.classList.toggle('on', x === b));
      render();
    };
    wrapF.appendChild(b);
  });

  function card(m) {
    const n = el('article', 'mcard' + (m.isNew ? ' mcard--new' : ''));
    n.id = 'model-' + m.id;
    n.tabIndex = -1;
    n.setAttribute('aria-label', m.name);
    n.innerHTML = `
      <div class="mcard__top">
        <span class="mcard__name">${m.name}</span>
        ${m.isNew ? '<span class="mcard__badge mcard__badge--new">New</span>' : ''}
      </div>
      <span class="mcard__id">${m.id}</span>
      <div class="mcard__row"><span>input / 1M</span><b>${m.in}</b></div>
      <div class="mcard__row"><span>output / 1M</span><b>${m.out}</b></div>
      <div class="mcard__row"><span>context</span><b>${m.ctx}</b></div>
      <div class="mcard__tags">${m.caps.map(c => `<span class="tag tag--${c}">${(CAPS.find(x => x.id === c) || {}).name || c}</span>`).join('')}</div>`;
    return n;
  }

  function render() {
    wrapU.innerHTML = '';
    PROVIDERS.forEach(p => {
      // priority rule: a model flagged isNew moves to the front of its provider row
      const models = p.models
        .filter(m => active === 'all' || m.caps.includes(active))
        .sort((a, b) => (b.isNew ? 1 : 0) - (a.isNew ? 1 : 0));
      if (!models.length) return;
      const row = el('div', 'prov');
      row.innerHTML = `<div class="prov__id">
          <span class="prov__name">${p.name}</span>
          <span class="prov__meta">${p.note}, ${models.length} ${models.length === 1 ? 'model' : 'models'}</span>
        </div>`;
      const grid = el('div', 'prov__models');
      models.forEach(m => grid.appendChild(card(m)));
      row.appendChild(grid);
      wrapU.appendChild(row);
    });
  }
  render();

  // Cloud links always reveal their exact card, including after filtering.
  document.addEventListener('click', event => {
    const link = event.target instanceof Element
      ? event.target.closest('#growField a[data-model-id]') : null;
    if (!link || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    if (!ALL_MODELS.some(m => m.id === link.dataset.modelId)) return;
    event.preventDefault();
    if (active !== 'all') wrapF.querySelector('[data-cap="all"]').click();
    const target = document.getElementById('model-' + link.dataset.modelId);
    if (!target) return;
    const hash = '#' + target.id;
    if (location.hash !== hash) history.pushState(null, '', hash);
    target.focus({ preventScroll: true });
    const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
    if (window.__lenis && !reduced) {
      // Rendering a different filter changes page height; refresh before measuring.
      window.__lenis.resize();
      window.__lenis.scrollTo(target, { duration: .9 });
    } else {
      target.scrollIntoView({ block: 'start', behavior: reduced ? 'instant' : 'smooth' });
    }
  });

})();

const videoCleanup = new WeakMap();
function buildVideo(host, cfg) {
  videoCleanup.get(host)?.();
  const secs = (cfg.duration || '2:00').split(':').reduce((a, b) => a * 60 + +b, 0);
  const v = el('div', 'vid');
  v.innerHTML = `
    <div class="vid__bar"><span>${cfg.label}</span><span>${cfg.duration}</span></div>
    <div class="vid__stage" role="button" tabindex="0" aria-label="${cfg.label}" aria-pressed="false">
      ${window.apiTokenVideoCover(cfg.cover || 'start', 'en')}
      <div class="vid__poster">${(cfg.lines || []).map(l => `<span class="ln"><b>${l[0]}</b>${l[1]}</span>`).join('')}</div>
      <div class="vid__scrim"><span class="vid__play"><i></i>Watch, ${cfg.duration}</span></div>
    </div>
    <div class="vid__ctl">
      <span class="vid__state">Ready</span>
      <span class="vid__prog"><i></i></span>
      <span class="vid__time">0:00 / ${cfg.duration}</span>
    </div>`;
  host.innerHTML = '';
  host.appendChild(v);

  const stage = $('.vid__stage', v), bar = $('.vid__prog i', v), time = $('.vid__time', v), state = $('.vid__state', v);
  let t = 0, timer = null;
  videoCleanup.set(host, () => clearInterval(timer));

  const fmt = s => Math.floor(s / 60) + ':' + String(Math.floor(s % 60)).padStart(2, '0');
  const draw = () => { bar.style.width = (t / secs * 100) + '%'; time.textContent = fmt(t) + ' / ' + cfg.duration; };

  function play() {
    v.classList.add('playing'); state.textContent = 'Playing';
    stage.setAttribute('aria-pressed', 'true');
    timer = setInterval(() => {
      t += .25;
      if (t >= secs) { t = 0; pause(); state.textContent = 'Ready'; }
      draw();
    }, 250);
  }
  function pause() {
    clearInterval(timer); timer = null;
    v.classList.remove('playing');
    stage.setAttribute('aria-pressed', 'false');
    if (t > 0) state.textContent = 'Paused';
  }
  stage.onclick = () => timer ? pause() : play();
  stage.onkeydown = e => {
    if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); stage.click(); }
  };
  $('.vid__prog', v).onclick = e => {
    const r = e.currentTarget.getBoundingClientRect();
    t = (e.clientX - r.left) / r.width * secs; draw();
  };
  draw();
  return v;
}

$$('[data-video]').forEach(host => buildVideo(host, {
  label: host.dataset.label,
  duration: host.dataset.duration,
  lines: [['$ ','npm create apitoken@latest'],['','✓ account created'],['$ ','apitoken keys create --name prod'],['','sk-pool-3f9a••••  · balance $5.00'],['$ ','export ANTHROPIC_BASE_URL=https://router.apitoken.sale'],['','✓ first request completed · 1.24s · $0.0042']]
}));

const countIO = new IntersectionObserver(es => {
  es.forEach(e => {
    if (!e.isIntersecting) return;
    const n = e.target, to = +n.dataset.count, suf = n.dataset.suffix || '';
    let cur = 0; const step = Math.max(1, to / 34);
    const tick = () => {
      cur = Math.min(to, cur + step);
      n.textContent = Math.round(cur).toLocaleString('en-US') + suf;
      if (cur < to) requestAnimationFrame(tick);
    };
    tick();
    countIO.unobserve(n);
  });
}, { threshold: .5 });
// "Why apiToken" figures: pick a take with ?nums=a|b|c (preview comparison)
(() => {
  const want = new URLSearchParams(location.search).get('nums');
  const sets = $$('[data-nums]');
  if (!want || !sets.some(x => x.dataset.nums === want)) return;
  sets.forEach(x => { x.hidden = x.dataset.nums !== want; });
})();
$$('[data-count]').forEach(n => countIO.observe(n));

(() => {
  const tabs = $('#intTabs'), host = $('#intVideo'), steps = $('#intSteps');
  if (!tabs) return;
  let active = INTEGRATIONS[0];

  INTEGRATIONS.forEach(it => {
    const b = el('button', 'tab' + (it === active ? ' on' : ''), it.name);
    b.onclick = () => { active = it; $$('.tab', tabs).forEach(x => x.classList.toggle('on', x === b)); render(); };
    tabs.appendChild(b);
  });

  function render() {
    buildVideo(host, { label: 'Guide / ' + active.name, duration: active.dur, lines: active.lines, cover: active.id });
    steps.innerHTML = active.steps.map((s, i) => `<div><b>${String(i + 1).padStart(2, '0')}</b>${s}</div>`).join('');
  }
  render();
})();

(() => {
  const profWrap = $('#calcProfiles'), params = $('#calcParams'), input = $('#spendInput');
  if (!profWrap) return;
  let profile = PROFILES[1], devs = 25, usage = 2;
  const USAGE = ['Low', 'Medium', 'High', 'Very high'];

  chipGroup(profWrap, PROFILES.map(p => p.name), PROFILES.indexOf(profile), i => {
    profile = PROFILES[i];
    renderParams(); recalc(true);
  });

  /* A chip group with a gliding thumb behind the active chip — same feel as
     the language switcher. */
  function chipGroup(wrap, items, active, onPick) {
    items.forEach((item, i) => {
      const b = el('button', 'chip' + (i === active ? ' on' : ''), item);
      b.onclick = () => {
        $$('.chip', wrap).forEach(x => x.classList.toggle('on', x === b));
        moveThumb();
        onPick(i);
      };
      wrap.appendChild(b);
    });
    const thumb = el('span', 'chip-thumb');
    thumb.setAttribute('aria-hidden', 'true');
    wrap.insertBefore(thumb, wrap.firstChild);
    function moveThumb() {
      const on = wrap.querySelector('.chip.on');
      if (!on) return;
      thumb.style.width = on.offsetWidth + 'px';
      thumb.style.height = on.offsetHeight + 'px';
      thumb.style.transform = 'translate(' + on.offsetLeft + 'px,' + on.offsetTop + 'px)';
    }
    window.addEventListener('resize', moveThumb);
    requestAnimationFrame(moveThumb);
    window.addEventListener('load', moveThumb);
    setTimeout(moveThumb, 1000);
    setTimeout(moveThumb, 2000);
  }

  function renderParams() {
    params.innerHTML = '';
    if (!profile.seats) { params.hidden = true; return; }
    params.hidden = false;
    params.innerHTML = `
      <span class="calc__q">Parameters</span>
      <div class="range">
        <div class="range__top"><span class="calc__lab">Developers</span><span class="range__val" id="devVal">${devs}</span></div>
        <input type="range" id="devRange" min="1" max="200" value="${devs}">
      </div>
      <div class="calc__usage">
        <span class="calc__lab">Intensity</span>
        <div class="calc__opts" id="usageOpts"></div>
      </div>`;
    $('#devRange').oninput = e => { devs = +e.target.value; $('#devVal').textContent = devs; recalc(true); };
    chipGroup($('#usageOpts'), USAGE, usage, i => { usage = i; recalc(true); });
  }

  function estimate() {
    if (profile.seats) return devs * 260 * (0.6 + usage * 0.45);
    return profile.base * (0.7 + usage * 0.2);
  }

  function recalc(fromProfile) {
    let spend;
    if (fromProfile) { spend = estimate(); input.value = Math.round(spend).toLocaleString('en-US').replace(/,/g, ' '); }
    else spend = +input.value.replace(/[^\d]/g, '') || 0;

    const now = spend, next = spend * 0.5, save = now - next;
    $('#resNow').textContent = money(now);
    $('#resNew').textContent = money(next);
    $('#resMo').textContent  = money(save);
    $('#resYr').textContent  = money(save * 12) + ' / year';
  }

  input.oninput = () => recalc(false);
  input.onblur  = () => { input.value = (+input.value.replace(/[^\d]/g, '') || 0).toLocaleString('en-US').replace(/,/g, ' '); };
  $('#advToggle').onclick = () => { const a = $('#adv'); a.hidden = !a.hidden; };

  renderParams(); recalc(true);
})();

(() => {
  const lib = $('#lib');
  if (!lib) return;
  // each card opens the matching written guide, so the play-styled cover leads somewhere
  TUTORIALS.forEach(([num, name, dur, cover]) => {
    const n = el('a', 'tut reveal');
    n.href = '/docs/learn/' + GUIDE_SLUGS[cover];
    n.innerHTML = `
      <div class="tut__thumb">${window.apiTokenVideoCover(cover, 'en', true)}</div>
      <div class="tut__body">
        <span class="tut__num">${num}</span><span class="tut__dur">${dur}</span>
        <span class="tut__name">${name}</span>
      </div>`;
    lib.appendChild(n);
  });
})();

(() => {
  const acc = $('#acc');
  if (!acc) return;
  FAQ.forEach(([q, a], i) => {
    const item = el('div', 'acc__item');
    item.innerHTML = `<button class="acc__q">${q}<i>+</i></button><div class="acc__a"><p>${a}</p></div>`;
    const body = $('.acc__a', item);
    $('.acc__q', item).onclick = () => {
      const open = item.classList.contains('open');
      $$('.acc__item', acc).forEach(x => { x.classList.remove('open'); $('.acc__a', x).style.maxHeight = null; });
      if (!open) { item.classList.add('open'); body.style.maxHeight = body.scrollHeight + 'px'; }
    };
    acc.appendChild(item);
    if (i === 0) $('.acc__q', item).click();
  });
})();

observeReveals();


/* theme toggle — light (warm paper) ↔ dark (deep ink), persisted.
   Writes both the landing key (apitoken-theme) and the dashboard
   key (theme:v1) so the theme follows the user between the static
   landing and the Next.js dashboard; reads fall back across the
   three keys via the inline bootstrap in each page's <head>. */
(function(){
  const KEY = 'apitoken-theme';
  const DASH_KEY = 'theme:v1';
  const root = document.documentElement;
  const btn = document.getElementById('themeTgl');
  const apply = (t) => {
    if (t === 'dark') root.dataset.theme = 'dark'; else delete root.dataset.theme;
    if (btn) {
      const ru = document.documentElement.lang === 'ru';
      const label = t === 'dark' ? (ru ? 'Включить светлую тему' : 'Switch to light theme') : (ru ? 'Включить тёмную тему' : 'Switch to dark theme');
      btn.setAttribute('aria-label', label);
      btn.title = label;
    }
  };
  let saved = 'light';
  try { saved = (localStorage.getItem(DASH_KEY) || localStorage.getItem(KEY)) === 'dark' ? 'dark' : 'light'; } catch(e) {}
  apply(saved);
  if (btn) btn.addEventListener('click', () => {
    saved = root.dataset.theme === 'dark' ? 'light' : 'dark';
    try { localStorage.setItem(KEY, saved); localStorage.setItem(DASH_KEY, saved); } catch(e) {}
    apply(saved);
  });
})();

/* language switcher: glide the thumb to the hovered/target language on click,
   then let the plain link navigate — no page fade, no white flash */
(function(){
  document.querySelectorAll('.lang').forEach(function(wrap){
    wrap.querySelectorAll('.lang__link').forEach(function(a){
      a.addEventListener('click', function(){
        const target = /en\.html/.test(a.getAttribute('href') || '') ? 'en' : 'ru';
        wrap.dataset.active = target;
        /* persist the explicit choice in the dashboard's lang key so the
           Next.js app picks it up and the RU→EN default redirect stays off */
        try { localStorage.setItem('lang:v1', target); } catch(e1) {}
        wrap.querySelectorAll('.lang__link').forEach(function(x){ x.classList.toggle('is-active', x === a); });
        /* remember how far down the page we are (as a fraction) so the other
           language opens at the same relative position instead of jumping to top */
        try {
          const max = document.documentElement.scrollHeight - window.innerHeight;
          if(max > 0) sessionStorage.setItem('apitoken-scroll-frac', String(window.scrollY / max));
        } catch(e2) {}
      });
    });
  });
})();
