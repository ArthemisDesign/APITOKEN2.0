import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "在 Roo Code 中使用 Claude API",
    h1: "在 Roo Code 中使用 Claude API",
    description: "通过 apitoken.sale 将 VS Code 中的 Roo Code 接入 Claude：选择 Anthropic 提供方，启用自定义 base URL，粘贴密钥，以统一 50% 的折扣编码。",
    keywords: ["claude api roo code", "roo code anthropic", "roo code claude", "roo code 自定义 base url", "roo code api 密钥"],
    dek: "Roo Code 是带原生 Anthropic 提供方和自定义 base URL 选项的智能体 VS Code 扩展——在折扣网关上两分钟即可完成设置。",
    sections: [
      { h2: "三步设置", blocks: [
        { type: "steps", items: [
          "打开 Roo Code 设置，选择 Anthropic 作为 API 提供方。",
          "启用自定义 base URL 选项并设为 https://router.apitoken.sale；粘贴你的 sk-pool-… 密钥。",
          "选择一个模型，例如 claude-opus-4-8 或 claude-sonnet-5，然后开始任务。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额，足够你接通工具并在充值前跑通真实调用。" },
      ] },
      { h2: "Roo Code 为什么烧 token——以及如何少花钱", blocks: [
        { type: "p", text: "智能体扩展会循环地读文件、规划、编辑、复查，一个任务可能跑很多次模型调用。这正是按 token 折扣最有价值的负载：同样的会话便宜 50%，控制台里还有 token 级明细。" },
        { type: "list", items: [
          "日常任务走 claude-sonnet-5，难题交给 claude-opus-4-8。",
          "提示缓存按更便宜的官方缓存费率计费，再叠加你的折扣。",
          "一把密钥同时覆盖 Roo Code、Cline、Cursor 和各 SDK。",
        ] },
      ] },
    ],
    faq: [
      { q: "Roo Code 支持自定义 Anthropic base URL 吗？", a: "支持——Anthropic 提供方设置里有自定义 base URL 选项；设为 https://router.apitoken.sale 并使用你的 apitoken.sale 密钥即可。" },
      { q: "这把密钥能让 Roo Code 用哪些模型？", a: "所有受支持的 Claude 模型——Opus 4.8 和 4.7、Sonnet 5 和 4.6、Haiku 4.5——共用一把密钥和一个预付余额。" },
      { q: "和用 Cline 有什么区别？", a: "设置几乎一样：两者都是带 Anthropic 提供方、接受自定义 base URL 的 VS Code 智能体。用你喜欢的那个即可；密钥在两者中都能用。" },
    ],
  };
