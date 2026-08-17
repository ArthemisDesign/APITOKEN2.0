import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Как использовать apiToken.sale в Kimi Code",
    h1: "Kimi, Claude, GPT и Gemini в Kimi Code",
    description: "Подключите Kimi Code к apiToken.sale через OpenAI-совместимый provider config, объявите namespaced-модель и защитите API-ключ в config.toml.",
    keywords: ["kimi code api", "kimi code custom provider", "kimi code config toml", "kimi code api ключ", "kimi code k3", "kimi code openai compatible"],
    dek: "Kimi Code принимает custom OpenAI-совместимый provider, поэтому одна запись apiToken.sale достигает единого каталога. Каждую модель нужно объявить отдельно с настоящим namespace и проверенным размером контекста.",
    sections: [
      { h2: "Установите CLI и объявите provider", blocks: [
        sourceBlock("kimi-api-for-kimi-code", 0, 0),
        { type: "note", text: "Не запускайте /login: он привяжет CLI к Kimi membership. Custom provider credentials Kimi Code хранит только в config.toml, поэтому файл содержит ключ в открытом виде и должен быть защищён." },
      ] },
      { h2: "Запустите, проверьте и добавьте модели", blocks: [
        sourceBlock("kimi-api-for-kimi-code", 1, 0),
        { type: "list", items: [
          "/status должен показывать https://router.apitoken.sale/v1 как base URL провайдера.",
          "Поле model использует namespace единого каталога: например kimi/k3, openai/gpt-5.6-terra или google/gemini-3.6-flash.",
          "Объявляйте каждую дополнительную модель в config.toml с проверенным max_context_size — по нему Kimi Code решает, когда сжимать контекст.",
        ] },
      ] },
    ],
    faq: [
      { q: "Kimi Code работает с ключом apiToken.sale?", a: "Да. Добавьте OpenAI-совместимый provider с base_url https://router.apitoken.sale/v1 и сохраните ключ в config.toml Kimi Code." },
      { q: "Kimi Code может запускать не только Kimi?", a: "Да. Та же запись provider достигает единого каталога; объявите каждую Claude, GPT, Gemini или Kimi модель с namespaced ID и правильным лимитом контекста." },
      { q: "Зачем нужен chmod 600?", a: "Kimi Code не читает custom-provider credentials из shell. Сырой API-ключ лежит в config.toml, поэтому файл должен читаться только вашим аккаунтом." },
    ],
  };
