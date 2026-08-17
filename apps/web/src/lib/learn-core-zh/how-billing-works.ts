import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "apitoken.sale 的计费如何运作",
    h1: "计费如何运作",
    description: "了解 apitoken.sale 的计费：预付余额、按官方费率的逐次请求计量、你的统一折扣，以及控制台中的 token 级用量。",
    keywords: ["多提供商 api 计费", "claude api 计费", "gpt api 计费", "gemini api 计费", "kimi api 计费", "预付 api 余额"],
    dek: "计费采用透明预付模式。Claude、GPT、Gemini 与 Kimi 请求按各自官方费率精确计量、应用折扣后，从同一余额扣除，并提供可审计明细。",
    sections: [
      { h2: "预付余额", blocks: [
        { type: "p", text: "你可以充值任意整数美元金额。余额永不过期，无需客户订阅；支持的 Claude、GPT、Gemini 与 Kimi 共用这一余额。" },
      ] },
      { h2: "逐次请求计量", blocks: [
        { type: "list", items: [
          "每次调用按所属提供商的精确用量项换算为官方消费，包括输入、输出、缓存、长上下文与图像。",
          "所有支持的提供商统一减去 B2C 50% 折扣。",
          "净额从你的预付余额中扣除。",
        ] },
      ] },
      { h2: "完全可见", blocks: [
        { type: "p", text: "每次请求都在控制台中显示，含输入、输出、缓存和思考 token，因此你始终清楚余额去向。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "计费是预付还是后付？", a: "预付。你预先充入一份余额，请求从中扣减；没有月度账单。" },
      { q: "一份余额能覆盖 Claude、GPT、Gemini 与 Kimi 吗？", a: "能。每个提供商按自己的官方费率表计量，再应用同一 B2C 折扣，最终费用从同一份预付余额扣除。" },
      { q: "我能看到 token 级用量吗？", a: "可以。控制台会按模型、提供商和 token 桶分解显示用量。" },
    ],
  };
