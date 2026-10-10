/* =========================================================
   apiToken landing — prototype logic
   Everything model-related is data-driven: add an entry to
   PROVIDERS and the layout absorbs it with no CSS change. A model
   flagged isNew: true goes first in its row with a soft highlight.
   ========================================================= */

/* ---------------------------------------------------------
   DATA
   --------------------------------------------------------- */
const CAPS = [
  { id: 'all',      name: 'Все' },
  { id: 'coding',   name: 'Код' },
  { id: 'reasoning',name: 'Рассуждение' },
  { id: 'agents',   name: 'Агенты' },
  { id: 'image',    name: 'Изображения' },
  { id: 'fast',     name: 'Быстрые' },
  { id: 'long',     name: 'Длинный контекст' },
  { id: 'cheap',    name: 'Дешёвые' }
];

const PROVIDERS = [
  {
    name: 'Anthropic', note: 'Claude',
    models: [
      { name: 'Claude Opus 5',   id: 'claude-opus-5',   ctx: '200K', in: '$2.50', out: '$12.50', caps: ['coding','reasoning','agents','long'], edge: 'Сильнейшая в сложном коде и длинных агентных задачах' },
      { name: 'Claude Sonnet 5', id: 'claude-sonnet-5', ctx: '200K', in: '$1.00', out: '$5.00',  caps: ['coding','reasoning','agents'], edge: 'Качество близко к Opus, цена в 2,5 раза ниже', isNew: true },
      { name: 'Claude Haiku 4.5',id: 'claude-haiku-4-5',ctx: '200K', in: '$0.50', out: '$2.50',  caps: ['fast','cheap','agents'], edge: 'Быстрые ответы для чатов и массовых задач' },
      { name: 'Claude Fable 5',  id: 'claude-fable-5',  ctx: '200K', in: '$5.00', out: '$25.00', caps: ['reasoning','long'], edge: 'Глубокие рассуждения над длинными текстами' }
    ]
  },
  {
    name: 'OpenAI', note: 'GPT',
    models: [
      { name: 'GPT-5.6 Sol',  id: 'gpt-5-6-sol',  ctx: '400K', in: '$1.75', out: '$9.00',  caps: ['coding','reasoning','agents','long'], edge: 'Флагман OpenAI: код, агенты и контекст 400K' },
      { name: 'GPT-5.6 Terra',id: 'gpt-5-6-terra',ctx: '400K', in: '$0.90', out: '$4.50',  caps: ['coding','agents'], edge: 'Надёжная рабочая модель для кода и агентов' },
      { name: 'GPT-5.6 Luna', id: 'gpt-5-6-luna', ctx: '128K', in: '$0.30', out: '$1.50',  caps: ['fast','cheap'], edge: 'Дешёвая и быстрая для простых запросов' },
      { name: 'GPT-5.5',      id: 'gpt-5-5',      ctx: '256K', in: '$1.20', out: '$6.00',  caps: ['reasoning'], edge: 'Проверенная модель для рассуждений' }
    ]
  },
  {
    name: 'Google', note: 'Gemini',
    models: [
      { name: 'Gemini 3 Pro',   id: 'gemini-3-pro',   ctx: '1M',   in: '$1.10', out: '$5.50', caps: ['reasoning','long','image'], edge: 'Контекст 1M и понимание изображений' },
      { name: 'Gemini 3 Flash', id: 'gemini-3-flash', ctx: '1M',   in: '$0.20', out: '$1.00', caps: ['fast','cheap','long'], edge: 'Контекст 1M по цене лёгкой модели' },
      { name: 'Gemini Image',   id: 'gemini-image',   ctx: '32K',  in: '$0.40', out: '$2.00', caps: ['image'], edge: 'Генерация и редактирование изображений' }
    ]
  },
  {
    name: 'Kimi', note: 'Moonshot',
    models: [
      { name: 'Kimi K2.5',      id: 'kimi-k2-5',      ctx: '256K', in: '$0.28', out: '$1.40', caps: ['cheap','coding','long'], edge: 'Сильный код по минимальной цене' },
      { name: 'Kimi K2 Turbo',  id: 'kimi-k2-turbo',  ctx: '128K', in: '$0.15', out: '$0.75', caps: ['fast','cheap'], edge: 'Самая низкая цена в каталоге' }
    ]
  }
];

const INTEGRATIONS = [
  { id:'claude-code', name:'Claude Code', dur:'1:42',
    steps:['Получить API-ключ','Открыть терминал','Настроить endpoint','Запустить Claude Code','Первый запрос'],
    lines:[['$ ','export ANTHROPIC_BASE_URL=https://router.apitoken.sale'],['$ ','export ANTHROPIC_API_KEY=sk-pool-••••'],['$ ','claude'],['','Claude Code v2.4 · подключено к apiToken'],['> ','отрефактори src/api/client.ts'],['','✓ готово · claude-opus-5 · 8.4s · $0.031']] },
  { id:'cursor', name:'Cursor', dur:'2:16',
    steps:['Открыть настройки','Выбрать провайдера','Указать endpoint','Добавить ключ','Выбрать модель'],
    lines:[['','Settings → Models → Custom provider'],['','Base URL: https://router.apitoken.sale/v1'],['','API key: sk-pool-••••'],['','Model: claude-opus-5'],['','✓ соединение проверено'],['','Cursor использует apiToken для всех запросов']] },
  { id:'codex', name:'Codex CLI', dur:'1:58',
    steps:['Установить CLI','Прописать endpoint','Добавить ключ','Выбрать модель','Запустить задачу'],
    lines:[['$ ','npm i -g @openai/codex'],['$ ','export OPENAI_BASE_URL=https://router.apitoken.sale/v1'],['$ ','export OPENAI_API_KEY=sk-pool-••••'],['$ ','codex "добавь тесты к utils/date.ts"'],['','✓ 6 файлов изменено · gpt-5-6-sol'],['','стоимость запроса: $0.024']] },
  { id:'opencode', name:'opencode', dur:'1:34',
    steps:['Установить','Открыть конфиг','Указать провайдера','Вставить ключ','Запуск'],
    lines:[['$ ','opencode auth login'],['','Provider: custom (OpenAI-compatible)'],['','URL: https://router.apitoken.sale/v1'],['','Key: sk-pool-••••'],['$ ','opencode'],['','✓ модели каталога доступны']] },
  { id:'direct', name:'Прямой API', dur:'2:35',
    steps:['Создать ключ','Выбрать формат','Отправить запрос','Разобрать ответ','Включить стриминг'],
    lines:[['','POST https://router.apitoken.sale/v1/messages'],['','x-api-key: sk-pool-••••'],['','{ "model": "claude-opus-5", ... }'],['','200 OK · 1.24s'],['','{ "content": [{ "type": "text", ... }] }'],['','стриминг: "stream": true → SSE']] }
];

const PROFILES = [
  { id:'product', name:'AI-продукт',        base:14000 },
  { id:'devteam', name:'Команда разработки',base:20000, seats:true },
  { id:'support', name:'Бот поддержки',     base:9000 },
  { id:'content', name:'Контент-платформа', base:12000 },
  { id:'agents',  name:'Агенты',            base:26000 },
  { id:'custom',  name:'Другое',            base:20000 }
];

const GUIDE_SLUGS = { start: 'claude-api-quick-setup', 'claude-code': 'claude-code-api-key', cursor: 'claude-api-key-for-cursor', models: 'claude-api-gateway', direct: 'anthropic-sdk-base-url', billing: 'how-billing-works' };
const TUTORIALS = [
  ['01','Начало работы','2:04','start'],['02','Claude Code','1:42','claude-code'],['03','Cursor','2:16','cursor'],
  ['04','Переключение моделей','1:18','models'],['05','Прямой API','2:35','direct'],['06','Биллинг и расходы','1:26','billing']
];

const FAQ = [
  ['Откуда берётся скидка до 50%?','Мы закупаем доступ к моделям объёмом и перераспределяем его между клиентами. Вы платите по официальным ставкам провайдера за вычетом скидки — сам запрос уходит в тот же официальный API.'],
  ['Какие модели доступны?','Модели Anthropic, OpenAI, Google, Kimi и других провайдеров. Каталог пополняется по мере выхода новых моделей — менять интеграцию для этого не нужно.'],
  ['Можно ли использовать apiToken вместо существующего OpenAI API?','Да. Достаточно поменять base URL и ключ — формат запросов и ответов остаётся прежним, включая стриминг и вызов инструментов.'],
  ['Как быстро можно подключиться?','Около двух минут: регистрация, создание ключа, замена endpoint. Отдельные аккаунты у провайдеров не нужны.'],
  ['Поддерживаются ли Claude Code и Cursor?','Да, как и Codex CLI, opencode, Cline и любые SDK, работающие с Anthropic-, OpenAI- или Gemini-совместимыми маршрутами.'],
  ['Как оплачиваются запросы?','Предоплата: пополняете баланс на любую сумму, списание идёт пропорционально фактическому потреблению токенов по всем моделям сразу.'],
  ['Есть ли специальные условия для бизнеса?','Да. Для компаний действуют цены под объём, централизованный биллинг и отдельный канал поддержки — условия обсуждаются индивидуально.'],
  ['Что происходит при добавлении новых моделей?','Новая модель появляется в каталоге и сразу доступна по тому же ключу и endpoint. Изменения на вашей стороне не требуются.'],
  ['Можно ли использовать несколько моделей в одном продукте?','Да, это основной сценарий: дорогая модель на сложных шагах, дешёвая — на массовых, с общим балансом и единой статистикой.'],
  ['Как получить B2B предложение?','Заполните короткую форму в разделе «Бизнес-условия» — мы вернёмся с расчётом под ваш объём.']
];

const GROW_ITEMS = ['GPT','Claude','Gemini','Kimi','Llama','Qwen','DeepSeek','+ новый провайдер','+ новая модель','+ ...'];

/* ---------------------------------------------------------
   HELPERS
   --------------------------------------------------------- */
const $  = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const el = (tag, cls, html) => {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (html != null) n.innerHTML = html;
  return n;
};
const money = n => '$' + Math.round(n).toLocaleString('ru-RU').replace(/,/g, ' ');
const ALL_MODELS = PROVIDERS.flatMap(p => p.models.map(m => ({ ...m, provider: p.name })));

/* ---------------------------------------------------------
   HEADER
   --------------------------------------------------------- */
const hdr = $('#hdr');
addEventListener('scroll', () => hdr.classList.toggle('small', scrollY > 40), { passive: true });

/* ---------------------------------------------------------
   THEME TOGGLE — light (warm paper) ↔ dark (deep ink), persisted.
   Writes both the landing key (apitoken-theme) and the dashboard
   key (theme:v1) so the theme follows the user between the static
   landing and the Next.js dashboard; reads fall back across the
   three keys via the inline bootstrap in each page's <head>.
   --------------------------------------------------------- */
(() => {
  const KEY = 'apitoken-theme';
  const DASH_KEY = 'theme:v1';
  const root = document.documentElement;
  const btn = $('#themeTgl');
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
  try { saved = (localStorage.getItem(DASH_KEY) || localStorage.getItem(KEY)) === 'dark' ? 'dark' : 'light'; } catch {}
  apply(saved);
  btn?.addEventListener('click', () => {
    saved = root.dataset.theme === 'dark' ? 'light' : 'dark';
    try { localStorage.setItem(KEY, saved); localStorage.setItem(DASH_KEY, saved); } catch {}
    apply(saved);
  });
})();

/* ---------------------------------------------------------
   REVEAL ON SCROLL
   --------------------------------------------------------- */
const io = new IntersectionObserver(entries => {
  entries.forEach(e => { if (e.isIntersecting) { e.target.classList.add('in'); io.unobserve(e.target); } });
}, { threshold: .12, rootMargin: '0px 0px -60px' });
const observeReveals = () => $$('.reveal:not(.in)').forEach(n => io.observe(n));

/* ---------------------------------------------------------
   HERO · model ticker
   --------------------------------------------------------- */
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

/* ---------------------------------------------------------
   01 · MODEL UNIVERSE
   --------------------------------------------------------- */
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
    const n = el('article', 'mcard' + (m.isNew ? ' mcard--new' : ''));
    n.innerHTML = `
      <div class="mcard__top">
        <span class="mcard__name">${m.name}</span>
        ${m.isNew ? '<span class="mcard__badge mcard__badge--new">Новинка</span>' : ''}
      </div>
      <span class="mcard__id">${m.id}</span>
      <p class="mcard__edge">${m.edge}</p>
      <div class="mcard__row"><span>вход / 1M</span><b>${m.in}</b></div>
      <div class="mcard__row"><span>выход / 1M</span><b>${m.out}</b></div>
      <div class="mcard__row"><span>контекст</span><b>${m.ctx}</b></div>
      <div class="mcard__tags">${m.caps.map(c => `<span class="tag">${(CAPS.find(x => x.id === c) || {}).name || c}</span>`).join('')}</div>`;
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
          <span class="prov__meta">${p.note} · ${models.length} ${models.length === 1 ? 'модель' : 'моделей'}</span>
        </div>`;
      const grid = el('div', 'prov__models');
      models.forEach(m => grid.appendChild(card(m)));
      row.appendChild(grid);
      wrapU.appendChild(row);
    });
  }
  render();
})();

/* ---------------------------------------------------------
   VIDEO COMPONENT  (default / hover / playing / paused)
   --------------------------------------------------------- */
const videoCleanup = new WeakMap();
function buildVideo(host, cfg) {
  videoCleanup.get(host)?.();
  const secs = (cfg.duration || '2:00').split(':').reduce((a, b) => a * 60 + +b, 0);
  const v = el('div', 'vid');
  v.innerHTML = `
    <div class="vid__bar"><span>${cfg.label}</span><span>${cfg.duration}</span></div>
    <div class="vid__stage" role="button" tabindex="0" aria-label="${cfg.label}" aria-pressed="false">
      ${window.apiTokenVideoCover(cfg.cover || 'start', 'ru')}
      <div class="vid__poster">${(cfg.lines || []).map(l => `<span class="ln"><b>${l[0]}</b>${l[1]}</span>`).join('')}</div>
      <div class="vid__scrim"><span class="vid__play"><i></i>Смотреть · ${cfg.duration}</span></div>
    </div>
    <div class="vid__ctl">
      <span class="vid__state">Готово</span>
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
    v.classList.add('playing'); state.textContent = 'Играет';
    stage.setAttribute('aria-pressed', 'true');
    timer = setInterval(() => {
      t += .25;
      if (t >= secs) { t = 0; pause(); state.textContent = 'Готово'; }
      draw();
    }, 250);
  }
  function pause() {
    clearInterval(timer); timer = null;
    v.classList.remove('playing');
    stage.setAttribute('aria-pressed', 'false');
    if (t > 0) state.textContent = 'Пауза';
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
  lines: [['$ ','npm create apitoken@latest'],['','✓ аккаунт создан'],['$ ','apitoken keys create --name prod'],['','sk-pool-3f9a••••  · баланс $5.00'],['$ ','export ANTHROPIC_BASE_URL=https://router.apitoken.sale'],['','✓ первый запрос выполнен · 1.24s · $0.0042']]
}));

/* ---------------------------------------------------------
   04 · COUNT-UP
   --------------------------------------------------------- */
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

/* ---------------------------------------------------------
   06 · INTEGRATIONS
   --------------------------------------------------------- */
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
    buildVideo(host, { label: 'Инструкция / ' + active.name, duration: active.dur, lines: active.lines, cover: active.id });
    steps.innerHTML = active.steps.map((s, i) => `<div><b>${String(i + 1).padStart(2, '0')}</b>${s}</div>`).join('');
  }
  render();
})();

/* ---------------------------------------------------------
   09 · CALCULATOR
   --------------------------------------------------------- */
(() => {
  const profWrap = $('#calcProfiles'), params = $('#calcParams'), input = $('#spendInput');
  if (!profWrap) return;
  let profile = PROFILES[1], devs = 25, usage = 2;
  const USAGE = ['Низкая', 'Средняя', 'Высокая', 'Очень высокая'];

  /* A chip group with a gliding thumb behind the active chip — same feel as
     the language switcher. The GSAP scroll-reveal shifts chips by y:30, so
     the thumb re-measures once fonts are ready and again after the reveal
     would have settled, and never latches onto a mid-animation offset. */
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
    if (document.fonts && document.fonts.ready) document.fonts.ready.then(moveThumb);
    setTimeout(moveThumb, 1000);
    setTimeout(moveThumb, 2000);
  }

  chipGroup(profWrap, PROFILES.map(p => p.name), PROFILES.indexOf(profile), i => {
    profile = PROFILES[i];
    renderParams(); recalc(true);
  });

  function renderParams() {
    params.innerHTML = '';
    if (!profile.seats) { params.hidden = true; return; }
    params.hidden = false;
    params.innerHTML = `
      <span class="calc__q">Параметры</span>
      <div class="range">
        <div class="range__top"><span class="calc__lab">Разработчиков</span><span class="range__val" id="devVal">${devs}</span></div>
        <input type="range" id="devRange" min="1" max="200" value="${devs}">
      </div>
      <div class="calc__usage">
        <span class="calc__lab">Интенсивность</span>
        <div class="calc__opts" id="usageOpts"></div>
      </div>`;
    $('#devRange').oninput = e => { devs = +e.target.value; $('#devVal').textContent = devs; recalc(true); };
    chipGroup($('#usageOpts'), USAGE, usage, i => { usage = i; recalc(true); });
  }

  function estimate() {
    if (profile.seats) return devs * 260 * (0.6 + usage * 0.45);
    return profile.base * (0.7 + usage * 0.2);
  }

  /* smooth the savings numbers so slider drags feel fluid, not steppy */
  let rafSave = null, shownSave = null;
  function fmtMoney(n) { return money(n); }
  function tweenSave(to) {
    if (shownSave === null) shownSave = to;
    const from = shownSave;
    shownSave = to;
    if (Math.round(from) === Math.round(to)) { $('#resMo').textContent = fmtMoney(to); $('#resYr').textContent = fmtMoney(to * 12) + ' в год'; return; }
    cancelAnimationFrame(rafSave);
    const t0 = performance.now(), dur = 340;
    const step = (t) => {
      const k = Math.min(1, (t - t0) / dur);
      const e = 1 - Math.pow(1 - k, 3);
      const v = from + (to - from) * e;
      $('#resMo').textContent = fmtMoney(v);
      $('#resYr').textContent = fmtMoney(v * 12) + ' в год';
      if (k < 1) rafSave = requestAnimationFrame(step);
    };
    rafSave = requestAnimationFrame(step);
  }

  function recalc(fromProfile) {
    let spend;
    if (fromProfile) { spend = estimate(); input.value = Math.round(spend).toLocaleString('ru-RU').replace(/,/g, ' '); }
    else spend = +input.value.replace(/[^\d]/g, '') || 0;

    const now = spend, next = spend * 0.5, save = now - next;
    $('#resNow').textContent = money(now);
    $('#resNew').textContent = money(next);
    tweenSave(save);
  }

  input.oninput = () => recalc(false);
  input.onblur  = () => { input.value = (+input.value.replace(/[^\d]/g, '') || 0).toLocaleString('ru-RU').replace(/,/g, ' '); };
  $('#advToggle').onclick = () => { const a = $('#adv'); a.hidden = !a.hidden; };

  renderParams(); recalc(true);
})();

/* ---------------------------------------------------------
   12 · VIDEO LIBRARY
   --------------------------------------------------------- */
(() => {
  const lib = $('#lib');
  if (!lib) return;
  // each card opens the matching written guide, so the play-styled cover leads somewhere
  TUTORIALS.forEach(([num, name, dur, cover]) => {
    const n = el('a', 'tut reveal');
    n.href = '/ru/docs/learn/' + GUIDE_SLUGS[cover];
    n.innerHTML = `
      <div class="tut__thumb">${window.apiTokenVideoCover(cover, 'ru', true)}</div>
      <div class="tut__body">
        <span class="tut__num">${num}</span><span class="tut__dur">${dur}</span>
        <span class="tut__name">${name}</span>
      </div>`;
    lib.appendChild(n);
  });
})();

/* ---------------------------------------------------------
   FAQ
   --------------------------------------------------------- */
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

/* ---------------------------------------------------------
   INIT
   --------------------------------------------------------- */
observeReveals();

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
