import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "GPT Image 2 API: генерация и редактирование",
    h1: "Генерация и редактирование изображений через GPT Image 2 API",
    description: "Используйте GPT Image 2 для генерации и редактирования изображений: endpoints, model ID, лимит референсов, токенные цены и скидка apiToken.sale 50%.",
    keywords: ["gpt image 2 api", "gpt-image-2", "api генерации изображений openai", "gpt image редактирование", "цена gpt image", "image generation api"],
    dek: "GPT Image 2 использует отдельные image routes, но тот же ключ и баланс, что GPT для текста. Создавайте изображения по промпту или редактируйте до пяти PNG-референсов без отдельного тарифа.",
    sections: [
      { h2: "Запрос на генерацию", blocks: [
        sourceBlock("gpt-image-2-api-guide", 0, 0),
        { type: "p", text: "Для редактирования отправьте multipart/form-data на /v1/images/edits с той же моделью и максимум пятью PNG. Текущая поверхность возвращает один PNG без стриминга." },
      ] },
      { h2: "Как считается стоимость изображения", blocks: [
        { type: "table", headers: ["Компонент", "Официально за 1 млн", "Цена здесь"], rows: [
          ["Текстовый input", "$5", "$2.50"],
          ["Image input", "$8", "$4"],
          ["Image output", "$30", "$15"],
        ] },
        { type: "list", items: [
          "Кэшированный текстовый и image input стоит 25% обычного тарифа.",
          "gpt-image-2 — alias immutable-снимка gpt-image-2-2026-04-21.",
          "Image usage списывается с того же баланса, что запросы GPT, Claude и Gemini.",
        ] },
      ] },
    ],
    faq: [
      { q: "Какой endpoint использует GPT Image 2?", a: "POST /v1/images/generations для нового изображения и POST /v1/images/edits для редактирования на OpenAI-совместимом base URL." },
      { q: "GPT Image 2 умеет редактировать изображение?", a: "Да. Маршрут edits принимает до пяти PNG-референсов в multipart/form-data." },
      { q: "Нужны отдельный ключ и баланс?", a: "Нет. Используются тот же Bearer-ключ и предоплаченный баланс, что для остальных моделей." },
    ],
  };
