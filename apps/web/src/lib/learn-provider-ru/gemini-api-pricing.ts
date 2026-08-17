import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Цены Gemini API: как считается стоимость",
    h1: "Цены Gemini API: Pro, Flash, Flash-Lite и изображения",
    description: "Сравните цены Gemini Pro, Flash, Flash-Lite и Nano Banana 2: cached input, long context, image output и плоская скидка apiToken.sale 50%.",
    keywords: ["цены gemini api", "стоимость gemini api", "цена токенов gemini", "цена gemini flash", "цена gemini pro", "дешевый gemini api"],
    dek: "Стоимость Gemini зависит от tier модели, кэшированного input, типа output и — для Pro — длины контекста. Gateway рассчитывает официальные компоненты точно и применяет скидку 50%.",
    sections: [
      { h2: "Тарифы основных текстовых моделей", blocks: [
        { type: "table", headers: ["Модель", "Официально: input / cache / output", "После скидки 50%"], rows: [
          ["gemini-3.1-pro-preview", "$2 / $0.20 / $12", "$1 / $0.10 / $6"],
          ["gemini-3.6-flash", "$1.50 / $0.15 / $7.50", "$0.75 / $0.075 / $3.75"],
          ["gemini-3.1-flash-lite", "$0.25 / $0.025 / $1.50", "$0.125 / $0.0125 / $0.75"],
          ["gemini-2.5-flash-lite", "$0.10 / $0.01 / $0.40", "$0.05 / $0.005 / $0.20"],
        ] },
        { type: "p", text: "Все значения указаны за 1 млн токенов. Cached input — самостоятельный usage-компонент провайдера; один токен не добавляется одновременно в fresh input." },
      ] },
      { h2: "Long context и изображения", blocks: [
        { type: "list", items: [
          "У Gemini 3.1 Pro Preview после 200K input весь запрос стоит $4 input и $18 output за 1 млн.",
          "Gemini 3.1 Flash Image тарифицирует текстовый output по $3, а image output — по $60 за 1 млн image-токенов.",
          "Cached input Flash Image стоит как обычный input: скидки текстовых моделей у него нет.",
          "B2C-скидка 50% применяется после точного расчёта официальных компонентов.",
        ] },
      ] },
    ],
    faq: [
      { q: "Какая модель Gemini самая дешёвая?", a: "Среди опубликованных текстовых tiers Gemini 2.5 Flash-Lite стоит официально $0.10 input/$0.40 output, здесь — $0.05/$0.20 после скидки." },
      { q: "Когда действует long-context тариф Gemini?", a: "Для Gemini 3.1 Pro Preview после 200K входных токенов. Повышенные ставки применяются ко всему запросу." },
      { q: "Как считается image output?", a: "Gemini 3.1 Flash Image стоит $60 за 1 млн image-output токенов официально и $30 после скидки 50%." },
    ],
  };
