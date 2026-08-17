import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API 上的提示缓存",
    h1: "用 Claude 提示缓存削减成本",
    description: "提示缓存让 Claude API 上重复的上下文便宜得多。它在 apiToken.sale 上如何运作、何时使用，以及如何与你的折扣叠加。",
    keywords: ["claude 提示缓存", "claude api 缓存", "anthropic prompt cache", "缓存降低 claude 成本", "claude 缓存读取"],
    dek: "如果你反复发送同样的大段上下文——系统提示、文件、工具定义——缓存会把这些 token 从昂贵变成近乎免费。",
    sections: [
      { h2: "缓存如何省钱", blocks: [
        { type: "p", text: "缓存写入和缓存读取分别计量，而缓存读取只是全新输入 token 价格的一小部分。稳定、复用的上下文是理想的缓存对象。" },
      ] },
      { h2: "它可与你的折扣叠加", blocks: [
        { type: "p", text: "缓存降低 token 数量；你的 apiToken.sale 折扣降低每 token 单价。两者叠加，账单大幅缩水，而且每一条缓存行都会显示在你的用量明细中。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "提示缓存能省多少？", a: "缓存读取只是全新输入 token 价格的一小部分，因此重复的大段上下文会便宜得多。" },
      { q: "缓存能配合折扣一起用吗？", a: "能——缓存降低 token 数量、折扣降低每 token 单价，因此节省效果相乘。" },
    ],
  };
