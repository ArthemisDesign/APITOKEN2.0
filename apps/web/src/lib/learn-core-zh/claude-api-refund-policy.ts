import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API 退款政策",
    h1: "退款与支持",
    description: "了解 apitoken.sale 如何处理余额、退款和支持。预付余额永不过期，并通过 Telegram 提供英语和俄语支持。",
    keywords: ["claude api 退款", "apitoken 退款政策", "claude api 支持", "claude api 退钱", "claude api 帮助"],
    dek: "预付余额的设计就是为了低风险：它永不过期，你只为实际调用的部分付费，而支持只需一条消息即可触达。",
    sections: [
      { h2: "余额与退款", blocks: [
        { type: "p", text: "由于余额为预付制且永不过期，未使用的资金会一直保留供日后使用。退款通过原支付渠道处理；请带上你的账户信息联系支持。" },
      ] },
      { h2: "获取帮助", blocks: [
        { type: "p", text: "支持通过 Telegram 提供英语和俄语服务，也可发邮件至 apitokensale@gmail.com。大多数集成问题都能很快得到解答。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "充值与余额如何运作", blocks: [
        { type: "p", text: "你以任意整数美元金额充值余额，且仅在请求运行时才扣减。由于它永不过期，没什么理由超额充值——用多少充多少即可。" },
        { type: "list", items: [
          "预付、永不过期的余额。",
          "退款通过原支付渠道处理。",
          "用你的账户邮箱联系支持以获取帮助。",
        ] },
      ] },
    ],
    faq: [
      { q: "我的余额会过期吗？", a: "不会。预付余额永不过期，仅在真实 API 使用时才消耗。" },
      { q: "我该如何联系支持？", a: "通过 Telegram 以英语或俄语联系支持，或发邮件至 apitokensale@gmail.com。" },
    ],
  };
