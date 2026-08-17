import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Gemini Pro, Flash и Flash-Lite: сравнение",
    h1: "Сравнение Gemini Pro, Flash и Flash-Lite",
    description: "Сравните Gemini Pro, Flash и Flash-Lite по цене, контексту, reasoning и задачам, чтобы выбрать модель для кода, агентов и массового API.",
    keywords: ["gemini pro или flash", "gemini flash или flash lite", "лучшая модель gemini", "сравнение моделей gemini", "gemini для программирования", "gemini 3.6 flash"],
    dek: "Выбирайте tier как маршрут: Pro — для самого сложного reasoning, Flash — coding default, Flash-Lite — дешёвые массовые шаги. Один ключ работает со всеми тремя.",
    sections: [
      { h2: "Выбор по задаче", blocks: [
        { type: "table", headers: ["Tier", "Для чего подходит", "Рекомендуемый ID"], rows: [
          ["Pro", "Сложный reasoning, планирование, глубокий анализ кода и документов", "gemini-3.1-pro-preview"],
          ["Flash", "Повседневный код, multimodal-агенты и production", "gemini-3.6-flash"],
          ["Flash-Lite", "Классификация, извлечение, роутинг и pre-processing", "gemini-3.1-flash-lite"],
          ["Image", "Генерация и редактирование изображений", "gemini-3.1-flash-image"],
        ] },
        { type: "p", text: "Gemini 3.6 Flash — лучший старт для новых текстовых задач. Только самые сложные запросы поднимайте до Pro, а предсказуемый bulk опускайте до Flash-Lite." },
      ] },
      { h2: "Компромисс контекста и цены", blocks: [
        { type: "list", items: [
          "Текущие текстовые модели дают контекст 1M и output до 64K.",
          "У Pro есть long-context premium после 200K input; Flash и Flash-Lite сохраняют плоские ставки.",
          "Cached input текстовых моделей обычно стоит 10% fresh input.",
          "Перед большими запросами используйте countTokens и маршрутизируйте по eval, а не по названию модели.",
        ] },
      ] },
    ],
    faq: [
      { q: "Какую Gemini выбрать для программирования?", a: "Начните с Gemini 3.6 Flash. Сложную архитектуру и review отправляйте в 3.1 Pro Preview, дешёвые предсказуемые шаги — в Flash-Lite." },
      { q: "У Flash-Lite меньше контекст?", a: "Нет. Опубликованные text Flash-Lite сохраняют контекст 1M; их преимущество — цена и задержка на простых задачах." },
      { q: "Для смены tier нужен новый ключ?", a: "Нет. Оставьте тот же base URL и x-goog-api-key, измените только model ID." },
    ],
  };
