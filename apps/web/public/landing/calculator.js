/* Shared RU/EN token estimator. Model prices come from the visible landing catalog.
   Claude/GPT cache reads cost 10% of fresh input; writes and media are not estimated. */
(function (root) {
  'use strict';
  const NANO = 1_000_000_000n;
  const MAX_TOKENS = 9_007_199_254_740_991n;

  function invalid(field) {
    return Object.assign(new Error('Invalid calculator field: ' + field), { field });
  }

  function tokenCount(value, field) {
    const text = String(value ?? '').trim().replace(/[\u00a0\u202f]/g, ' ');
    if (!/^\d+$/.test(text) && !/^\d{1,3}(?:[ ,]\d{3})+$/.test(text)) throw invalid(field);
    const digits = text.replace(/[ ,]/g, '').replace(/^0+(?=\d)/, '');
    if (digits.length > 16) throw invalid(field);
    const count = BigInt(digits);
    if (count > MAX_TOKENS) throw invalid(field);
    return count;
  }

  function cacheShare(value) {
    const text = String(value ?? '').trim().replace(/\s*%$/, '');
    if (!/^\d{1,3}(?:[.,]\d{1,2})?$/.test(text)) throw invalid('cachePercent');
    const [whole, fraction = ''] = text.split(/[.,]/);
    const share = BigInt(whole) * 100n + BigInt(fraction.padEnd(2, '0'));
    if (share > 10_000n) throw invalid('cachePercent');
    return share;
  }

  function priceRate(value) {
    const text = String(value).replace(/^\$/, '');
    if (!/^\d+(?:\.\d{1,9})?$/.test(text)) throw invalid('model');
    const [whole, fraction = ''] = text.split('.');
    return BigInt(whole) * NANO + BigInt(fraction.padEnd(9, '0'));
  }

  function modelsFromProviders(providers) {
    return providers.filter(p => p.name === 'Anthropic' || p.name === 'OpenAI').flatMap(p => p.models.map(model => {
      const inputRate = priceRate(model.in);
      return { id: model.id, name: model.name, inputRate, outputRate: priceRate(model.out), cacheRate: inputRate / 10n };
    }));
  }

  function calculate({ inputTokens, outputTokens, cachePercent, model }) {
    const input = tokenCount(inputTokens, 'inputTokens');
    const output = tokenCount(outputTokens, 'outputTokens');
    const cache = cacheShare(cachePercent);
    if (!model || !['inputRate', 'outputRate', 'cacheRate'].every(key => typeof model[key] === 'bigint' && model[key] >= 0n)) throw invalid('model');
    const numerator = input * ((10_000n - cache) * model.inputRate + cache * model.cacheRate)
      + output * 10_000n * model.outputRate;
    const denominator = 10_000n * 1_000_000n;
    const next = (numerator + denominator / 2n) / denominator;
    // The catalog shows prices after the homepage's flat 50% discount.
    const now = (numerator * 2n + denominator / 2n) / denominator;
    const save = now - next;
    return { now, next, save, year: save * 12n };
  }

  function formatMoney(nano, locale = 'en-US') {
    const ru = locale.startsWith('ru');
    if (nano > 0n && nano < 10_000_000n) return ru ? '<$0,01' : '<$0.01';
    const cents = (nano + 5_000_000n) / 10_000_000n;
    return '$' + (cents / 100n).toLocaleString(ru ? 'ru-RU' : 'en-US')
      + (ru ? ',' : '.') + String(cents % 100n).padStart(2, '0');
  }

  function bind({ providers, language, onResult, onInvalid, onClose, document = root.document }) {
    const panel = document.getElementById('adv');
    const toggle = document.getElementById('advToggle');
    const error = document.getElementById('advError');
    const fields = {
      inputTokens: document.getElementById('advInputTokens'),
      outputTokens: document.getElementById('advOutputTokens'),
      cachePercent: document.getElementById('advCache'),
      model: document.getElementById('advModel'),
    };
    const models = modelsFromProviders(providers);
    for (const model of models) {
      const option = document.createElement('option');
      option.value = model.id;
      option.textContent = model.name;
      fields.model.appendChild(option);
    }
    fields.model.value = models.some(model => model.id === 'claude-opus-5') ? 'claude-opus-5' : models[0]?.id || '';
    const messages = language === 'ru' ? {
      inputTokens: 'Введите целое число входных токенов от 0 до 9 007 199 254 740 991.',
      outputTokens: 'Введите целое число выходных токенов от 0 до 9 007 199 254 740 991.',
      cachePercent: 'Доля чтения из кэша — от 0 до 100%, не более двух знаков после запятой.',
      model: 'Выберите модель для расчёта.',
    } : {
      inputTokens: 'Enter a whole input token count from 0 to 9,007,199,254,740,991.',
      outputTokens: 'Enter a whole output token count from 0 to 9,007,199,254,740,991.',
      cachePercent: 'Cache reads must be from 0 to 100%, with at most two decimal places.',
      model: 'Choose a model to calculate its cost.',
    };

    function clearError() {
      error.hidden = true;
      error.textContent = '';
      Object.values(fields).forEach(field => field.removeAttribute('aria-invalid'));
    }
    function close() {
      panel.hidden = true;
      toggle.setAttribute('aria-expanded', 'false');
      clearError();
    }
    function update() {
      if (panel.hidden) return;
      clearError();
      let result;
      try {
        result = calculate({
          inputTokens: fields.inputTokens.value,
          outputTokens: fields.outputTokens.value,
          cachePercent: fields.cachePercent.value,
          model: models.find(model => model.id === fields.model.value),
        });
      } catch (problem) {
        const field = problem.field || 'model';
        error.textContent = messages[field];
        error.hidden = false;
        fields[field].setAttribute('aria-invalid', 'true');
        onInvalid();
        return;
      }
      onResult(result);
    }
    for (const [name, field] of Object.entries(fields)) field.addEventListener(name === 'model' ? 'change' : 'input', update);
    toggle.addEventListener('click', () => {
      if (panel.hidden) {
        panel.hidden = false;
        toggle.setAttribute('aria-expanded', 'true');
        update();
      } else {
        close();
        onClose();
      }
    });
    close();
    return { close };
  }

  const api = { modelsFromProviders, calculate, formatMoney, bind };
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.apiTokenCalculator = api;
})(typeof window === 'object' ? window : globalThis);
