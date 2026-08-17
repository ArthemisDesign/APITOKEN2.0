import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "GPT-5.6 Sol, Terra и Luna: сравнение",
    h1: "Сравнение GPT-5.6 Sol, Terra и Luna",
    description: "Сравните GPT-5.6 Sol, Terra и Luna по цене, reasoning effort, контексту и задачам, чтобы выбрать GPT-модель для кода и production.",
    keywords: ["gpt-5.6 sol или terra", "gpt-5.6 terra или luna", "лучшая модель gpt-5.6", "модели gpt-5.6", "сравнение gpt-5.6", "gpt для программирования"],
    dek: "У семейства GPT-5.6 общий контекст 400K, output до 128K и полный диапазон reasoning effort. Практическая разница — сколько качества и скорости вы покупаете за токен.",
    sections: [
      { h2: "Выбор по задаче", blocks: [
        { type: "table", headers: ["Tier", "Для чего подходит", "Официально input / output"], rows: [
          ["Sol", "Сложный reasoning, долгие агенты, трудный code review", "$5 / $30"],
          ["Terra", "Повседневный код, production-чат, сбалансированные агенты", "$2 / $12"],
          ["Luna", "Классификация, извлечение, роутинг и простые массовые задачи", "$0.20 / $1.20"],
        ] },
        { type: "p", text: "Terra — безопасный default: те же controls и context, что у Sol, за 40% цены. Переходите на Sol, когда eval показывает разницу в качестве, а предсказуемый bulk отправляйте в Luna." },
      ] },
      { h2: "Что у моделей одинаковое", blocks: [
        { type: "list", items: [
          "Контекст 400K и output до 128K.",
          "Текст и изображения на входе, текст на выходе.",
          "Responses и Chat Completions с SSE-стримингом.",
          "Reasoning effort от none до max в линейке GPT-5.6.",
          "Один endpoint, ключ и баланс для переключения модели по задаче.",
        ] },
      ] },
    ],
    faq: [
      { q: "Какая GPT-5.6 лучше для программирования?", a: "Начните с Terra. Sol используйте для самой сложной архитектуры и агентов, Luna — для дешёвых детерминированных подзадач." },
      { q: "Нужны разные endpoints для Sol, Terra и Luna?", a: "Нет. Все три работают через один OpenAI-совместимый base URL и ключ; меняется только model ID." },
      { q: "Terra поддерживает max reasoning effort?", a: "Да. У Sol, Terra и Luna один диапазон GPT-5.6, включая max." },
    ],
  };
