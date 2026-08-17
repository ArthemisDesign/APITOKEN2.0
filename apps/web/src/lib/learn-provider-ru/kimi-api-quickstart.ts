import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Kimi API Quickstart",
    h1: "Быстрый старт Kimi API с Anthropic SDK",
    description: "Вызывайте Kimi K3 и Kimi for Coding через apiToken.sale с Anthropic Messages API, x-api-key, namespaced model IDs, terminal usage и общим балансом.",
    keywords: ["kimi api quickstart", "инструкция kimi api", "kimi anthropic api", "пример kimi k3 api", "kimi for coding api", "kimi api curl"],
    dek: "Kimi говорит на Anthropic Messages через единый router. Существующему Anthropic-клиенту нужны только custom base URL, ключ apiToken.sale и явный kimi/* model ID.",
    sections: [
      { h2: "Первый запрос через curl", blocks: [
        sourceBlock("kimi-api-quickstart", 0, 0),
        { type: "p", text: "Terminal usage сохраняет Anthropic-форму, поэтому существующий parser usage продолжит работать. Маршрут принимает stream: true, но инкрементальность на границе провайдера ещё проходит live-проверку." },
      ] },
      { h2: "Anthropic Python SDK", blocks: [
        sourceBlock("kimi-api-quickstart", 1, 0),
        { type: "note", text: "Не подставляйте Open Platform ID вроде kimi-k2.7-code. Публичный router принимает subscription aliases из GET /v1/models. OpenAI-совместимые клиенты вызывают те же Kimi aliases через единый route /v1." },
      ] },
    ],
    faq: [
      { q: "Можно использовать Anthropic SDK с Kimi?", a: "Да. Укажите base_url https://router.apitoken.sale и выберите kimi/* model ID из scoped-каталога." },
      { q: "Можно ли установить stream: true для Kimi?", a: "Маршрут принимает этот параметр, но инкрементальность upstream и публичных chunks ещё проходит live-проверку. Если важны сроки появления chunks, используйте non-stream режим." },
      { q: "С какого model ID начать?", a: "kimi/kimi-for-coding — coding default; kimi/k3-256k — K3 reasoning без полного контекста 1M." },
    ],
  };
