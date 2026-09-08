/* Editorial covers for the landing's existing tutorial/demo slots.
   Topic IDs are shared by RU/EN, integration tabs and the tutorial library.
   These are code-native illustrations, not screenshots or connected video files. */
(() => {
  const topics = {
    start: { number: '01', palette: 'coral', ru: ['Первый', 'API-запрос', 'От API-ключа до ответа модели'], en: ['Your first', 'API request', 'From an API key to a model response'] },
    'claude-code': { number: '02', palette: 'ink', ru: ['Claude', 'Code', 'Подключение через терминал'], en: ['Claude', 'Code', 'Connect through your terminal'] },
    cursor: { number: '03', palette: 'paper', ru: ['Cursor', '× apiToken', 'Ключ и endpoint в редакторе'], en: ['Cursor', '× apiToken', 'Your key and endpoint in the editor'] },
    models: { number: '04', palette: 'paper', ru: ['Меняйте', 'модели', 'Один ключ — разные провайдеры'], en: ['Switch', 'models', 'One key, multiple providers'] },
    direct: { number: '05', palette: 'ink', ru: ['Прямой', 'API', 'Запрос, ответ и стриминг'], en: ['Direct', 'API', 'Requests, responses and streaming'] },
    billing: { number: '06', palette: 'coral', ru: ['Под контролем:', 'расходы', 'Баланс и использование токенов'], en: ['Understand', 'your spend', 'Balance and token usage'] },
    codex: { number: '07', palette: 'paper', ru: ['Codex', 'CLI', 'Настройка агента для кода'], en: ['Codex', 'CLI', 'Set up your coding agent'] },
    opencode: { number: '08', palette: 'ink', ru: ['opencode', '× apiToken', 'Провайдер, ключ и запуск'], en: ['opencode', '× apiToken', 'Provider, key and launch'] },
  };
  const shell = (id, command) => `<div class="vcover-window"><div class="vcover-window__bar"><i></i><i></i><i></i><span>terminal</span></div><div class="vcover-terminal"><span class="vcover-logo vcover-logo--${id}"></span><code><b>$</b> ${command}<i class="vcover-caret"></i></code><span class="vcover-code-line"></span><span class="vcover-code-line short"></span></div></div>`;
  const art = {
    start: '<div class="vcover-access"><span class="vcover-access__label">API KEY</span><span class="vcover-key"><i></i></span><code>sk-pool-••••</code></div><div class="vcover-response"><span>API</span><b>→</b><span>200 OK</span></div>',
    'claude-code': shell('claude-code', 'claude'),
    codex: shell('codex', 'codex'),
    opencode: shell('opencode', 'opencode'),
    cursor: '<div class="vcover-window vcover-editor"><div class="vcover-window__bar"><i></i><i></i><i></i><span>settings</span></div><div class="vcover-editor__body"><span class="vcover-braces">{ }</span><div><span>BASE URL</span><i class="vcover-code-line"></i></div><div><span>API KEY</span><code>•••• ••••</code></div></div></div>',
    models: '<div class="vcover-models"><div><i class="vcover-logo vcover-logo--claude-code"></i><span>Claude</span></div><div><i class="vcover-logo vcover-logo--codex"></i><span>GPT</span></div><div><i class="vcover-logo vcover-logo--gemini"></i><span>Gemini</span></div><span class="vcover-models__route">API KEY <b>↗</b></span></div>',
    direct: '<div class="vcover-api"><span class="vcover-api__method">POST</span><span class="vcover-braces">{ }</span><div class="vcover-api__response"><b>200</b><span>OK / SSE</span></div><i class="vcover-stream"></i></div>',
    billing: '<div class="vcover-window vcover-billing"><div class="vcover-window__bar"><span>TOKENS / USAGE</span></div><div class="vcover-bars"><i></i><i></i><i></i><i></i><i></i><i></i></div><div class="vcover-billing__legend"><span>IN</span><span>OUT</span></div></div>',
  };
  window.apiTokenVideoCover = (id, language, compact = false) => {
    const topic = topics[id];
    if (!topic) throw new Error(`Unknown video cover topic: ${id}`);
    const [line1, line2, subtitle] = topic[language === 'ru' ? 'ru' : 'en'];
    return `<div class="vcover vcover--${topic.palette}${compact ? ' vcover--compact' : ''}" data-cover="${id}" aria-hidden="true">
      <div class="vcover-brand"><span>apiToken<span class="vcover-brand__reg">®</span></span><span class="vcover-number">${topic.number} / ${language === 'ru' ? 'УРОК' : 'GUIDE'}</span></div>
      <div class="vcover-copy"><strong class="vcover-title"><span>${line1}</span><span>${line2}</span></strong><span class="vcover-subtitle">${subtitle}</span></div>
      <div class="vcover-art vcover-art--${id}">${art[id]}</div>
      ${compact ? `<span class="vcover-caption"><i></i>${language === 'ru' ? 'Инструкция' : 'Tutorial'}</span>` : ''}
    </div>`;
  };
})();
