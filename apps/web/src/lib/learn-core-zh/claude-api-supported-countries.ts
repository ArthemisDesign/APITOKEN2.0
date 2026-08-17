import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API 支持的国家/地区",
    h1: "你可以在哪里使用 apitoken.sale",
    description: "apitoken.sale 全球可用，无 Anthropic 计费国家要求。用银行卡或加密货币支付，即可在 Anthropic 不直接服务的地区使用 Claude API。",
    keywords: ["claude api 支持的国家", "claude api 全球可用", "anthropic api 国家限制", "claude api 可用地区"],
    dek: "由于我们自行签发密钥和余额，因此没有 Anthropic 计费国家的门槛。这让身处直接注册困难地区的开发者也能用上 Claude API。",
    sections: [
      { h2: "无计费国家门槛", blocks: [
        { type: "list", items: [
          "无需 Anthropic 账户或受支持的计费国家。",
          "支持银行卡和加密货币支付。",
          "通过 Telegram 提供英语和俄语支持。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "跨地区的支付方式", blocks: [
        { type: "p", text: "由于我们签发密钥和余额，你不受 Anthropic 支持的计费国家约束。在可用的地区用银行卡支付，或在银行卡被拒的地区用加密货币支付。" },
        { type: "list", items: [
          "无需 Anthropic 计费国家。",
          "结账时可用银行卡或加密货币。",
          "通过 Telegram 提供英语和俄语支持。",
        ] },
      ] },
    ],
    faq: [
      { q: "我所在的国家能用 Claude API 吗？", a: "apitoken.sale 没有计费国家要求，因此你可以在 Anthropic 不直接计费的地区购买余额并使用密钥。" },
      { q: "支付限制怎么办？", a: "你可以用银行卡或加密货币支付，这在银行卡不可用的地区很有帮助。" },
    ],
  };
