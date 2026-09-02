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
      { name: 'Claude Opus 5',   id: 'claude-opus-5',   ctx: '200K', in: '$2.50', out: '$12.50', caps: ['coding','reasoning','agents','long'], badge: 'Best quality' },
      { name: 'Claude Sonnet 5', id: 'claude-sonnet-5', ctx: '200K', in: '$1.00', out: '$5.00',  caps: ['coding','reasoning','agents'],        badge: 'New', badgeKind: 'new' },
      { name: 'Claude Haiku 4.5',id: 'claude-haiku-4-5',ctx: '200K', in: '$0.50', out: '$2.50',  caps: ['fast','cheap','agents'] },
      { name: 'Claude Fable 5',  id: 'claude-fable-5',  ctx: '200K', in: '$5.00', out: '$25.00', caps: ['reasoning','long'] }
    ]
  },
  {
    name: 'OpenAI', note: 'GPT',
    models: [
      { name: 'GPT-5.6 Sol',  id: 'gpt-5-6-sol',  ctx: '400K', in: '$1.75', out: '$9.00',  caps: ['coding','reasoning','agents','long'] },
      { name: 'GPT-5.6 Terra',id: 'gpt-5-6-terra',ctx: '400K', in: '$0.90', out: '$4.50',  caps: ['coding','agents'] },
      { name: 'GPT-5.6 Luna', id: 'gpt-5-6-luna', ctx: '128K', in: '$0.30', out: '$1.50',  caps: ['fast','cheap'] },
      { name: 'GPT-5.5',      id: 'gpt-5-5',      ctx: '256K', in: '$1.20', out: '$6.00',  caps: ['reasoning'] }
    ]
  },
  {
    name: 'Google', note: 'Gemini',
    models: [
      { name: 'Gemini 3 Pro',   id: 'gemini-3-pro',   ctx: '1M',   in: '$1.10', out: '$5.50', caps: ['reasoning','long','image'] },
      { name: 'Gemini 3 Flash', id: 'gemini-3-flash', ctx: '1M',   in: '$0.20', out: '$1.00', caps: ['fast','cheap','long'] },
      { name: 'Gemini Image',   id: 'gemini-image',   ctx: '32K',  in: '$0.40', out: '$2.00', caps: ['image'] }
    ]
  },
  {
    name: 'Kimi', note: 'Moonshot',
    models: [
      { name: 'Kimi K2.5',      id: 'kimi-k2-5',      ctx: '256K', in: '$0.28', out: '$1.40', caps: ['cheap','coding','long'], badge: 'Best price' },
      { name: 'Kimi K2 Turbo',  id: 'kimi-k2-turbo',  ctx: '128K', in: '$0.15', out: '$0.75', caps: ['fast','cheap'] }
    ]
  },
  {
    name: 'Mistral', note: 'Open weights',
    models: [
      { name: 'Mistral Large 3', id: 'mistral-large-3', ctx: '128K', in: '$0.60', out: '$3.00', caps: ['coding','agents'] },
      { name: 'Mistral Small',   id: 'mistral-small',   ctx: '128K', in: '$0.10', out: '$0.50', caps: ['fast','cheap'] }
    ]
  }
];

const TASKS = [
  { id: 'coding',  name: 'Code',              picks: ['claude-opus-5','gpt-5-6-sol','claude-sonnet-5','kimi-k2-5'] },
  { id: 'agents',  name: 'AI agent',          picks: ['claude-opus-5','gpt-5-6-terra','claude-haiku-4-5','mistral-large-3'] },
  { id: 'support', name: 'Support',           picks: ['claude-sonnet-5','gpt-5-6-luna','claude-haiku-4-5','kimi-k2-turbo'] },
  { id: 'content', name: 'Content',           picks: ['claude-fable-5','gpt-5-6-sol','gemini-3-pro','kimi-k2-5'] },
  { id: 'image',   name: 'Images',            picks: ['gemini-image','gemini-3-pro','gpt-5-6-terra','mistral-small'] },
  { id: 'volume',  name: 'High volumes',      picks: ['gemini-3-flash','kimi-k2-turbo','claude-haiku-4-5','mistral-small'] },
  { id: 'reason',  name: 'Reasoning',         picks: ['claude-opus-5','gpt-5-5','gemini-3-pro','claude-sonnet-5'] }
];

const ROLES = [
  { role: 'Maximum quality', why: 'The hardest tasks where result accuracy matters most.' },
  { role: 'Balanced',        why: 'Your daily workhorse: near-top quality at a lower price.' },
  { role: 'Fast',            why: 'Low latency for interactive scenarios.' },
  { role: 'Economical',      why: 'High-volume request streams at minimum cost.' }
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

const SWITCH_MODELS = [
  { id:'claude-opus-5',  prov:'Anthropic', lat:'1.2s', cost:'$0.0042', ans:'The first approach wins on cost, the second on latency. From 1M requests onwards the gap becomes decisive…' },
  { id:'gpt-5-6-sol',    prov:'OpenAI',    lat:'0.9s', cost:'$0.0038', ans:'Both options are viable. The key difference is long-haul maintenance cost…' },
  { id:'gemini-3-pro',   prov:'Google',    lat:'0.7s', cost:'$0.0026', ans:'Comparison across three axes: price, speed, response quality. The second approach is preferable under high load…' },
  { id:'kimi-k2-5',      prov:'Kimi',      lat:'0.6s', cost:'$0.0009', ans:'The cost difference is almost fivefold. With comparable quality on this task the choice is clear…' }
];

const PROFILES = [
  { id:'product', name:'AI product',        base:14000 },
  { id:'devteam', name:'Dev team',          base:20000, seats:true },
  { id:'support', name:'Support bot',       base:9000 },
  { id:'content', name:'Content platform',  base:12000 },
  { id:'agents',  name:'Agents',            base:26000 },
  { id:'custom',  name:'Other',             base:20000 }
];

const TUTORIALS = [
  ['01','Getting started','2:04'],['02','Claude Code','1:42'],['03','Cursor','2:16'],
  ['04','Switching models','1:18'],['05','Direct API','2:35'],['06','Billing & spend','1:26']
];

const FAQ = [
  ['Where does the up to 50% discount come from?','We purchase model access in volume and redistribute it across clients. You pay the provider\'s official rate minus the discount — the request still goes to the same official API.'],
  ['Which models are available?','Models from Anthropic, OpenAI, Google, Kimi, Mistral and other providers. The catalog grows as new models are released, with no changes needed on your side.'],
  ['Can I use apiToken instead of my existing OpenAI API?','Yes. Just change the base URL and key — request and response formats stay the same, including streaming and tool calls.'],
  ['How fast can I connect?','About two minutes: sign up, create a key, swap the endpoint. No separate provider accounts needed.'],
  ['Are Claude Code and Cursor supported?','Yes, as well as Codex CLI, opencode, Cline and any SDK that supports Anthropic-, OpenAI- or Gemini-compatible routes.'],
  ['How are requests billed?','Prepay: top up your balance by any amount, and charges are deducted proportionally to actual token consumption across all models.'],
  ['Are there special terms for business?','Yes. Companies get volume pricing, centralized billing and a dedicated support channel — terms are discussed individually.'],
  ['What happens when new models are added?','A new model appears in the catalog and is immediately available through the same key and endpoint. No changes on your side are required.'],
  ['Can I use several models in one product?','Yes, that is the main scenario: an expensive model for complex steps, a cheap one for high-volume steps, with a shared balance and unified statistics.'],
  ['How do I get a B2B quote?','Fill out the short form in the Business terms section and we will return a calculation tailored to your volume.']
];

const GROW_ITEMS = ['GPT','Claude','Gemini','Kimi','Mistral','Llama','Qwen','DeepSeek','+ new provider','+ new model','+ ...'];

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
const byId = id => ALL_MODELS.find(m => m.id === id);

const hdr = $('#hdr');
addEventListener('scroll', () => hdr.classList.toggle('small', scrollY > 40), { passive: true });
$('#burger')?.addEventListener('click', () => $('.nav').classList.toggle('open'));

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
    b.onclick = () => {
      active = c.id;
      $$('.chip', wrapF).forEach(x => x.classList.toggle('on', x === b));
      render();
    };
    wrapF.appendChild(b);
  });

  function card(m) {
    const n = el('article', 'mcard');
    n.innerHTML = `
      <div class="mcard__top">
        <span class="mcard__name">${m.name}</span>
      </div>
      <span class="mcard__id">${m.id}</span>
      <div class="mcard__row"><span>input / 1M</span><b>${m.in}</b></div>
      <div class="mcard__row"><span>output / 1M</span><b>${m.out}</b></div>
      <div class="mcard__row"><span>context</span><b>${m.ctx}</b></div>
      <div class="mcard__tags">${m.caps.map(c => `<span class="tag">${(CAPS.find(x => x.id === c) || {}).name || c}</span>`).join('')}</div>`;
    return n;
  }

  function render() {
    wrapU.innerHTML = '';
    PROVIDERS.forEach(p => {
      const models = p.models.filter(m => active === 'all' || m.caps.includes(active));
      if (!models.length) return;
      const row = el('div', 'prov');
      row.innerHTML = `<div class="prov__id">
          <span class="prov__name">${p.name}</span>
          <span class="prov__meta">${p.note} · ${models.length} ${models.length === 1 ? 'model' : 'models'}</span>
        </div>`;
      const grid = el('div', 'prov__models');
      models.forEach(m => grid.appendChild(card(m)));
      row.appendChild(grid);
      wrapU.appendChild(row);
    });
  }
  render();
})();

function buildVideo(host, cfg) {
  const secs = (cfg.duration || '2:00').split(':').reduce((a, b) => a * 60 + +b, 0);
  const v = el('div', 'vid');
  v.innerHTML = `
    <div class="vid__bar"><span>${cfg.label}</span><span>${cfg.duration}</span></div>
    <div class="vid__stage">
      <div class="vid__poster">${(cfg.lines || []).map(l => `<span class="ln"><b>${l[0]}</b>${l[1]}</span>`).join('')}</div>
      <div class="vid__scrim"><span class="vid__play"><i></i>Watch · ${cfg.duration}</span></div>
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

  const fmt = s => Math.floor(s / 60) + ':' + String(Math.floor(s % 60)).padStart(2, '0');
  const draw = () => { bar.style.width = (t / secs * 100) + '%'; time.textContent = fmt(t) + ' / ' + cfg.duration; };

  function play() {
    v.classList.add('playing'); state.textContent = 'Playing';
    timer = setInterval(() => {
      t += .25;
      if (t >= secs) { t = 0; pause(); state.textContent = 'Ready'; }
      draw();
    }, 250);
  }
  function pause() {
    clearInterval(timer); timer = null;
    v.classList.remove('playing');
    if (t > 0) state.textContent = 'Paused';
  }
  stage.onclick = () => timer ? pause() : play();
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
      n.textContent = Math.round(cur) + suf;
      if (cur < to) requestAnimationFrame(tick);
    };
    tick();
    countIO.unobserve(n);
  });
}, { threshold: .5 });
$$('[data-count]').forEach(n => countIO.observe(n));

(() => {
  const opts = $('#askOpts'), out = $('#picks');
  if (!opts) return;
  let active = TASKS[0];

  TASKS.forEach(t => {
    const b = el('button', 'chip' + (t === active ? ' on' : ''), t.name);
    b.onclick = () => { active = t; $$('.chip', opts).forEach(x => x.classList.toggle('on', x === b)); render(); };
    opts.appendChild(b);
  });

  function render() {
    out.innerHTML = '';
    active.picks.forEach((id, i) => {
      const m = byId(id); if (!m) return;
      const r = ROLES[i] || ROLES[ROLES.length - 1];
      const n = el('article', 'pick');
      n.innerHTML = `
        <span class="pick__role">${r.role}</span>
        <span class="pick__name">${m.name}</span>
        <span class="pick__prov">${m.provider}</span>
        <p class="pick__why">${r.why}</p>
        <div class="pick__specs">
          <div><span>input / 1M</span><b>${m.in}</b></div>
          <div><span>output / 1M</span><b>${m.out}</b></div>
          <div><span>context</span><b>${m.ctx}</b></div>
        </div>
        <span class="pick__cta">Use model →</span>`;
      out.appendChild(n);
    });
    observeReveals();
  }
  render();
})();

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
    buildVideo(host, { label: 'Guide / ' + active.name, duration: active.dur, lines: active.lines });
    steps.innerHTML = active.steps.map((s, i) => `<div><b>${String(i + 1).padStart(2, '0')}</b>${s}</div>`).join('');
  }
  render();
})();

(() => {
  const pick = $('#switchPick');
  if (!pick) return;
  let idx = 0, auto;

  const apply = i => {
    idx = i;
    const m = SWITCH_MODELS[i];
    $$('.chip', pick).forEach((x, j) => x.classList.toggle('on', j === i));
    $('#swModel').textContent = `"${m.id}"`;
    $('#swProvider').textContent = m.prov;
    $('#swLatency').textContent = m.lat;
    $('#swCost').textContent = m.cost;
    const ans = $('#swAnswer');
    ans.style.opacity = 0;
    setTimeout(() => { ans.textContent = m.ans; ans.style.opacity = 1; }, 200);
  };

  SWITCH_MODELS.forEach((m, i) => {
    const b = el('button', 'chip' + (i === 0 ? ' on' : ''), m.id);
    b.onclick = () => { clearInterval(auto); apply(i); };
    pick.appendChild(b);
  });
  $('#swAnswer').style.transition = 'opacity .2s';
  apply(0);
  auto = setInterval(() => apply((idx + 1) % SWITCH_MODELS.length), 3600);
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

$('#b2bForm')?.addEventListener('submit', e => {
  e.preventDefault();
  $('#b2bOk').hidden = false;
  e.target.querySelector('button').textContent = 'Sent';
});

(() => {
  const lib = $('#lib');
  if (!lib) return;
  TUTORIALS.forEach(([num, name, dur]) => {
    const n = el('article', 'tut reveal');
    n.innerHTML = `
      <div class="tut__thumb"><span class="tut__grid"></span></div>
      <div class="tut__body">
        <span class="tut__num">${num}</span><span class="tut__dur">${dur}</span>
        <span class="tut__name">${name}</span>
      </div>`;
    lib.appendChild(n);
  });
})();

(() => {
  const field = $('#growField');
  if (!field) return;
  const seen = new Set();
  const place = (txt, i, isNew) => {
    const s = el('span', isNew ? 'new' : '', txt);
    let x, y, tries = 0;
    do {
      x = 6 + Math.random() * 76;
      y = 10 + Math.random() * 72;
      tries++;
    } while (tries < 20 && [...seen].some(([sx, sy]) => Math.abs(sx - x) < 13 && Math.abs(sy - y) < 12));
    seen.add([x, y]);
    s.style.left = x + '%';
    s.style.top = y + '%';
    s.style.animationDelay = (i * .16) + 's';
    field.appendChild(s);
  };
  const start = () => GROW_ITEMS.forEach((t, i) => place(t, i, t.startsWith('+')));
  new IntersectionObserver((es, ob) => {
    es.forEach(e => { if (e.isIntersecting) { start(); ob.disconnect(); } });
  }, { threshold: .3 }).observe(field);
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


/* theme toggle — light (warm paper) ↔ dark (deep ink), persisted. */
(function(){
  const KEY = 'apitoken-theme';
  const root = document.documentElement;
  const btn = document.getElementById('themeTgl');
  const apply = (t) => {
    if (t === 'dark') root.dataset.theme = 'dark'; else delete root.dataset.theme;
    if (btn) btn.textContent = t === 'dark' ? '☾' : '☀';
  };
  let saved = 'light';
  try { saved = localStorage.getItem(KEY) === 'dark' ? 'dark' : 'light'; } catch(e) {}
  apply(saved);
  if (btn) btn.addEventListener('click', () => {
    saved = root.dataset.theme === 'dark' ? 'light' : 'dark';
    try { localStorage.setItem(KEY, saved); } catch(e) {}
    apply(saved);
  });
})();

/* language switcher: glide the thumb to the hovered/target language on click,
   then let the plain link navigate — no page fade, no white flash */
(function(){
  document.querySelectorAll('.lang').forEach(function(wrap){
    wrap.querySelectorAll('.lang__link').forEach(function(a){
      a.addEventListener('click', function(){
        wrap.dataset.active = /en\.html/.test(a.getAttribute('href') || '') ? 'en' : 'ru';
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
