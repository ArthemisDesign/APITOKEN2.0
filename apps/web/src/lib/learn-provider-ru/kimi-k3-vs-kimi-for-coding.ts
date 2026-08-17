import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Kimi K3 и Kimi for Coding: сравнение",
    h1: "Сравнение Kimi K3 и Kimi for Coding",
    description: "Сравните Kimi K3, K3 256K, Kimi for Coding и High Speed по контексту, reasoning, задержке и цене для кода и агентов.",
    keywords: ["kimi k3 или kimi for coding", "kimi k3 api", "kimi k2.7 code", "лучшая kimi для кода", "сравнение моделей kimi", "kimi highspeed"],
    dek: "K3 — семейство для reasoning и длинного контекста; Kimi for Coding — экономичная coding-линейка. High Speed покупает скорость за двойную ставку, а aliases K3 выбирают 256K или 1M.",
    sections: [
      { h2: "Карта семейства", blocks: [
        { type: "table", headers: ["Публичный ID", "Контекст", "Для чего подходит"], rows: [
          ["kimi/kimi-for-coding", "256K", "Повседневный код и экономичные agent loops"],
          ["kimi/kimi-for-coding-highspeed", "256K", "Latency-sensitive код, где скорость окупается"],
          ["kimi/k3-256k", "256K", "K3 reasoning без полного context mode"],
          ["kimi/k3 · kimi/k3[1m]", "1M", "Большие кодовые базы, документы и сложный reasoning"],
        ] },
        { type: "p", text: "k3[1m] — compatibility spelling режима K3 1M, а не отдельная модель. Router нормализует его в настоящий wire model k3." },
      ] },
      { h2: "Reasoning и маршрутизация", blocks: [
        { type: "list", items: [
          "K3 поддерживает low, high и max reasoning effort; default — high.",
          "Kimi for Coding и High Speed работают с включённым thinking.",
          "Доступность моделей берётся из scoped /v1/models перед фиксацией alias.",
          "Практический router отправляет обычный код в Kimi for Coding, а большие и сложные задачи — в K3.",
        ] },
      ] },
    ],
    faq: [
      { q: "Какая Kimi лучше для программирования?", a: "Kimi for Coding — экономичный default. K3 выбирайте для сложного reasoning и длинного контекста, High Speed — только когда задержка важнее двойной цены." },
      { q: "k3 и k3[1m] — разные модели?", a: "Нет. Это один режим K3 1M; запись со скобками — compatibility alias." },
      { q: "Можно вызвать внутренний official model ID?", a: "Нет. Используйте публичные subscription aliases из router catalog, а не тарифные IDs вроде kimi-k2.7-code." },
    ],
  };
