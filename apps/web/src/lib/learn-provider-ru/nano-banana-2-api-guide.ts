import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Nano Banana 2 API: инструкция",
    h1: "Генерация изображений через Nano Banana 2 API",
    description: "Используйте Gemini 3.1 Flash Image (Nano Banana 2) через нативный Gemini API: model ID, generateContent, цена image output и скидка 50%.",
    keywords: ["nano banana 2 api", "gemini 3.1 flash image api", "gemini генерация изображений", "nano banana api ключ", "цена gemini image", "google image api"],
    dek: "Nano Banana 2 — публичное имя Gemini 3.1 Flash Image. Модель работает через нативный generateContent, принимает multimodal input и возвращает изображения с того же баланса, что текстовые модели.",
    sections: [
      { h2: "Используйте точный model ID", blocks: [
        sourceBlock("nano-banana-2-api-guide", 0, 0),
        { type: "p", text: "Разбирайте response parts по MIME type: текстовые части содержат комментарий, image parts — сгенерированный файл. В API используйте gemini-3.1-flash-image, а не маркетинговое имя." },
      ] },
      { h2: "Лимиты и цены", blocks: [
        { type: "list", items: [
          "Контекст 128K и output до 32K — меньше, чем у текстовой Flash-линейки.",
          "Официально text input/output стоят $0.50/$3 за 1 млн, image output — $60.",
          "После скидки apiToken.sale это $0.25/$1.50 и $30 за image output.",
          "Cached input этой image-модели остаётся по полной ставке $0.50.",
        ] },
        { type: "note", text: "Для чисто текстового ответа выбирайте text Flash. Flash Image нужен, когда response должен содержать отрисованное изображение." },
      ] },
    ],
    faq: [
      { q: "Какой model ID у Nano Banana 2?", a: "gemini-3.1-flash-image на нативном маршруте Gemini generateContent." },
      { q: "Сколько стоит image output Nano Banana 2?", a: "$60 за 1 млн image-output токенов официально и $30 после скидки apiToken.sale 50%." },
      { q: "Нужен отдельный image API key?", a: "Нет. Используйте тот же sk-pool ключ в x-goog-api-key и общий баланс." },
    ],
  };
