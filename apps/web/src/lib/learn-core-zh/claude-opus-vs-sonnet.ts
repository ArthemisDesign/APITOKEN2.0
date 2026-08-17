import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude Opus 对比 Sonnet——该用哪个",
    h1: "Claude Opus 对比 Sonnet：该用哪个模型",
    description: "Opus 还是 Sonnet？为编码和智能体挑选合适 Claude 模型的实用指南——并在一把 apitoken.sale 密钥和余额上同时使用两者。",
    keywords: ["claude opus 对比 sonnet", "该用哪个 claude 模型", "opus 还是 sonnet 编码", "最佳 claude 模型", "claude 模型对比"],
    dek: "Opus 和 Sonnet 解决不同的问题。选对模型是获得更好结果、少花 token 的最简单方式——而且你可以在一把密钥上同时保留两者。",
    sections: [
      { h2: "默认使用 Sonnet", blocks: [
        { type: "p", text: "Sonnet 5 和 Sonnet 4.6 能又快又省地处理绝大多数编码和智能体工作。从这里开始。" },
      ] },
      { h2: "遇到难题再升级到 Opus", blocks: [
        { type: "p", text: "在复杂重构、架构设计以及额外推理物有所值的长时间高风险会话中，就该选 Opus 4.8。" },
        { type: "note", text: "由于一把密钥同时覆盖两者，你可以为每项任务路由到合适的档位，而无需在多个提供方之间来回切换。" },
        { type: "table", headers: ["", "Claude Opus 4.8", "Claude Sonnet 5"], rows: [
          ["官方价格（输入 / 输出 / 1M）", "$5 / $25", "$3 / $15"],
          ["本站（−50%）", "$2.50 / $12.50", "$1.50 / $7.50"],
          ["上下文窗口", "1M token", "1M token"],
          ["最适合", "高难推理、长程智能体运行", "日常编码与智能体"],
        ] },
        { type: "link", text: "比较所有 Claude 模型与价格", href: "/models" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "哪个更适合编码？", a: "Sonnet 是日常编码推荐的默认模型；复杂推理和长时间重构则使用 Opus。" },
      { q: "我能在一个账户上同时使用两者吗？", a: "可以。Opus、Sonnet 和 Haiku 都共用同一把密钥和预付余额。" },
    ],
  };
