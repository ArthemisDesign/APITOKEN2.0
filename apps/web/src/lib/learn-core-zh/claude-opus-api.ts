import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude Opus API 访问",
    h1: "通过 API 使用 Claude Opus 4.8",
    description: "通过一把 apitoken.sale 密钥以统一低于官方费率 50% 的价格访问 Claude Opus 4.8 和 4.7。最适合复杂推理、重构与长时间的智能体会话。",
    keywords: ["claude opus api", "claude opus 4.8 api", "opus api 密钥", "claude opus 价格", "claude opus 折扣"],
    dek: "Opus 是 Claude 能力最强的档位——面对高难度推理、架构设计和长时间智能体运行时应当选它。apitoken.sale 让你在与其他模型相同的密钥和余额上使用 Opus 4.8 和 4.7。",
    sections: [
      { h2: "何时使用 Opus", blocks: [
        { type: "list", items: [
          "复杂重构与跨文件改动。",
          "架构设计、规划与高风险推理。",
          "对一致性和缓存复用要求高的长时间会话。",
        ] },
      ] },
      { h2: "在你的余额上使用 Opus", blocks: [
        { type: "p", text: "Opus 4.8（模型 ID claude-opus-4-8）和 Opus 4.7 按官方 token 费率减去你的折扣计费，因此你能以标价的一小部分用上顶级档位。" },
        { type: "table", headers: ["模型", "官方 输入 / 输出（$ / 1M）", "本站（−50%）"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
        ] },
        { type: "link", text: "Claude Opus 4.8 详细价格（缓存、上下文、FAQ）", href: "/models/claude-opus-4-8" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "有哪些 Opus 模型可用？", a: "Claude Opus 4.8（claude-opus-4-8）和 Claude Opus 4.7，与 Sonnet、Haiku 共用同一把密钥和预付余额。" },
      { q: "Opus 值得多花那些 token 吗？", a: "对于复杂推理、重构和长时间智能体运行，值得。对于快速、廉价的任务，Haiku 或 Sonnet 通常更划算。" },
    ],
  };
