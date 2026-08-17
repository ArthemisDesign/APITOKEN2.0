import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Настройка Codex CLI с apiToken.sale — профиль GPT-5.6",
    h1: "Запускайте Codex CLI на apiToken.sale",
    description: "Настройте Codex CLI именованным профилем model_providers на OpenAI-совместимый эндпоинт apiToken.sale — модели GPT-5.6 на предоплаченном балансе с единой скидкой 50%, без аккаунта ChatGPT.",
    keywords: ["codex cli настройка", "codex config.toml", "codex кастомный провайдер", "codex api ключ", "codex cli gpt-5.6", "codex responses api", "codex cli без chatgpt"],
    dek: "Codex CLI полностью работает на аутентификации по API-ключу, если задать кастомного провайдера модели. Один TOML-профиль направляет его на apiToken.sale, и предоплаченный баланс покрывает каждую сессию — без входа в ChatGPT и с единой скидкой 50% от официального расхода.",
    sections: [
      { h2: "Создайте профиль", blocks: [
        { type: "p", text: "Сохраните это как ~/.codex/apitoken.config.toml. Именованный профиль не трогает конфигурацию Codex по умолчанию и возможный вход в ChatGPT — вы включаете его явно на каждый запуск." },
        { type: "code", code: `# ~/.codex/apitoken.config.toml\nmodel = "gpt-5.6-sol"\nmodel_provider = "apitoken"\n\n[model_providers.apitoken]\nname = "apiToken.sale"\nbase_url = "https://router.apitoken.sale/v1"\nwire_api = "responses"\nenv_key = "APITOKEN_API_KEY"` },
        { type: "p", text: "env_key задаёт имя переменной окружения, из которой Codex читает ключ, — секрет остаётся в шелле и никогда не попадает в TOML-файл." },
        { type: "note", text: "Новые аккаунты через Google или GitHub получают приветственный бонус $5 на баланс платформы для поддерживаемых Claude, GPT, Gemini и Kimi; аккаунтам по email и паролю бонус не начисляется." },
      ] },
      { h2: "Запуск и проверка", blocks: [
        { type: "code", code: `export APITOKEN_API_KEY=sk-pool-•••\ncodex --profile apitoken` },
        { type: "list", items: [
          "Всегда явно передавайте --profile apitoken, чтобы не было двусмысленности, какой провайдер — и какая переменная — активен.",
          "Меняйте модель под проект строкой model: gpt-5.6-sol для самой сложной работы, gpt-5.6-terra на каждый день, gpt-5.6-luna для быстрых дешёвых шагов.",
          "GET https://router.apitoken.sale/v1/models с тем же Bearer-ключом показывает актуальный набор моделей — единый каталог разделяет ID по провайдерам (anthropic/*, openai/*, google/*).",
        ] },
        { type: "note", text: "wire_api = \"responses\" — правильное значение для этого шлюза: он обслуживает и Responses, и Chat Completions, а Codex стримит через Responses. Ставьте \"chat\" только если конкретный клиент требует классическую форму." },
      ] },
      { h2: "Ошибки, которые можно встретить", blocks: [
        { type: "list", items: [
          "Missing APITOKEN_API_KEY — переменная из env_key не экспортирована в шелле, который запускает codex. Экспортируйте её в том же шелле или в профиле оболочки.",
          "stream error: unexpected status 401 — ключ неверен, отозван или base_url потерял суффикс /v1. Воспроизведите curl'ом вне Codex, чтобы понять, какая половина сломана.",
          "stream error: unexpected status 404 — ID модели не включён; проверьте GET https://router.apitoken.sale/v1/models вместо предположений.",
          "402 — предоплаченный баланс нужно пополнить; ожидание не поможет.",
        ] },
        { type: "link", text: "Полный разбор ошибок Codex — config.toml, auth.json, stream errors", href: "/errors/codex" },
      ] },
    ],
    faq: [
      { q: "Нужен ли аккаунт или подписка ChatGPT?", a: "Нет. С кастомным профилем model_providers и ключом провайдера в окружении Codex работает полностью на аутентификации по API-ключу — вход ChatGPT из auth.json роли не играет." },
      { q: "Меняет ли это мою конфигурацию Codex по умолчанию?", a: "Нет. Профиль живёт в отдельном файле и активируется только с --profile apitoken. Конфигурация и вход по умолчанию остаются как были." },
      { q: "Скидка та же, что и для Claude?", a: "Да. Использование GPT-5.6 метерится по официальным ставкам OpenAI, и ваша единая скидка B2C 50% применяется к тому же предоплаченному балансу." },
      { q: "Responses или Chat Completions для wire_api?", a: "Используйте wire_api = \"responses\" — шлюз обслуживает оба формата, а Codex построен вокруг потока Responses. Форма Chat Completions существует для клиентов, которым она нужна." },
    ],
  };
