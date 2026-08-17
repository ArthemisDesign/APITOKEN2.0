import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "apitoken.sale 对比 Anthropic 官方直购",
    h1: "apitoken.sale 对比直接向 Anthropic 购买",
    description: "对比 apitoken.sale 与 Anthropic 官方直购：完全相同的 Messages API 和模型，但统一立省 50%、无需账户、支持银行卡或加密货币支付。",
    keywords: ["claude api 对比 anthropic 官方", "apitoken 对比 anthropic", "anthropic api 替代", "比 anthropic api 更便宜", "claude api 转售"],
    dek: "apitoken.sale 并不是另一套 API——它就是同一套 Anthropic Messages API，从预付余额中以折扣价转售。下面说明真正改变了什么、又没有改变什么。",
    sections: [
      { h2: "保持不变的部分", blocks: [
        { type: "list", items: [
          "同一套 Anthropic Messages API、接口和流式输出。",
          "相同的模型 ID（claude-opus-4-8、claude-sonnet-5、claude-haiku-4-5）。",
          "与你代码已预期的相同的请求与响应格式。",
        ] },
      ] },
      { h2: "发生改变的部分", blocks: [
        { type: "list", items: [
          "价格：B2C 统一比官方消费低 50%。",
          "开通：无需 Anthropic 账户、排队或开票国家要求。",
          "支付：银行卡或加密货币。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "各自适合谁", blocks: [
        { type: "p", text: "如果你已经拥有顺畅的 Anthropic 开票和企业协议，直购或许适合你。如果你想用同样的模型但更便宜、更快上手，并且能用银行卡或加密货币付款，那么 apitoken.sale 是务实之选。" },
      ] },
    ],
    faq: [
      { q: "apitoken.sale 是真正的 Claude API 吗？", a: "是的——它提供同一套 Anthropic Messages API 和模型。只有定价和开通方式不同。" },
      { q: "为什么它比 Anthropic 官方直购更便宜？", a: "余额是预付且汇集的，并对官方消费套用统一 50% 的折扣。" },
    ],
  };
