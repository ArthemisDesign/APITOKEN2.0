import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "GPT API 定价详解",
    h1: "GPT API 定价：输入、缓存、输出与长上下文",
    description: "了解 GPT-5.6 Sol、Terra、Luna 的输入、缓存输入、缓存写入、输出和长上下文价格，以及 apiToken.sale 固定五折。",
    keywords: ["gpt api 定价", "gpt-5.6 价格", "gpt api 成本", "gpt token 价格", "gpt-5.6 sol 价格", "便宜 gpt api"],
    dek: "GPT 成本由精确的 token 项组成，而不是按请求收费。模型层级、缓存 token 和输入长度决定官方费用，apiToken.sale 再减免 50%。",
    sections: [
      { h2: "当前 GPT-5.6 费率", blocks: [
        { type: "table", headers: ["模型", "官方输入 / 缓存 / 输出", "五折后价格"], rows: [
          ["gpt-5.6-sol", "$5 / $0.50 / $30", "$2.50 / $0.25 / $15"],
          ["gpt-5.6-terra", "$2 / $0.20 / $12", "$1 / $0.10 / $6"],
          ["gpt-5.6-luna", "$0.20 / $0.02 / $1.20", "$0.10 / $0.01 / $0.60"],
        ] },
        { type: "p", text: "以上均为每 100 万 token。gpt-5.6 是 gpt-5.6-sol 的别名，价格相同，并非独立费率。" },
      ] },
      { h2: "缓存写入与长上下文", blocks: [
        { type: "list", items: [
          "GPT-5.6 缓存写入按普通输入的 125% 计费，缓存读取按 10% 计费。",
          "输入超过 272K token 后，整次请求使用 2 倍输入和 1.5 倍输出费率。",
          "推理 token 已包含在输出中，不会作为额外项目重复收费。",
          "仪表板记录终态 usage 与折扣后的精确扣费。",
        ] },
        { type: "note", text: "切换层级通常比压缩提示词节省更多：Terra 每 token 是 Sol 的 40%，Luna 仅为 4%。应按任务难度路由。" },
      ] },
    ],
    faq: [
      { q: "GPT-5.6 每 100 万 token 多少钱？", a: "官方 Sol 为 $5 输入/$30 输出，Terra 为 $2/$12，Luna 为 $0.20/$1.20；apiToken.sale 对每个项目固定五折。" },
      { q: "什么是 cached input？", a: "由提供商缓存命中的重复提示前缀。同一个 token 不会同时按缓存输入和新输入收费。" },
      { q: "长上下文费率何时生效？", a: "输入超过 272K token 时，整次请求使用 2 倍输入和 1.5 倍输出费率，然后再应用折扣。" },
    ],
  };
