import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude Sonnet API 访问",
    h1: "通过 API 使用 Claude Sonnet",
    description: "通过 apitoken.sale 使用 Claude Sonnet 5 和 Sonnet 4.6——日常编码与智能体的默认模型，统一享受官方 API 价格 50% 的折扣。",
    keywords: ["claude sonnet api", "claude sonnet 5 api", "sonnet api 密钥", "claude sonnet 价格", "最适合编码的 claude 模型"],
    dek: "Sonnet 是主力：足够快，适合交互式编码；又足够聪明，胜任真正的智能体工作流。apitoken.sale 在一份折扣余额上提供 Sonnet 5 和 Sonnet 4.6。",
    sections: [
      { h2: "日常主力模型", blocks: [
        { type: "p", text: "对于大多数编码和智能体任务，Sonnet 是合适的默认选择——在质量、速度和成本之间取得了很好的平衡。把 Opus 留给真正的难题。" },
      ] },
      { h2: "Sonnet 定价说明", blocks: [
        { type: "p", text: "Claude Sonnet 5（claude-sonnet-5）采用介绍期官方费率，引擎始终在套用你的折扣前应用当前有效费率。Sonnet 4.6 仍可在同一把密钥上使用。" },
        { type: "table", headers: ["模型", "官方 输入 / 输出（$ / 1M）", "本站（−50%）"], rows: [
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50"],
        ] },
        { type: "link", text: "Claude Sonnet 5 详细价格（缓存、上下文、FAQ）", href: "/models/claude-sonnet-5" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "我能用哪些 Sonnet 模型？", a: "Claude Sonnet 5（claude-sonnet-5）和 Claude Sonnet 4.6，与 Opus、Haiku 共用同一份余额。" },
      { q: "Sonnet 适合编码吗？", a: "适合——Sonnet 是日常编码和智能体工作流推荐的默认模型。" },
    ],
  };
