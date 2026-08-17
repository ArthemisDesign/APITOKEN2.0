import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Как купить API-ключ GPT",
    h1: "Как купить API-ключ GPT",
    description: "Купите API-ключ GPT с предоплаченным балансом, оплатой картой или криптовалютой и OpenAI-совместимым endpoint для GPT-5.6, GPT-5.5 и GPT Image 2 со скидкой 50%.",
    keywords: ["купить api ключ gpt", "api ключ gpt", "купить ключ openai api", "gpt-5.6 api", "openai совместимый api", "gpt api с предоплатой"],
    dek: "Один ключ apiToken.sale открывает каталог GPT без отдельного аккаунта OpenAI Platform. Пополните баланс, укажите OpenAI-совместимый endpoint и платите на 50% меньше официальной стоимости каждого запроса.",
    sections: [
      { h2: "Получите ключ GPT за три шага", blocks: [
        { type: "steps", items: [
          "Создайте аккаунт apiToken.sale и выпустите ключ в дашборде.",
          "Пополните баланс на любую целую сумму в долларах картой или криптовалютой — без пакетов и ежемесячных обязательств.",
          "Укажите base URL https://router.apitoken.sale/v1, используйте Authorization: Bearer и выберите модель из GET /v1/models.",
        ] },
        sourceBlock("how-to-buy-gpt-api-key", 0, 1),
      ] },
      { h2: "Что входит в доступ", blocks: [
        { type: "list", items: [
          "Responses и Chat Completions с инкрементальным SSE-стримингом.",
          "GPT-5.6 Sol, Terra и Luna, предыдущие GPT и отдельные маршруты GPT Image 2.",
          "Тот же ключ и баланс работают с поддерживаемыми Claude, Gemini и Kimi.",
          "Плоская B2C-скидка 50% от официальной стоимости каждого запроса.",
        ] },
        { type: "note", text: "Храните ключ в серверной переменной окружения. GPT использует Authorization: Bearer; x-api-key и x-goog-api-key относятся к протоколам Anthropic и Gemini." },
      ] },
    ],
    faq: [
      { q: "Нужен ли аккаунт OpenAI?", a: "Нет. Ключ, баланс и биллинг находятся в apiToken.sale; клиенту нужен только custom base URL и Bearer-ключ." },
      { q: "Один ключ работает с GPT и Claude?", a: "Да. Один sk-pool ключ и баланс покрывают всех поддерживаемых провайдеров; меняются только endpoint и заголовок авторизации." },
      { q: "Это OpenAI Platform?", a: "Нет. Это независимый OpenAI-совместимый шлюз со своим аккаунтом, предоплаченным балансом и каталогом моделей." },
    ],
  };
