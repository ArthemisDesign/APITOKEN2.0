import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Как купить API-ключ Kimi",
    h1: "Как купить API-ключ Kimi",
    description: "Купите предоплаченный API-ключ для Kimi K3 и Kimi for Coding, используйте Anthropic Messages или OpenAI-совместимые клиенты и платите на 50% меньше официальной стоимости.",
    keywords: ["купить api ключ kimi", "api ключ kimi", "kimi k3 api", "kimi for coding api", "moonshot kimi api", "kimi api с предоплатой"],
    dek: "Kimi доступен в собственном namespace на едином router. Используйте нативный Anthropic Messages route или OpenAI-совместимый клиент, а usage списывается с общего баланса Claude, GPT и Gemini.",
    sections: [
      { h2: "Получите доступ за три шага", blocks: [
        { type: "steps", items: [
          "Создайте аккаунт apiToken.sale и выпустите sk-pool ключ.",
          "Пополните баланс на любую целую сумму в долларах картой или криптовалютой — отдельный Kimi-план вам не нужен.",
          "Откройте GET https://router.apitoken.sale/v1/models и выберите kimi/* ID из живого каталога вашего ключа.",
        ] },
        sourceBlock("how-to-buy-kimi-api-key", 0, 1),
      ] },
      { h2: "Чем отличается маршрут Kimi", blocks: [
        { type: "list", items: [
          "Kimi — отдельный provider namespace, но не четвёртый wire format: используйте POST /v1/messages с x-api-key либо единый OpenAI-совместимый route /v1.",
          "Публичные IDs — aliases kimi/k3 и kimi/kimi-for-coding, а не внутренние тарифные названия.",
          "У K3 есть варианты контекста 256K и 1M, у Kimi for Coding — обычный и High Speed aliases.",
          "Ответ /v1/models — источник истины: доступность зависит от capacity провайдера и policy ключа.",
        ] },
      ] },
    ],
    faq: [
      { q: "Для Kimi нужен отдельный API-ключ?", a: "Нет. Тот же sk-pool ключ и баланс работают с Kimi и другими поддерживаемыми провайдерами." },
      { q: "Какой endpoint использует Kimi?", a: "Для Anthropic Messages — https://router.apitoken.sale/v1/messages; для OpenAI-совместимого клиента — Chat Completions на /v1. Оба принимают публичные kimi/* IDs." },
      { q: "Зачем сначала проверять /v1/models?", a: "Каталог scoped к ключу и показывает только модели, которые сейчас можно маршрутизировать и тарифицировать." },
    ],
  };
