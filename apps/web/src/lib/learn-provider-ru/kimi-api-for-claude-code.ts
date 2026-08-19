import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Kimi API для Claude Code: K3 и Kimi for Coding",
    h1: "Запуск Kimi K3 и Kimi for Coding в Claude Code",
    description: "Kimi API для Claude Code: направьте Claude Code на Kimi K3 или Kimi for Coding через apiToken.sale — контекст 1M, закреплённые tier и скидка 50%.",
    keywords: ["kimi claude code", "kimi api для claude code", "kimi k3 claude code", "kimi for coding claude code", "claude code custom model", "claude code kimi api", "claude code anthropic_base_url", "claude code subagent model", "k3 1m claude code", "claude code без подписки claude", "kimi api anthropic messages endpoint"],
    dek: "Claude Code говорит по Anthropic Messages с любым эндпоинтом, который вы укажете, поэтому subscription alias Kimi на роутере apiToken.sale работает без плагина и без патча. Надёжная настройка закрепляет каждый внутренний model tier на Kimi — незакреплённый tier унаследует Claude ID и упадёт только тогда, когда запустится фоновый путь. Весь расход ложится на один предоплатный баланс — вдвое ниже официальных токен-расценок Kimi.",
    sections: [
      { h2: "Claude Code уже говорит на протоколе Kimi", blocks: [
        { type: "p", text: "Claude Code отправляет запросы Anthropic Messages туда, куда указывает ANTHROPIC_BASE_URL, а роутер https://router.apitoken.sale отвечает по этому протоколу для subscription alias Kimi. Ни плагина, ни прокси, ни форка не нужно: вы меняете переменные окружения — и каждая сессия, выбор tier и вызов subagent уходят в Kimi вместо Anthropic. Биллинг переезжает на ваш предоплатный баланс apiToken.sale — ровно на 50% ниже официальных токен-расценок Kimi." },
        { type: "p", text: "Единственное, из-за чего такая настройка ломается молча, — внутренняя карта моделей Claude Code. Она держит отдельные модели для основной сессии, tiers Opus/Sonnet/Haiku и subagents. Если задать только ANTHROPIC_MODEL, видимый диалог перенаправится, а фоновые пути — генерация заголовка, compaction, Task subagents — по-прежнему пойдут с унаследованными Claude ID и сломаются в момент запуска." },
        { type: "note", text: "Новые аккаунты, созданные через Google или GitHub, получают приветственный бонус $5 на баланс платформы — действует на поддерживаемые модели Claude, GPT, Gemini и Kimi; аккаунтам по email и паролю бонус не начисляется." },
      ] },
      { h2: "Закрепите эндпоинт и каждый model tier", blocks: [
        sourceBlock("kimi-api-for-claude-code", 1, 0),
        { type: "p", text: "Три переменные ANTHROPIC_DEFAULT_* покрывают tier-маршрутизацию Claude Code, CLAUDE_CODE_SUBAGENT_MODEL — Task subagents, а две переменные контекста поднимают и окно, и потолок auto-compact до 1M токенов K3. На Anthropic lane указывайте subscription alias в чистом виде; scoped-каталог GET /v1/models показывает написания с namespace kimi/*, поэтому сверьтесь с ним, прежде чем закреплять alias в долгоживущем окружении." },
        { type: "note", text: "Не пропускайте две переменные 1M на alias k3 и не оставляйте их на alias с 256K. Они сообщают Claude Code, сколько контекста можно использовать до compaction, и значение, которое обслуживаемая модель не поддерживает, искажает это решение в обе стороны." },
      ] },
      { h2: "Подберите alias под сессию", blocks: [
        { type: "table", headers: ["Alias", "Контекст", "Официально hit / miss / output", "Здесь после скидки 50%"], rows: [
          ["kimi-for-coding", "256K", "$0.19 / $0.95 / $4", "$0.095 / $0.475 / $2"],
          ["kimi-for-coding-highspeed", "256K", "$0.38 / $1.90 / $8", "$0.19 / $0.95 / $4"],
          ["k3-256k", "256K", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
          ["k3 · k3[1m]", "1M", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
        ] },
        { type: "p", text: "Цифры — за 1M токенов; кеширование Kimi работает автоматически, поэтому cache hit и cache miss тарифицируются как отдельные ноги. Для alias на 256K, таких как k3-256k или kimi-for-coding, оставьте закрепление tiers ровно как выше, но уберите CLAUDE_CODE_MAX_CONTEXT_TOKENS и CLAUDE_CODE_AUTO_COMPACT_WINDOW. k3[1m] — совместимое написание 1M-режима K3: роутер нормализует его в реальную wire-модель провайдера k3, поэтому обе формы стоят одинаково." },
        { type: "p", text: "Практичное разделение: kimi-for-coding — ежедневный рабочий инструмент для правок и тестовых циклов, k3 — когда сессии нужен длинный контекст по всему репозиторию, а kimi-for-coding-highspeed — только когда латентность оправдывает ровно двукратные базовые расценки." },
        { type: "link", text: "Полное сравнение K3 и Kimi for Coding", href: "/docs/learn/kimi-k3-vs-kimi-for-coding" },
      ] },
      { h2: "Проверяйте маршрут, а не самопрезентацию модели", blocks: [
        { type: "steps", items: [
          "Запустите сессию и выполните /status. Убедитесь, что Anthropic base URL — это apiToken.sale, прежде чем доверять чему-либо ещё в сессии.",
          "Отправьте один тривиальный промпт — «Reply with exactly: connected». Чистый ответ доказывает ключ, base URL и баланс за один round trip.",
          "Проверьте scoped-каталог, прежде чем закреплять alias надолго: curl https://router.apitoken.sale/v1/models с вашим ключом покажет, что ключ реально может вызывать.",
          "Один раз прогоните Task subagent. Это путь, который чаще всего тащит незакреплённый tier, — и такой отказ лучше поймать в первый день, а не посреди рефакторинга.",
        ] },
        { type: "note", text: "Не просите модель назвать себя как способ проверки. System prompt Claude Code может заставить любой backend назваться Claude, поэтому представление ничего не говорит о том, какая модель обслуживает turn, — доказательства это /status и путь запроса." },
      ] },
      { h2: "Переключатели reasoning — не селекторы моделей", blocks: [
        { type: "p", text: "Значение none или off в слоте модели отключает reasoning K3; оно не переключает вас на другую или более старую модель Kimi. Такие turns в любом случае остаются на тарифе K3. kimi-k2.6 — не адресуемая публичная модель на роутере, поэтому ввод этого имени не выбирает ничего — используйте alias из scoped-каталога." },
        { type: "p", text: "K3 поддерживает уровни reasoning effort low, high и max, по умолчанию — high; Kimi for Coding работает с включённым thinking. Reasoning-токены — это подмножество output и тарифицируются по расценке output: они никогда не добавляются повторно как отдельный класс токенов, поэтому сессия с тяжёлым thinking отражается как объём output, а не как надбавка." },
      ] },
      { h2: "Сколько стоит сессия Kimi на предоплатном балансе", blocks: [
        { type: "p", text: "Каждый turn тарифицируется за токен по официальным расценкам Kimi выше, а фиксированная скидка 50% вычитается до того, как списание коснётся вашего предоплатного баланса. Ни подписки, ни платы за место: неделя простоя ничего не стоит, а тяжёлый рефакторинг стоит ровно столько токенов, сколько он потребил, — при половине официального расхода. Тот же баланс покрывает поддерживаемые модели Claude, GPT и Gemini, так что сессия Claude Code на Kimi тратит тот же пул, что и всё остальное, что вы запускаете." },
        { type: "list", items: [
          "Задайте общий лимит расходов на ключ и проверяйте settled-расход в дашборде.",
          "По умолчанию используйте kimi-for-coding и поднимайте сессии по всему репозиторию до k3 вместо того, чтобы гонять всё по расценкам K3.",
          "Приберегите kimi-for-coding-highspeed для циклов, чувствительных к латентности: его расценки ровно вдвое выше базового tier.",
          "Воспринимайте ответ об исчерпании баланса как сигнал: пополните баланс — и следующий запрос пройдёт; повторные попытки ничего не меняют.",
        ] },
        { type: "link", text: "Расценки Kimi по alias и cache-ногам", href: "/docs/learn/kimi-api-pricing" },
        { type: "link", text: "Живой каталог всех поддерживаемых моделей и цен", href: "/models" },
      ] },
    ],
    faq: [
      { q: "Claude Code поддерживает Kimi K3?", a: "Да. Направьте ANTHROPIC_BASE_URL на https://router.apitoken.sale, аутентифицируйтесь ключом apiToken.sale и закрепите каждый model tier на допущенном subscription alias Kimi — плагин не нужен, потому что Claude Code уже говорит по Anthropic Messages." },
      { q: "Почему нужно закрепить все переменные моделей Claude Code?", a: "Claude Code выбирает отдельные модели для основной сессии, своих tiers и subagents. Незакреплённый tier может унаследовать Claude ID и упасть только при запуске фонового пути, поэтому сессия может выглядеть здоровой, пока сломаны compaction или вызов Task." },
      { q: "Как сохранить полный контекст K3 1M в Claude Code?", a: "Используйте k3 или k3[1m] и установите обе переменные CLAUDE_CODE_MAX_CONTEXT_TOKENS и CLAUDE_CODE_AUTO_COMPACT_WINDOW в 1048576. На alias с 256K, таких как k3-256k или kimi-for-coding, обе переменные не задавайте." },
      { q: "kimi-k2.6 — валидный ID модели в Claude Code?", a: "Нет. kimi-k2.6 не является адресуемой публичной моделью на роутере, а none/off в слоте модели отключает reasoning K3, а не выбирает другую модель. Используйте subscription alias, которые возвращает scoped-каталог GET /v1/models." },
      { q: "Сколько стоит сессия Claude Code на Kimi?", a: "Расход тарифицируется за токен по официальным расценкам Kimi с фиксированной скидкой 50% на предоплатном балансе — Kimi for Coding стоит $0.19 / $0.95 / $4 за 1M токенов cache-hit, cache-miss и output до скидки, High Speed — ровно вдвое дороже." },
    ],
  };
