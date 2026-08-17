import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "从俄罗斯及受限地区使用 Claude API",
    h1: "在俄罗斯使用 Claude API",
    description: "通过 apitoken.sale 从俄罗斯及其他受限地区访问 Claude API——无需 Anthropic 账户，支持银行卡或加密货币支付，一把密钥通用所有 Claude 模型。",
    keywords: ["俄罗斯 claude api", "从俄罗斯使用 claude api", "anthropic api 俄罗斯", "claude api 受限地区", "claude api 支付", "claude api 免翻墙"],
    dek: "Anthropic 并非在每个国家都直接销售，这让俄罗斯及其他地区的开发者缺乏明确的付款途径。apitoken.sale 消除了这道障碍：你购买预付余额即可拿到一把可用密钥，无论 Anthropic 在哪里开票。",
    sections: [
      { h2: "为什么直接访问很难", blocks: [
        { type: "p", text: "在 Anthropic 注册通常要求受支持的开票国家和支付方式。如果你无法完成这一步，就拿不到密钥——即便模型本身在网络上是可达的。" },
      ] },
      { h2: "apitoken.sale 如何解决", blocks: [
        { type: "list", items: [
          "无需 Anthropic 账户——密钥和余额由我们签发。",
          "用银行卡或加密货币支付，哪种方便用哪种。",
          "即时激活，无需排队，无需公司核验。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "与你现有的工具兼容", blocks: [
        { type: "p", text: "将 Claude Code、Cursor、Cline 或 Anthropic SDK 指向 https://router.apitoken.sale，即可像以前一样继续工作。支持提供俄语和英语服务，通过 Telegram 联系。" },
      ] },
      { h2: "在俄罗斯免 VPN 使用 Claude API", blocks: [
        { type: "p", text: "签发密钥和余额没有 Anthropic 开票国家的门槛，因此你不需要外国银行卡或公司即可开始。网络可达性取决于你自己的连接，但购买余额和生成密钥都没有地域限制。" },
      ] },
    ],
    faq: [
      { q: "我能从俄罗斯付款吗？", a: "可以。你可以通过收银服务商用银行卡或加密货币支付，因此不要求受支持的 Anthropic 开票国家。" },
      { q: "我需要 VPN 吗？", a: "你无需 Anthropic 账户或开票国家。网络可达性取决于你自己的连接，但签发密钥和余额没有地域限制。" },
      { q: "有俄语支持吗？", a: "有——支持提供俄语和英语服务，通过 Telegram 联系。" },
      { q: "我能从俄罗斯为 Claude API 付款吗？", a: "可以——用银行卡或加密货币支付，因此不要求受支持的 Anthropic 开票国家。" },
    ],
  };
