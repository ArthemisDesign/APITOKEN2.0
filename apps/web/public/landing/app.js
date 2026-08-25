/* =========================================================
   apiToken landing — prototype logic
   Everything model-related is data-driven: add an entry to
   PROVIDERS / PICKS and the layout absorbs it with no CSS change.
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
      { name: 'Claude Opus 5',   id: 'claude-opus-5',   ctx: '200K', in: '$2.50', out: '$12.50', caps: ['coding','reasoning','agents','long'], badge: 'Лучшее качество' },
      { name: 'Claude Sonnet 5', id: 'claude-sonnet-5', ctx: '200K', in: '$1.00', out: '$5.00',  caps: ['coding','reasoning','agents'],        badge: 'Новинка', badgeKind: 'new' },
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
      { name: 'Kimi K2.5',      id: 'kimi-k2-5',      ctx: '256K', in: '$0.28', out: '$1.40', caps: ['cheap','coding','long'], badge: 'Лучшая цена' },
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
  { id: 'coding',  name: 'Код',              picks: ['claude-opus-5','gpt-5-6-sol','claude-sonnet-5','kimi-k2-5'] },
  { id: 'agents',  name: 'AI-агент',         picks: ['claude-opus-5','gpt-5-6-terra','claude-haiku-4-5','mistral-large-3'] },
  { id: 'support', name: 'Поддержка',        picks: ['claude-sonnet-5','gpt-5-6-luna','claude-haiku-4-5','kimi-k2-turbo'] },
  { id: 'content', name: 'Контент',          picks: ['claude-fable-5','gpt-5-6-sol','gemini-3-pro','kimi-k2-5'] },
  { id: 'image',   name: 'Изображения',      picks: ['gemini-image','gemini-3-pro','gpt-5-6-terra','mistral-small'] },
  { id: 'volume',  name: 'Высокие объёмы',   picks: ['gemini-3-flash','kimi-k2-turbo','claude-haiku-4-5','mistral-small'] },
  { id: 'reason',  name: 'Рассуждение',      picks: ['claude-opus-5','gpt-5-5','gemini-3-pro','claude-sonnet-5'] }
];

const ROLES = [
  { role: 'Максимальное качество', why: 'Самые сложные задачи, где важна точность результата.' },
  { role: 'Баланс',                why: 'Основная рабочая модель: качество близко к топовой, цена ниже.' },
  { role: 'Быстрая',               why: 'Короткая латентность для интерактивных сценариев.' },
  { role: 'Экономичная',           why: 'Массовый поток запросов с минимальной стоимостью.' }
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

const SWITCH_MODELS = [
  { id:'claude-opus-5',  prov:'Anthropic', lat:'1.2s', cost:'$0.0042', ans:'Первый подход выигрывает по стоимости, второй — по латентности. При объёме от 1M запросов разница становится решающей…' },
  { id:'gpt-5-6-sol',    prov:'OpenAI',    lat:'0.9s', cost:'$0.0038', ans:'Оба варианта рабочие. Ключевое отличие — в стоимости обслуживания на длинной дистанции…' },
  { id:'gemini-3-pro',   prov:'Google',    lat:'0.7s', cost:'$0.0026', ans:'Сравнение по трём осям: цена, скорость, качество ответа. Второй подход предпочтителен при высокой нагрузке…' },
  { id:'kimi-k2-5',      prov:'Kimi',      lat:'0.6s', cost:'$0.0009', ans:'Разница в стоимости — почти пятикратная. При равном качестве на этой задаче выбор очевиден…' }
];

const PROFILES = [
  { id:'product', name:'AI-продукт',        base:14000 },
  { id:'devteam', name:'Команда разработки',base:20000, seats:true },
  { id:'support', name:'Бот поддержки',     base:9000 },
  { id:'content', name:'Контент-платформа', base:12000 },
  { id:'agents',  name:'Агенты',            base:26000 },
  { id:'custom',  name:'Другое',            base:20000 }
];

const TUTORIALS = [
  ['01','Начало работы','2:04'],['02','Claude Code','1:42'],['03','Cursor','2:16'],
  ['04','Переключение моделей','1:18'],['05','Прямой API','2:35'],['06','Биллинг и расходы','1:26']
];

const FAQ = [
  ['Откуда берётся скидка до 50%?','Мы закупаем доступ к моделям объёмом и перераспределяем его между клиентами. Вы платите по официальным ставкам провайдера за вычетом скидки — сам запрос уходит в тот же официальный API.'],
  ['Какие модели доступны?','Модели Anthropic, OpenAI, Google, Kimi, Mistral и других провайдеров. Каталог пополняется по мере выхода новых моделей — менять интеграцию для этого не нужно.'],
  ['Можно ли использовать apiToken вместо существующего OpenAI API?','Да. Достаточно поменять base URL и ключ — формат запросов и ответов остаётся прежним, включая стриминг и вызов инструментов.'],
  ['Как быстро можно подключиться?','Около двух минут: регистрация, создание ключа, замена endpoint. Отдельные аккаунты у провайдеров не нужны.'],
  ['Поддерживаются ли Claude Code и Cursor?','Да, как и Codex CLI, opencode, Cline и любые SDK, работающие с Anthropic-, OpenAI- или Gemini-совместимыми маршрутами.'],
  ['Как оплачиваются запросы?','Предоплата: пополняете баланс на любую сумму, списание идёт пропорционально фактическому потреблению токенов по всем моделям сразу.'],
  ['Есть ли специальные условия для бизнеса?','Да. Для компаний действуют цены под объём, централизованный биллинг и отдельный канал поддержки — условия обсуждаются индивидуально.'],
  ['Что происходит при добавлении новых моделей?','Новая модель появляется в каталоге и сразу доступна по тому же ключу и endpoint. Изменения на вашей стороне не требуются.'],
  ['Можно ли использовать несколько моделей в одном продукте?','Да, это основной сценарий: дорогая модель на сложных шагах, дешёвая — на массовых, с общим балансом и единой статистикой.'],
  ['Как получить B2B предложение?','Заполните короткую форму в разделе «Бизнес-условия» — мы вернёмся с расчётом под ваш объём.']
];

const GROW_ITEMS = ['GPT','Claude','Gemini','Kimi','Mistral','Llama','Qwen','DeepSeek','+ новый провайдер','+ новая модель','+ ...'];

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
const byId = id => ALL_MODELS.find(m => m.id === id);

/* ---------------------------------------------------------
   HEADER
   --------------------------------------------------------- */
const hdr = $('#hdr');
addEventListener('scroll', () => hdr.classList.toggle('small', scrollY > 40), { passive: true });
$('#burger')?.addEventListener('click', () => $('.nav').classList.toggle('open'));

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
    const n = el('article', 'mcard');
    n.innerHTML = `
      <div class="mcard__top">
        <span class="mcard__name">${m.name}</span>
      </div>
      <span class="mcard__id">${m.id}</span>
      <div class="mcard__row"><span>вход / 1M</span><b>${m.in}</b></div>
      <div class="mcard__row"><span>выход / 1M</span><b>${m.out}</b></div>
      <div class="mcard__row"><span>контекст</span><b>${m.ctx}</b></div>
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
function buildVideo(host, cfg) {
  const secs = (cfg.duration || '2:00').split(':').reduce((a, b) => a * 60 + +b, 0);
  const v = el('div', 'vid');
  v.innerHTML = `
    <div class="vid__bar"><span>${cfg.label}</span><span>${cfg.duration}</span></div>
    <div class="vid__stage">
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

  const fmt = s => Math.floor(s / 60) + ':' + String(Math.floor(s % 60)).padStart(2, '0');
  const draw = () => { bar.style.width = (t / secs * 100) + '%'; time.textContent = fmt(t) + ' / ' + cfg.duration; };

  function play() {
    v.classList.add('playing'); state.textContent = 'Играет';
    timer = setInterval(() => {
      t += .25;
      if (t >= secs) { t = 0; pause(); state.textContent = 'Готово'; }
      draw();
    }, 250);
  }
  function pause() {
    clearInterval(timer); timer = null;
    v.classList.remove('playing');
    if (t > 0) state.textContent = 'Пауза';
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
   05 · MODEL EXPLORER
   --------------------------------------------------------- */
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
          <div><span>вход / 1M</span><b>${m.in}</b></div>
          <div><span>выход / 1M</span><b>${m.out}</b></div>
          <div><span>контекст</span><b>${m.ctx}</b></div>
        </div>
        <span class="pick__cta">Использовать модель →</span>`;
      out.appendChild(n);
    });
    observeReveals();
  }
  render();
})();

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
    buildVideo(host, { label: 'Инструкция / ' + active.name, duration: active.dur, lines: active.lines });
    steps.innerHTML = active.steps.map((s, i) => `<div><b>${String(i + 1).padStart(2, '0')}</b>${s}</div>`).join('');
  }
  render();
})();

/* ---------------------------------------------------------
   07 · MODEL SWITCH
   --------------------------------------------------------- */
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

/* ---------------------------------------------------------
   09 · CALCULATOR
   --------------------------------------------------------- */
(() => {
  const profWrap = $('#calcProfiles'), params = $('#calcParams'), input = $('#spendInput');
  if (!profWrap) return;
  let profile = PROFILES[1], devs = 25, usage = 2;
  const USAGE = ['Низкая', 'Средняя', 'Высокая', 'Очень высокая'];

  PROFILES.forEach(p => {
    const b = el('button', 'chip' + (p === profile ? ' on' : ''), p.name);
    b.onclick = () => {
      profile = p;
      $$('.chip', profWrap).forEach(x => x.classList.toggle('on', x === b));
      renderParams(); recalc(true);
    };
    profWrap.appendChild(b);
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
      <div class="range" style="margin-top:28px">
        <div class="range__top"><span class="calc__lab">Интенсивность</span><span class="range__val" id="useVal" style="font-size:18px">${USAGE[usage]}</span></div>
        <input type="range" id="useRange" min="0" max="3" value="${usage}">
      </div>`;
    $('#devRange').oninput = e => { devs = +e.target.value; $('#devVal').textContent = devs; recalc(true); };
    $('#useRange').oninput = e => { usage = +e.target.value; $('#useVal').textContent = USAGE[usage]; recalc(true); };
  }

  function estimate() {
    if (profile.seats) return devs * 260 * (0.6 + usage * 0.45);
    return profile.base * (0.7 + usage * 0.2);
  }

  function recalc(fromProfile) {
    let spend;
    if (fromProfile) { spend = estimate(); input.value = Math.round(spend).toLocaleString('ru-RU').replace(/,/g, ' '); }
    else spend = +input.value.replace(/[^\d]/g, '') || 0;

    const now = spend, next = spend * 0.5, save = now - next;
    $('#resNow').textContent = money(now);
    $('#resNew').textContent = money(next);
    $('#resMo').textContent  = money(save);
    $('#resYr').textContent  = money(save * 12) + ' в год';
  }

  input.oninput = () => recalc(false);
  input.onblur  = () => { input.value = (+input.value.replace(/[^\d]/g, '') || 0).toLocaleString('ru-RU').replace(/,/g, ' '); };
  $('#advToggle').onclick = () => { const a = $('#adv'); a.hidden = !a.hidden; };

  renderParams(); recalc(true);
})();

/* ---------------------------------------------------------
   10 · B2B FORM
   --------------------------------------------------------- */
$('#b2bForm')?.addEventListener('submit', e => {
  e.preventDefault();
  $('#b2bOk').hidden = false;
  e.target.querySelector('button').textContent = 'Отправлено';
});

/* ---------------------------------------------------------
   12 · VIDEO LIBRARY
   --------------------------------------------------------- */
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

/* ---------------------------------------------------------
   13 · GROWING CATALOG FIELD
   --------------------------------------------------------- */
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
