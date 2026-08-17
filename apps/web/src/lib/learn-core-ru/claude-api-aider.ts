import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API в Aider",
    h1: "Используйте Claude API в Aider",
    description: "Запустите Aider на Claude через apiToken.sale: экспортируйте ANTHROPIC_API_BASE и ключ, выберите модель Claude и пишите код в терминале с единой скидкой 50%.",
    keywords: ["claude api aider", "aider anthropic", "aider claude", "aider anthropic api base", "aider claude api ключ"],
    dek: "Aider — терминальный парный программист, который быстро сжигает токены в длинных сессиях. Направьте его на дисконтный шлюз двумя переменными окружения и сохраните привычный процесс.",
    sections: [
      { h2: "Две переменные окружения", blocks: [
        { type: "code", code: `export ANTHROPIC_API_KEY=sk-pool-•••\nexport ANTHROPIC_API_BASE=https://router.apitoken.sale\n\naider --model anthropic/claude-opus-4-8` },
        { type: "p", text: "Под капотом Aider ведёт Anthropic-трафик через LiteLLM, который учитывает ANTHROPIC_API_BASE — конфиг-файл не нужен." },
        { type: "note", text: "Новые аккаунты через Google или GitHub получают приветственный бонус $5 на баланс платформы — этого хватит, чтобы подключить инструменты и сделать реальные вызовы до первого пополнения." },
      ] },
      { h2: "Выбор модели для Aider", blocks: [
        { type: "list", items: [
          "anthropic/claude-opus-4-8 — сложнейшие рефакторинги и длинные агентные правки.",
          "anthropic/claude-sonnet-5 — повседневный вариант по умолчанию; качество кода близко к Opus.",
          "anthropic/claude-haiku-4-5 — быстрые правки и дешёвые эксперименты.",
        ] },
        { type: "p", text: "Длинные сессии Aider — ровно то место, где потокенная скидка накапливается: карты репозитория, диффы и многофайловые правки тарифицируются как вход и выход." },
      ] },
    ],
    faq: [
      { q: "Работает ли Aider с кастомным эндпоинтом Claude?", a: "Да. Aider использует LiteLLM для моделей Anthropic, а LiteLLM учитывает переменную окружения ANTHROPIC_API_BASE — задайте её в https://router.apitoken.sale и запускайте Aider как обычно." },
      { q: "Какая модель Claude лучше в Aider?", a: "claude-sonnet-5 — лучший вариант по умолчанию для большинства задач; на сложнейшую многофайловую работу переключайтесь на claude-opus-4-8. Обе работают на одном ключе." },
      { q: "Насколько дешевле длинная сессия Aider?", a: "Каждый запрос тарифицируется по официальным потокенным ставкам минус ваша единая скидка 50%, поэтому сессия за $10 напрямую здесь стоит $5." },
    ],
  };
