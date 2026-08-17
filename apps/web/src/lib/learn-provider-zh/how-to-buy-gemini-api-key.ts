import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "如何购买 Gemini API 密钥",
    h1: "如何购买 Gemini API 密钥",
    description: "购买预付费 Gemini API 密钥，支持银行卡或加密货币付款，使用原生 Gemini 端点，并以一个账户调用 Gemini、GPT、Claude 和 Kimi，官方费用五折。",
    keywords: ["购买 gemini api 密钥", "gemini api 密钥", "google gemini api", "预付费 gemini api", "gemini api 付款", "便宜 gemini api"],
    dek: "apiToken.sale 密钥无需单独配置 Google Cloud 计费即可访问原生 Gemini API。充值一次，密钥通过 x-goog-api-key 发送，并与所有支持的提供商共享余额。",
    sections: [
      { h2: "三步获取 Gemini 密钥", blocks: [
        { type: "steps", items: [
          "创建 apiToken.sale 账户，在仪表板生成 sk-pool 密钥。",
          "使用银行卡或加密货币充值任意整数美元，余额不会过期。",
          "将 Gemini base URL 设为 https://router.apitoken.sale，通过 x-goog-api-key 认证，并从 GET /v1beta/models 选择模型。",
        ] },
        sourceBlock("how-to-buy-gemini-api-key", 0, 1),
      ] },
      { h2: "可用能力", blocks: [
        { type: "list", items: [
          "原生 Gemini 协议上的 Pro、Flash 和 Flash-Lite 文本模型。",
          "Gemini 3.1 Flash Image（Nano Banana 2）图像生成。",
          "Google 形状的 generateContent、streamGenerateContent 和 countTokens。",
          "固定 50% B2C 折扣，并与 GPT、Claude、Kimi 共用密钥和余额。",
        ] },
        { type: "note", text: "Google SDK 的 base URL 应填写裸域名。SDK 会自行附加 /v1beta；重复前缀会返回 404。" },
      ] },
    ],
    faq: [
      { q: "需要 Google Cloud 项目吗？", a: "不需要。网关账户和计费由 apiToken.sale 管理，客户端只需自定义 base URL 和 sk-pool 密钥。" },
      { q: "Gemini 使用哪个认证头？", a: "x-goog-api-key。原生 Gemini 路由不使用 Anthropic x-api-key 或 OpenAI Authorization: Bearer。" },
      { q: "同一密钥能调用 GPT 与 Gemini 吗？", a: "可以。密钥和余额共享，只需按提供商切换端点、协议和 model ID。" },
    ],
  };
