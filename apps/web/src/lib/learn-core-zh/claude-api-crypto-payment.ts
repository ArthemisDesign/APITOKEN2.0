import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "用加密货币支付 Claude API",
    h1: "用加密货币支付 Claude API",
    description: "在 apitoken.sale 上用加密货币或银行卡购买 Claude API 余额。无需 Anthropic 账户，即时开通，预付余额永不过期。",
    keywords: ["claude api 加密货币支付", "用加密货币买 claude api", "claude api usdt", "加密货币支付 anthropic api", "claude api 比特币"],
    dek: "如果银行卡不是一个选项——或者你就是更偏好加密货币——你可以用加密货币为 Claude API 余额充值并立即开始。",
    sections: [
      { h2: "银行卡或加密货币，任你选择", blocks: [
        { type: "p", text: "结账时你可以通过安全的支付服务商用银行卡或加密货币支付。无论哪种方式，余额都会以预付形式进入你的账户，仅在请求运行时才扣费。" },
      ] },
      { h2: "加密货币为什么有帮助", blocks: [
        { type: "list", items: [
          "无需 Anthropic 支持的计费国家。",
          "在银行卡被拒或不可用的地方很实用。",
          "余额永不过期，因此你充值一次，边构建边扣减。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "结账时会遇到什么", blocks: [
        { type: "p", text: "在结账时选择加密货币，向显示的地址转入金额，网络确认后你的余额即入账。若你更愿意用银行卡进行某笔特定充值，银行卡依然可用。" },
        { type: "list", items: [
          "链上确认后余额入账。",
          "任意整数美元金额；余额永不过期。",
          "每次充值都可在银行卡和加密货币之间切换。",
        ] },
      ] },
      { h2: "可以用哪些加密货币支付", blocks: [
        { type: "p", text: "加密货币充值通过安全的支付服务商处理，因此常见币种都受支持。" },
        { type: "list", items: [
          "USDT 及其他稳定币。",
          "BTC 及主流加密货币。",
          "网络确认交易后余额即入账。",
        ] },
      ] },
    ],
    faq: [
      { q: "支持哪些支付方式？", a: "你可以通过收银服务商用银行卡或加密货币支付。" },
      { q: "余额会过期吗？", a: "不会。预付余额永不过期，仅在真实 API 使用时才消耗。" },
      { q: "我能用 USDT 购买 Claude API 吗？", a: "可以——结账时你可以用 USDT 或其他受支持的加密货币为 Claude API 余额充值。" },
    ],
  };
