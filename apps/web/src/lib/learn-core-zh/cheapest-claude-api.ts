import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "最便宜的 Claude API——统一立省 50%",
    h1: "使用 Claude API 最省钱的方式",
    description: "把 Claude API 成本统一削减 50%。apitoken.sale 以预付折扣价出售一模一样的 Anthropic Messages API——同样的模型、同样的接口、更低的每 token 单价。",
    keywords: ["最便宜的 claude api", "claude api 折扣", "便宜的 claude api", "claude api 价格", "节省 anthropic api 费用", "比 anthropic 更便宜的 claude api"],
    dek: "Claude API 按 token 计费，而在漫长的编码会话中这些 token 累积得很快。apitoken.sale 通过汇集预付余额并套用统一折扣，让你以低 50% 的价格用上完全相同的 API。",
    sections: [
      { h2: "为什么更便宜", blocks: [
        { type: "p", text: "你向同一套 Anthropic Messages API 发送同样的请求，得到同样的响应。底层唯一不同的是计费：每次调用按官方费率计量，然后在扣减你的余额前先减去你的折扣。" },
        { type: "list", items: [
          "B2C 账户统一享受比官方消费低 50% 的折扣。",
          "每个请求适用同一费率——无需解锁。",
          "B2B 批量定价单独商议。",
        ] },
      ] },
      { h2: "省钱效果最明显的场景", blocks: [
        { type: "p", text: "智能体编码、漫长的多轮会话以及重度依赖提示缓存的工作流消耗的 token 最多——因此绝对节省额也最大。为每项任务选对模型还能进一步叠加节省。" },
        { type: "note", text: "小贴士：把快速、廉价的工作交给 Haiku，把 Opus 留给高难度推理，能让余额撑得更久。" },
      ] },
      { h2: "无订阅、无绑定", blocks: [
        { type: "p", text: "没有月费。你充值的是永不过期的预付余额，仅在请求运行时才消耗，因此闲置的日子不花一分钱。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "Claude API 折扣如何生效", blocks: [
        { type: "p", text: "没有加价，也没有单独的廉价模型——你得到的是对完全相同的 Claude API 的折扣访问。" },
        { type: "list", items: [
          "每次请求按官方 Anthropic token 费率计量。",
          "减去你的统一 50% 折扣。",
          "净额从你的预付余额中扣除。",
        ] },
        { type: "table", headers: ["模型", "官方 输入 / 输出（$ / 1M）", "本站（−50%）"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "link", text: "每个模型的完整价格（含缓存费率）", href: "/models" },
        { type: "link", text: "用免费计算器估算你的月度成本", href: "/tools/claude-api-cost-calculator" },
      ] },
    ],
    faq: [
      { q: "这真的是同一套 Claude API 吗？", a: "是的——同一套 Anthropic Messages API、相同的模型 ID、相同的请求与响应格式。只有每次调用的价格更低。" },
      { q: "我能省多少？", a: "B2C 定价为每个请求统一比官方 API 消费低 50%。" },
      { q: "有没有隐藏费用或订阅？", a: "没有。余额为预付制、永不过期，仅由真实 API 用量消耗——没有月费。" },
      { q: "有比直接从 Anthropic 购买更便宜的 Claude API 吗？", a: "有。apiToken.sale 以统一 50% 的折扣出售一模一样的 Anthropic API，且没有订阅。" },
    ],
  };
