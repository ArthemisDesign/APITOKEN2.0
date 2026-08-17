import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Как купить API-ключ Gemini",
    h1: "Как купить API-ключ Gemini",
    description: "Купите API-ключ Gemini с предоплаченным балансом, оплатой картой или криптовалютой, нативными Gemini endpoints и одним аккаунтом для Gemini, GPT, Claude и Kimi со скидкой 50%.",
    keywords: ["купить api ключ gemini", "api ключ gemini", "google gemini api", "gemini api с предоплатой", "оплата gemini api", "дешевый gemini api"],
    dek: "Ключ apiToken.sale даёт доступ к нативному Gemini API без отдельного Google Cloud billing. Один раз пополните баланс, передавайте ключ как x-goog-api-key и используйте его со всеми поддерживаемыми провайдерами.",
    sections: [
      { h2: "Получите ключ Gemini за три шага", blocks: [
        { type: "steps", items: [
          "Создайте аккаунт apiToken.sale и выпустите sk-pool ключ в дашборде.",
          "Пополните баланс на любую целую сумму в долларах картой или криптовалютой; баланс не сгорает.",
          "Укажите Gemini base URL https://router.apitoken.sale, отправляйте x-goog-api-key и выберите модель из GET /v1beta/models.",
        ] },
        sourceBlock("how-to-buy-gemini-api-key", 0, 1),
      ] },
      { h2: "Какие возможности доступны", blocks: [
        { type: "list", items: [
          "Текстовые Pro, Flash и Flash-Lite через нативный протокол Gemini.",
          "Gemini 3.1 Flash Image (Nano Banana 2) для генерации изображений.",
          "generateContent, streamGenerateContent и countTokens с Google-совместимыми схемами.",
          "Плоская B2C-скидка 50% и тот же ключ/баланс для GPT, Claude и Kimi.",
        ] },
        { type: "note", text: "В Google SDK указывайте голый host. SDK сам добавляет /v1beta; двойной префикс приводит к 404." },
      ] },
    ],
    faq: [
      { q: "Нужен Google Cloud project?", a: "Нет. Gateway-аккаунтом и биллингом управляет apiToken.sale; клиенту нужны только custom base URL и sk-pool ключ." },
      { q: "Какой заголовок авторизует Gemini?", a: "x-goog-api-key. Не используйте Anthropic x-api-key или OpenAI Authorization: Bearer на нативных Gemini routes." },
      { q: "Один ключ может вызывать GPT и Gemini?", a: "Да. Ключ и баланс общие; для каждого провайдера меняются endpoint, протокол и model ID." },
    ],
  };
