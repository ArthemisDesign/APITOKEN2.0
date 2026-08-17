import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "如何购买 Kimi API 密钥",
    h1: "如何购买 Kimi API 密钥",
    description: "购买一个预付费 API 密钥，通过 Anthropic Messages 或 OpenAI 兼容客户端使用 Kimi K3 和 Kimi for Coding，官方 API 费用五折。",
    keywords: ["购买 kimi api 密钥", "kimi api 密钥", "kimi k3 api", "kimi for coding api", "moonshot kimi api", "预付费 kimi api"],
    dek: "Kimi 以独立模型命名空间发布在统一路由器上。可使用原生 Anthropic Messages 路由或 OpenAI 兼容客户端，并与 Claude、GPT、Gemini 共享预付余额。",
    sections: [
      { h2: "三步获取访问权限", blocks: [
        { type: "steps", items: [
          "创建 apiToken.sale 账户并生成 sk-pool 密钥。",
          "使用银行卡或加密货币充值任意整数美元，用户侧无需另购 Kimi 套餐。",
          "读取 GET https://router.apitoken.sale/v1/models，从密钥的实时目录选择 kimi/* ID。",
        ] },
        sourceBlock("how-to-buy-kimi-api-key", 0, 1),
      ] },
      { h2: "Kimi 路由有何不同", blocks: [
        { type: "list", items: [
          "Kimi 是独立提供商命名空间，而不是第四种 wire format：可使用 POST /v1/messages 与 x-api-key，或统一 OpenAI 兼容 /v1 路由。",
          "公开 ID 是 kimi/k3、kimi/kimi-for-coding 等订阅别名，不是内部费率模型名。",
          "K3 有 256K 与 1M 上下文写法，Kimi for Coding 有普通与 High Speed 别名。",
          "实时 /v1/models 是权威来源，因为可用性受提供商容量和密钥策略影响。",
        ] },
      ] },
    ],
    faq: [
      { q: "Kimi 需要单独 API 密钥吗？", a: "不需要。同一个 sk-pool 密钥和余额覆盖 Kimi 与其他支持的提供商。" },
      { q: "Kimi 使用哪个端点？", a: "Anthropic Messages 使用 https://router.apitoken.sale/v1/messages；OpenAI 兼容客户端使用 /v1 Chat Completions。两者都接受公开 kimi/* ID。" },
      { q: "为什么先检查 /v1/models？", a: "目录按密钥作用域返回当前可路由且可定价的模型。" },
    ],
  };
