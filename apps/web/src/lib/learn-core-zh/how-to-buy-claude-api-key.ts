import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "如何购买 Claude API 密钥",
    h1: "如何购买 Claude API 密钥",
    description: "在 apitoken.sale 上几分钟内买到 Claude API 密钥——一把密钥通用所有 Claude 模型，预付余额，支持银行卡或加密货币支付，无需 Anthropic 账户。",
    keywords: ["购买 claude api 密钥", "如何购买 claude api", "claude api key", "获取 claude api 权限", "anthropic api 密钥"],
    dek: "无需 Anthropic 账户、无需邀请码、也不用公司信用卡即可开始使用 Claude。在 apitoken.sale 上你购买预付余额、生成一把密钥，就能以折扣价调用同一套 Anthropic Messages API。",
    sections: [
      { h2: "三步拿到你的密钥", blocks: [
        { type: "steps", items: [
          "创建一个免费账户并打开控制台——无需审批、无需排队。",
          "生成一把 API 密钥（形如 sk-pool-…）。同一把密钥可用于所有受支持的 Claude 模型。",
          "将任意兼容 Anthropic 的工具指向 https://router.apitoken.sale，并携带 x-api-key 请求头向 /v1/messages 发送请求。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "支付方式如何运作", blocks: [
        { type: "p", text: "想充多少就充多少（整数美元）——没有固定的产品套餐。你的余额为预付制，永不过期，仅在 API 请求实际运行时才会扣费。" },
        { type: "list", items: [
          "通过安全的收银服务商用银行卡或加密货币支付。",
          "每次请求都会先换算为官方 Anthropic API 消费，再套用你当前的折扣。",
          "B2C 账户每个请求统一享受比官方消费低 50% 的折扣。",
        ] },
      ] },
      { h2: "拿到密钥能做什么", blocks: [
        { type: "p", text: "一把密钥即可解锁全部受支持的 Claude 系列——Opus、Sonnet 和 Haiku——覆盖 Claude Code、Cursor、Cline、Continue、Zed 以及官方 Anthropic SDK。协议本身毫无变化，改变的只有价格。" },
      ] },
      { h2: "你能用到哪些 Claude 模型和工具", blocks: [
        { type: "p", text: "一把 Claude API 密钥即可在同一余额下解锁全部受支持的模型系列，并适用于所有兼容 Anthropic 的工具。" },
        { type: "list", items: [
          "模型：Claude Opus 4.8 与 4.7、Sonnet 5 与 4.6、Haiku 4.5。",
          "工具：Claude Code、Cursor、Cline、Continue、Zed 以及 Anthropic SDK。",
          "格式：支持流式输出与工具调用的 Anthropic Messages API。",
        ] },
      ] },
    ],
    faq: [
      { q: "购买 Claude API 密钥需要 Anthropic 账户吗？", a: "不需要。apitoken.sale 自行签发密钥和余额，因此你无需 Anthropic 账户、邀请码或审批即可开始。" },
      { q: "密钥多快能激活？", a: "即时激活。你在控制台生成密钥后，下一次请求即可使用——没有排队，也没有人工审核。" },
      { q: "起步要花多少钱？", a: "你可以充值任意整数美元金额。通过 Google 或 GitHub 创建的新账户还会获得 $5 平台欢迎奖励余额。" },
      { q: "这是官方的 Claude API 吗？", a: "是的——它提供同一套 Anthropic Messages API 和同样的 Claude 模型。不同的只有价格以及注册和付款方式。" },
    ],
  };
