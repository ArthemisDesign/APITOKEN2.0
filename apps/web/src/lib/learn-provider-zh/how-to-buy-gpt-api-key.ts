import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "如何购买 GPT API 密钥",
    h1: "如何购买 GPT API 密钥",
    description: "购买预付费 GPT API 密钥，支持银行卡或加密货币付款，通过 OpenAI 兼容端点使用 GPT-5.6、GPT-5.5 和 GPT Image 2，官方费用五折。",
    keywords: ["购买 gpt api 密钥", "gpt api 密钥", "购买 openai api", "gpt-5.6 api", "openai 兼容 api", "预付费 gpt api"],
    dek: "一个 apiToken.sale 密钥即可使用 GPT 目录，无需单独的 OpenAI Platform 账户。充值后设置 OpenAI 兼容端点，每次请求按官方费用的 50% 结算。",
    sections: [
      { h2: "三步获取 GPT 密钥", blocks: [
        { type: "steps", items: [
          "创建 apiToken.sale 账户，在仪表板生成密钥。",
          "使用银行卡或加密货币充值任意整数美元，无固定套餐或月费。",
          "将 base URL 设为 https://router.apitoken.sale/v1，使用 Authorization: Bearer，并从 GET /v1/models 选择模型。",
        ] },
        sourceBlock("how-to-buy-gpt-api-key", 0, 1),
      ] },
      { h2: "密钥包含哪些能力", blocks: [
        { type: "list", items: [
          "Responses 与 Chat Completions，均支持增量 SSE 流。",
          "GPT-5.6 Sol、Terra、Luna、旧版 GPT，以及独立的 GPT Image 2 路由。",
          "同一密钥和余额也可用于支持的 Claude、Gemini 与 Kimi 模型。",
          "每次请求均按官方费用享受固定 50% B2C 折扣。",
        ] },
        { type: "note", text: "请把密钥放在服务端环境变量中。GPT 使用 Authorization: Bearer；x-api-key 与 x-goog-api-key 分别属于 Anthropic 和 Gemini 协议。" },
      ] },
    ],
    faq: [
      { q: "需要 OpenAI 账户吗？", a: "不需要。密钥、余额和计费都由 apiToken.sale 提供，客户端只需自定义 base URL 和 Bearer 密钥。" },
      { q: "一个密钥能同时调用 GPT 和 Claude 吗？", a: "可以。同一个 sk-pool 密钥和余额覆盖所有支持的提供商，只需切换端点和认证头。" },
      { q: "这是 OpenAI Platform 吗？", a: "不是。这是独立的 OpenAI 兼容网关，拥有自己的账户、预付余额和模型目录。" },
    ],
  };
