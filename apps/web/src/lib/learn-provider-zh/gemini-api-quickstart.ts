import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Gemini API 快速入门",
    h1: "Gemini API 快速入门：curl 与 Google GenAI SDK",
    description: "通过 curl 或 Google GenAI SDK 发起首个 Gemini API 请求：原生 generateContent、x-goog-api-key 和明确的 Gemini model ID。",
    keywords: ["gemini api 快速入门", "gemini api 教程", "google genai sdk base url", "gemini generatecontent", "gemini api curl", "gemini api 示例"],
    dek: "网关保留原生 Google Gemini 协议。只需修改 base URL 和 API key，继续使用 generateContent 与官方 SDK 结构，并始终明确指定模型。",
    sections: [
      { h2: "使用 curl 发起首个请求", blocks: [
        sourceBlock("gemini-api-quickstart", 0, 0),
        { type: "p", text: "增量输出使用 streamGenerateContent?alt=sse。生成前可在同一模型路径调用 countTokens，免费估算输入 token。" },
      ] },
      { h2: "使用官方 Python SDK", blocks: [
        sourceBlock("gemini-api-quickstart", 1, 0),
        { type: "list", items: [
          "SDK 配置只传裸 base URL，不要附加 /v1beta。",
          "明确传入 model ID；客户端自动默认模型可能不在网关目录中。",
          "把 APITOKEN_API_KEY 放在环境变量中，不要写入源码。",
        ] },
      ] },
    ],
    faq: [
      { q: "官方 Google GenAI SDK 能用吗？", a: "可以。将 HttpOptions(base_url) 设为 https://router.apitoken.sale 并提供 apiToken.sale 密钥，请求与响应结构保持原生。" },
      { q: "如何流式输出 Gemini？", a: "使用 /v1beta/models/{model}:streamGenerateContent?alt=sse 与 x-goog-api-key，或 SDK 对应的流式方法。" },
      { q: "为什么重复 /v1beta 会 404？", a: "Google SDK 会自动添加 API 版本。只配置裸域名，最终 URL 中应只有一个 /v1beta。" },
    ],
  };
