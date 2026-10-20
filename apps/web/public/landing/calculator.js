/* Text-token estimate. Keep this rate-card snapshot aligned with src/lib/models.ts
   and crates/metering; the marketing cards in app.js/app-en.js are not rate cards. */
(function (global) {
  const NANO = 1_000_000_000n;
  const SOL_PROMO_END_MS = 1_795_305_600_000; // 2026-11-22 00:00 UTC, models.ts.
  const MODELS = [
    { id: 'claude-opus-5', name: 'Claude Opus 5', input: '5', output: '25', cache: '0.5' },
    { id: 'claude-sonnet-5', name: 'Claude Sonnet 5', input: '3', output: '15', cache: '0.3' },
    { id: 'claude-haiku-4-5', name: 'Claude Haiku 4.5', input: '1', output: '5', cache: '0.1' },
    { id: 'claude-fable-5', name: 'Claude Fable 5', input: '10', output: '50', cache: '1' },
    { id: 'gpt-5.6-sol', alias: 'gpt-5-6-sol', name: 'GPT-5.6 Sol', input: '5', output: '30', cache: '0.5' },
    { id: 'gpt-5.6-terra', alias: 'gpt-5-6-terra', name: 'GPT-5.6 Terra', input: '2', output: '12', cache: '0.2' },
    { id: 'gpt-5.6-luna', alias: 'gpt-5-6-luna', name: 'GPT-5.6 Luna', input: '0.2', output: '1.2', cache: '0.02' },
    { id: 'gpt-5.5', alias: 'gpt-5-5', name: 'GPT-5.5', input: '5', output: '30', cache: '0.5' },
    { id: 'gemini-3.1-pro-preview', name: 'Gemini 3.1 Pro Preview', input: '2', output: '12', cache: '0.2' },
    { id: 'gemini-3-flash-preview', name: 'Gemini 3 Flash Preview', input: '0.5', output: '3', cache: '0.05' },
  ];
  const COPY = {
    ru: {
      hint: 'По текущему базовому тарифу модели. Кэш — доля входных токенов; запись кэша, длинный контекст и дополнительные сборы не включены.',
      tokens: 'Укажите целое число токенов от 0 до 9 007 199 254 740 991.',
      cache: 'Укажите долю кэша от 0 до 100%.',
      model: 'Выберите поддерживаемую текстовую модель из списка.',
      year: ' в год',
    },
    en: {
      hint: 'Current base model rate. Cache is a share of input tokens; cache writes, long context and extra fees are excluded.',
      tokens: 'Enter a whole token count from 0 to 9 007 199 254 740 991.',
      cache: 'Enter a cache share from 0 to 100%.',
      model: 'Choose a supported text model from the list.',
      year: ' / year',
    },
  };

  function invalid(field, code) { throw Object.assign(new Error(code), { field, code }); }

  function tokens(raw, field) {
    const value = String(raw).trim();
    if (!/^(?:\d+|\d{1,3}(?:[ ,\u00a0\u202f]\d{3})+)$/.test(value)) invalid(field, 'tokens');
    const count = BigInt(value.replace(/[ ,\u00a0\u202f]/g, ''));
    if (count > 9_007_199_254_740_991n) invalid(field, 'tokens');
    return count;
  }

  function cacheBasisPoints(raw) {
    const match = /^(\d+)(?:[.,](\d{1,2}))?\s*%?$/.exec(String(raw).trim());
    if (!match) invalid('cachePercent', 'cache');
    const percent = BigInt(match[1]) * 100n + BigInt((match[2] || '').padEnd(2, '0'));
    if (percent > 10_000n) invalid('cachePercent', 'cache');
    return percent;
  }

  function rateNano(value) {
    const [whole, fraction = ''] = value.split('.');
    return BigInt(whole) * NANO + BigInt(fraction.padEnd(9, '0'));
  }

  function rateCard(modelId, nowMs) {
    const id = String(modelId).trim().toLowerCase();
    const model = MODELS.find(item => item.id === id || item.alias === id);
    if (!model) invalid('modelId', 'model');
    return model.id === 'gpt-5.6-sol' && nowMs < SOL_PROMO_END_MS
      ? { ...model, input: '4', output: '20', cache: '0.4' }
      : model;
  }

  function estimate(values, nowMs = Date.now()) {
    const input = tokens(values.inputTokens, 'inputTokens');
    const output = tokens(values.outputTokens, 'outputTokens');
    const cached = cacheBasisPoints(values.cachePercent);
    const model = rateCard(values.modelId, nowMs);
    // Percentage applies only to input: cache reads replace regular input,
    // rather than being added to it. Round only the final integer nanoUSD.
    const weighted = input * (rateNano(model.input) * (10_000n - cached) + rateNano(model.cache) * cached)
      + output * rateNano(model.output) * 10_000n;
    const denominator = 1_000_000n * 10_000n;
    const now = (weighted + denominator / 2n) / denominator;
    const next = (weighted + denominator) / (denominator * 2n); // Flat 50% discount.
    const save = now - next;
    return { now, next, save, year: save * 12n };
  }

  function formatMoney(nano, locale) {
    if (nano === 0n) return '$0';
    const digits = nano < 10_000_000n ? 9 : 2;
    const scale = 10n ** BigInt(9 - digits);
    const units = (nano + scale / 2n) / scale;
    const base = 10n ** BigInt(digits);
    const whole = (units / base).toLocaleString(locale).replace(/,/g, ' ');
    let fraction = (units % base).toString().padStart(digits, '0');
    if (digits === 9) fraction = fraction.replace(/0+$/, '');
    return '$' + whole + (fraction ? (locale === 'ru-RU' ? ',' : '.') + fraction : '');
  }

  function bind({ language, onBasic, beforeAdvanced = () => {}, now = () => Date.now() }) {
    const document = global.document;
    const panel = document.getElementById('adv');
    const toggle = document.getElementById('advToggle');
    const status = document.getElementById('advStatus');
    const fields = Object.fromEntries(['inputTokens', 'outputTokens', 'cachePercent', 'modelId']
      .map(name => [name, document.querySelector(`[data-calc-field="${name}"]`)]));
    const copy = COPY[language];
    const locale = language === 'ru' ? 'ru-RU' : 'en-US';
    const outputIds = { now: 'resNow', next: 'resNew', save: 'resMo', year: 'resYr' };
    const list = document.getElementById('advModels');
    MODELS.forEach(model => {
      const option = document.createElement('option');
      option.value = model.id;
      option.label = model.name;
      list.appendChild(option);
    });
    toggle.setAttribute('aria-controls', 'adv');
    toggle.setAttribute('aria-expanded', String(!panel.hidden));
    status.textContent = copy.hint;

    function close() {
      panel.hidden = true;
      toggle.setAttribute('aria-expanded', 'false');
    }

    function update() {
      if (panel.hidden) return;
      beforeAdvanced();
      Object.values(fields).forEach(field => field.removeAttribute('aria-invalid'));
      try {
        const result = estimate(Object.fromEntries(Object.entries(fields).map(([key, field]) => [key, field.value])), now());
        Object.entries(outputIds).forEach(([key, id]) => {
          document.getElementById(id).textContent = formatMoney(result[key], locale) + (key === 'year' ? copy.year : '');
        });
        status.textContent = copy.hint;
      } catch (error) {
        if (!error.field) throw error;
        fields[error.field].setAttribute('aria-invalid', 'true');
        status.textContent = copy[error.code];
        Object.values(outputIds).forEach(id => { document.getElementById(id).textContent = '—'; });
      }
    }

    Object.values(fields).forEach(field => {
      field.addEventListener('input', update);
      field.addEventListener('change', update);
    });
    toggle.addEventListener('click', () => {
      if (!panel.hidden) { close(); onBasic(); }
      else {
        panel.hidden = false;
        toggle.setAttribute('aria-expanded', 'true');
        update();
      }
    });
    return { close };
  }

  global.LandingCalculator = { estimate, formatMoney, rateCard, bind };
})(globalThis);
