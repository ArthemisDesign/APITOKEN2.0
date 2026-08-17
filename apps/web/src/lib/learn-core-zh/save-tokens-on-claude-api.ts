import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "如何在 Claude API 上节省 token",
    h1: "如何在 Claude API 上节省 token",
    description: "通过提示缓存、为每项任务选对模型和精简上下文来削减 Claude API 成本。这些实用的省 token 技巧可与 apitoken.sale 折扣叠加。",
    keywords: ["节省 claude api token", "降低 claude api 成本", "claude 提示缓存", "claude api 优化", "降低 claude api 账单"],
    dek: "你的折扣降低了每 token 的单价；这些技巧降低了 token 的数量。二者叠加，会让账单大幅缩水。",
    sections: [
      { h2: "使用提示缓存", blocks: [
        { type: "p", text: "长而稳定的上下文——系统提示、大文件、工具定义——都应当缓存。缓存读取的成本仅为全新输入 token 的一小部分，因此重复的上下文变得廉价。" },
      ] },
      { h2: "选对模型", blocks: [
        { type: "p", text: "不要把每个请求都发给 Opus。把廉价或高并发的工作路由给 Haiku，让日常编码留在 Sonnet 上，把 Opus 留给真正高难度的推理。" },
      ] },
      { h2: "精简上下文", blocks: [
        { type: "list", items: [
          "只发送任务真正需要的文件和历史。",
          "对长会话做摘要，而非完整重发。",
          "把 max_tokens 限制在响应真正需要的范围内。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "最能省 token 的单项措施是什么？", a: "对大而重复的上下文使用提示缓存，再配合选择能胜任任务的最便宜模型。" },
      { q: "这些技巧能与折扣叠加吗？", a: "能。折扣降低每 token 单价；这些技巧降低 token 数量，因此节省会相乘放大。" },
    ],
  };
