import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude Haiku API 访问",
    h1: "通过 API 使用 Claude Haiku 4.5",
    description: "通过 apitoken.sale 访问 Claude Haiku 4.5——最快、最经济的 Claude 模型，以预付折扣价理想应对高并发和低延迟任务。",
    keywords: ["claude haiku api", "claude haiku 4.5 api", "最快的 claude 模型", "便宜的 claude 模型", "haiku api 密钥"],
    dek: "Haiku 为速度和吞吐量而生：分类、抽取、路由以及任何延迟和成本比深度推理更重要的任务。",
    sections: [
      { h2: "何时该选 Haiku", blocks: [
        { type: "list", items: [
          "高并发、低延迟的请求。",
          "廉价的后台任务和预处理。",
          "在无需 Opus 的工作上让余额撑得更久。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "一把密钥混用多种模型", blocks: [
        { type: "p", text: "由于所有模型共用一把密钥和余额，你可以把廉价工作路由给 Haiku（claude-haiku-4-5），只把高难度请求升级到 Sonnet 或 Opus。" },
        { type: "table", headers: ["模型", "官方 输入 / 输出（$ / 1M）", "本站（−50%）"], rows: [
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "link", text: "Claude Haiku 4.5 详细价格（缓存、上下文、FAQ）", href: "/models/claude-haiku-4-5" },
      ] },
    ],
    faq: [
      { q: "Haiku 有多快、多便宜？", a: "Haiku 4.5 是速度最快、成本最低的 Claude 模型，非常适合高并发、对延迟敏感的工作。" },
      { q: "我能把 Haiku 与其他模型组合使用吗？", a: "可以。一把密钥和余额覆盖 Haiku、Sonnet 和 Opus，因此你能为每项任务路由到性价比最高的模型。" },
    ],
  };
