import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude 场景下 apiToken.sale 与 OpenRouter 对比",
    h1: "Claude 场景下 apiToken.sale 与 OpenRouter 对比",
    description: "在选择 Claude 网关？对比 apiToken.sale 与 OpenRouter：原生 Anthropic 端点加预付折扣，对比多提供方路由器。",
    keywords: ["openrouter 替代品", "apitoken 对比 openrouter", "claude api 网关", "openrouter claude", "最佳 claude api 网关"],
    dek: "两者都能让你无需 Anthropic 账户就用上 Claude，但架构不同。如果 Claude 是你的主力模型，原生 Anthropic 端点会让一切更简单。",
    sections: [
      { h2: "原生 Anthropic 端点", blocks: [
        { type: "p", text: "apiToken.sale 在 https://router.apitoken.sale 上暴露标准的 Anthropic Messages API，因此 Claude Code、Cursor 和 Anthropic SDK 都无需任何适配器即可使用。你不必经过一层通用的多提供方抽象。" },
      ] },
      { h2: "是预付折扣，不是加价", blocks: [
        { type: "list", items: [
          "B2C 统一折扣，官方 Claude 消费立省 50%。",
          "一把密钥、一份余额，通用 Opus、Sonnet 和 Haiku。",
          "银行卡或加密货币充值，永不过期。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "分别适合什么时候用", blocks: [
        { type: "list", items: [
          "apiToken.sale——Claude 是你的主力模型，你想要一个带折扣的原生 Anthropic 端点。",
          "OpenRouter——你需要在一层抽象后路由到众多提供方。",
          "两者都能让你无需 Anthropic 账户即可开始；但只有 apiToken.sale 直接对 Claude 消费打折。",
        ] },
      ] },
    ],
    faq: [
      { q: "为什么要选 Claude 原生网关？", a: "如果 Claude 是你的主力模型，原生 Anthropic 端点意味着你现有的 Anthropic 工具和 SDK 无需改动即可使用。" },
      { q: "apiToken.sale 会加价吗？", a: "不会——它对官方 Claude 消费打折，而不是在标价之上加价。" },
    ],
  };
