import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API 定价详解",
    h1: "Claude API 定价如何运作",
    description: "了解 Claude API 定价：按 token 的输入与输出费率、提示缓存，以及 apitoken.sale 如何套用统一 50% 的折扣。",
    keywords: ["claude api 定价", "claude api 成本", "claude api 定价如何运作", "claude token 定价", "anthropic api 定价详解"],
    dek: "Claude 按 token 计费——输入和输出分别计价——对缓存内容有折扣。apitoken.sale 保持这些机制完全一致，并在其上叠加一层折扣。",
    sections: [
      { h2: "Token、输入与输出", blocks: [
        { type: "p", text: "每次请求都按输入 token（你的提示和上下文）和输出 token（模型的回复）计量。输出 token 通常比输入更贵，更大的模型每 token 成本更高。" },
      ] },
      { h2: "缓存与思考", blocks: [
        { type: "list", items: [
          "缓存写入和缓存读取分别计量，且缓存读取便宜得多。",
          "在重推理调用中，思考 token 计入输出。",
          "流式与非流式请求的计费方式相同。",
        ] },
      ] },
      { h2: "apitoken.sale 的折扣", blocks: [
        { type: "p", text: "每次调用先换算为官方 Anthropic 消费，再减去你的折扣：B2C 每个请求统一减去 50%。每次请求都在控制台中以 token 级别的明细可见。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "按模型划分的 Claude API token 价格", blocks: [
        { type: "p", text: "更大的模型每 token 更贵：Opus 是高端档，Sonnet 是均衡的默认选择，Haiku 最便宜。你的折扣适用于所有模型，因此排序不变，但每个价格都更低。" },
        { type: "table", headers: ["模型", "官方 输入 / 输出（$ / 1M）", "本站（−50%）"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "link", text: "含缓存费率与上下文窗口的模型页面", href: "/models" },
      ] },
    ],
    faq: [
      { q: "Claude API 如何定价？", a: "按 token 计费，分为输入和输出，缓存读取另有更便宜的费率。更大的模型每 token 成本更高。" },
      { q: "折扣如何套用？", a: "先计算官方消费，再在扣减余额前减去你的 B2C 统一 50% 折扣。" },
      { q: "Claude API 的 token 如何计价？", a: "按 token 计费，分输入和输出，缓存读取更便宜。apiToken.sale 在官方 token 费率之上再套用你 50% 的统一折扣。" },
    ],
  };
