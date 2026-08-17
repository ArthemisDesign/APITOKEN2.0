import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Gemini API Quickstart",
    h1: "Быстрый старт Gemini API: curl и Google GenAI SDK",
    description: "Первый запрос к Gemini через apiToken.sale с curl или Google GenAI SDK: нативный generateContent, x-goog-api-key и явный model ID.",
    keywords: ["gemini api quickstart", "инструкция gemini api", "google genai sdk base url", "gemini generatecontent", "gemini api curl", "пример gemini api"],
    dek: "Шлюз сохраняет нативный протокол Google Gemini. Замените base URL и API key, оставьте схемы generateContent и официального SDK и всегда выбирайте модель явно.",
    sections: [
      { h2: "Первый запрос через curl", blocks: [
        sourceBlock("gemini-api-quickstart", 0, 0),
        { type: "p", text: "Для инкрементального ответа вызовите streamGenerateContent?alt=sse. На том же model path доступен countTokens для бесплатной оценки input до генерации." },
      ] },
      { h2: "Официальный Python SDK", blocks: [
        sourceBlock("gemini-api-quickstart", 1, 0),
        { type: "list", items: [
          "Передавайте только голый base URL, без /v1beta в конфигурации SDK.",
          "Всегда задавайте конкретный model ID: автоматического default клиента может не быть в каталоге gateway.",
          "Храните APITOKEN_API_KEY в переменной окружения, а не в исходном коде.",
        ] },
      ] },
    ],
    faq: [
      { q: "Работает официальный Google GenAI SDK?", a: "Да. Укажите HttpOptions(base_url) как https://router.apitoken.sale и передайте ключ apiToken.sale; формы запросов и ответов остаются нативными." },
      { q: "Как стримить ответ Gemini?", a: "Используйте /v1beta/models/{model}:streamGenerateContent?alt=sse с x-goog-api-key или соответствующий streaming-метод SDK." },
      { q: "Почему двойной /v1beta даёт 404?", a: "Google SDK добавляет версию API сам. Укажите только голый host, чтобы в итоговом URL был один /v1beta." },
    ],
  };
