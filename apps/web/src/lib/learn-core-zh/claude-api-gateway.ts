import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "什么是 Claude API 网关？",
    h1: "Claude API 网关是什么",
    description: "Claude API 网关位于你的工具和 Anthropic 之间，增加接入、计费和管控能力。apitoken.sale 是一个带统一 50% 折扣的原生网关。",
    keywords: ["claude api 网关", "什么是 api 网关", "anthropic 网关", "claude 代理", "claude api 接入层"],
    dek: "网关是介于你的代码和模型提供方之间的一层薄薄的中间层。好的 Claude 网关对你的工具是透明的，同时改善接入、价格和管控。",
    sections: [
      { h2: "网关做什么", blocks: [
        { type: "list", items: [
          "对外呈现标准的 Anthropic Messages API，让工具无需改动即可使用。",
          "处理接入和计费——在这里，就是折扣预付余额。",
          "增加按密钥的终身累计消费上限、可选到期日期和用量可见性。",
        ] },
      ] },
      { h2: "原生，而非转译层", blocks: [
        { type: "p", text: "apiToken.sale 是 Anthropic 原生的：把任意客户端指向 https://router.apitoken.sale/v1/messages，它的表现与 api.anthropic.com 完全一致——再加上你的折扣和控制台管控。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "挑选网关时看什么", blocks: [
        { type: "list", items: [
          "原生 Anthropic API，让工具和 SDK 无需改动即可使用。",
          "透明的按 token 计费，可在控制台审计。",
          "按密钥的管控：可选的终身累计消费上限和到期日期。",
          "无绑定——预付余额永不过期。",
        ] },
      ] },
    ],
    faq: [
      { q: "网关会改变 API 吗？", a: "不会。原生 Claude 网关讲的是标准的 Anthropic Messages API，因此你的工具和 SDK 无需改动。" },
      { q: "为什么用网关而不直接用 Anthropic？", a: "为了折扣、无需 Anthropic 账户即可即时开通，以及为单独密钥设置可选的终身累计消费上限和到期日期。" },
    ],
  };
