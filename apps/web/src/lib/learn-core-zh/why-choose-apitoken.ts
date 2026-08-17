import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "为什么选择 apiToken.sale",
    h1: "为什么选择 apiToken.sale",
    description: "为什么开发者用一个 apiToken.sale 密钥访问 Claude、GPT、Gemini 与 Kimi：原生或兼容 API、B2C 五折，以及银行卡或加密货币付款。",
    keywords: ["为什么选 apitoken.sale", "多提供商 api", "claude api 折扣", "gpt api 折扣", "gemini api 折扣", "kimi api 密钥"],
    dek: "apiToken.sale 用一个密钥和预付余额连接四个提供商系列，同时保留每种客户端所需的原生或兼容协议。",
    sections: [
      { h2: "一句话版本", blocks: [
        { type: "list", items: [
          "Claude 与 Kimi 使用 Anthropic Messages；GPT 与多提供商客户端（包括 Kimi）可用 OpenAI 兼容路由；Gemini 保留原生 generateContent。",
          "在永不过期的预付余额上，所有支持模型的官方消费统一享受 B2C 五折。",
          "即时、自助开通，无需分别配置 Anthropic、OpenAI、Google Cloud 或 Kimi 计费账户。",
          "支持银行卡或加密货币付款。",
          "每把密钥可选终身累计消费上限和到期日期，并在控制台查看 token 级用量明细。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "同一余额上的折扣 API token", blocks: [
        { type: "p", text: "一次充值，同一余额即可按 B2C 五折使用支持的 Claude、GPT、Gemini 与 Kimi 模型。余额永不过期，也没有客户订阅。" },
      ] },
    ],
    faq: [
      { q: "apiToken.sale 有什么不同？", a: "一个密钥和余额覆盖四个提供商系列，并统一享受 B2C 五折；客户端仍使用适合该提供商的原生或兼容协议。" },
      { q: "所有提供商都会转换成同一种 API 吗？", a: "不会。Claude 与 Kimi 保留 Anthropic Messages，GPT 使用 OpenAI 兼容路由，Gemini 保持 Google 原生结构；需要 OpenAI 形状的客户端也可通过统一路由调用 Kimi。" },
      { q: "apiToken.sale 是什么？", a: "一个独立的多提供商 API 网关，为支持的 Claude、GPT、Gemini 与 Kimi 提供折扣预付访问，无需分别开通提供商计费账户。" },
    ],
  };
