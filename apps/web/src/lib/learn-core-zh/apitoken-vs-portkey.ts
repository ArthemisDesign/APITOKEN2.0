import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "apiToken.sale 与 Portkey 对比（Claude）",
    h1: "apiToken.sale 与 Portkey 对比",
    description: "Portkey 是一款使用你自有厂商密钥进行路由与可观测的 AI 网关。apiToken.sale 则直接提供折扣价的 Claude 密钥和余额。两者分别在什么时候用，看这篇。",
    keywords: ["portkey 替代方案", "ai 网关 claude", "claude api 网关", "portkey claude api", "claude 密钥折扣"],
    dek: "这两款工具解决的是不同的问题。Portkey 位于你已拥有的厂商密钥之前；而 apiToken.sale 正是折扣 Claude 密钥和余额的来源。",
    sections: [
      { h2: "各司其职", blocks: [
        { type: "p", text: "Portkey 在你自带的 API 密钥之上增加路由、缓存和可观测能力。它并不向你出售 Claude 权限或折扣——背后你仍需一个已充值的 Anthropic 账户。" },
        { type: "p", text: "apitoken.sale 才是密钥和余额的来源：一个位于 https://router.apitoken.sale 的原生 Anthropic 端点，统一立省 50%，且无需 Anthropic 账户。" },
      ] },
      { h2: "两者甚至可以组合", blocks: [
        { type: "p", text: "如果你喜欢 Portkey 的可观测能力，可以把 apiToken.sale 密钥设为它的 Anthropic 厂商，从而在底层享受折扣。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "Portkey 会给我 Claude 折扣吗？", a: "不会——Portkey 只是覆盖在你自有密钥之上的网关。折扣 Claude 密钥和余额由 apiToken.sale 提供。" },
      { q: "两者能一起用吗？", a: "能。把 apiToken.sale 密钥作为 Portkey 的 Anthropic 厂商，既保留可观测能力又能少花钱。" },
    ],
  };
