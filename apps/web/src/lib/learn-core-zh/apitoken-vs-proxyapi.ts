import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude 场景下 apiToken.sale 与 ProxyAPI 对比",
    h1: "apiToken.sale 与 ProxyAPI 对比",
    description: "对比 Claude API 转售商：apiToken.sale 提供原生 Anthropic 端点、统一 50% 折扣、银行卡或加密货币支付，一把密钥通用所有模型。",
    keywords: ["proxyapi 替代品", "apitoken 对比 proxyapi", "claude api 转售", "proxyapi claude", "不用 proxyapi 用 claude api"],
    dek: "两者都能让你无需 Anthropic 账户就用上 Claude。差别在于付款方式、能省多少，以及端点是否真正 Anthropic 原生。",
    sections: [
      { h2: "原生 Anthropic 端点", blocks: [
        { type: "p", text: "apiToken.sale 在 https://router.apitoken.sale 上暴露标准的 Anthropic Messages API，因此 Claude Code、Cursor 和 Anthropic SDK 无需改动即可使用——你与 Claude 之间没有一层适配层。" },
      ] },
      { h2: "是折扣，不是加价", blocks: [
        { type: "list", items: [
          "B2C 统一折扣，官方 Claude 消费立省 50%。",
          "一把预付密钥、一份余额，通用 Opus、Sonnet 和 Haiku。",
          "银行卡或加密货币充值，永不过期。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "分别适合什么", blocks: [
        { type: "list", items: [
          "apiToken.sale——带统一折扣、密钥终身累计消费上限和可选到期日期的原生 Anthropic 端点。",
          "通用转售商——如果你已经在用它的其他提供方，可能适合你。",
          "两者都移除了 Anthropic 账户门槛；差别在于价格，以及 Claude 接入有多原生。",
        ] },
      ] },
    ],
    faq: [
      { q: "apiToken.sale 比普通转售商更便宜吗？", a: "它对官方 Claude 消费套用统一 50% 的折扣，而不是在标价之上再加价。" },
      { q: "我的 Anthropic 工具还能用吗？", a: "能——它是原生的 Anthropic Messages API，因此 Claude Code、Cursor 和 SDK 只需改一下 Base URL。" },
    ],
  };
