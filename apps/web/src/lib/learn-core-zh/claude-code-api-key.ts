import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "用 API 密钥配置 Claude Code",
    h1: "用 apitoken.sale 密钥运行 Claude Code",
    description: "只需两个环境变量即可为 Claude Code 配置 apitoken.sale 密钥，用预付余额以统一 50% 折扣运行所有 Claude 模型。",
    keywords: ["claude code api 密钥", "claude code 配置", "claude code anthropic base url", "claude code 自定义密钥", "低成本运行 claude code"],
    dek: "Claude Code 读取两个环境变量。把它们指向 apitoken.sale，即可保留全部功能，同时按折扣预付余额计费。",
    sections: [
      { h2: "两个变量", blocks: [
        { type: "code", code: `export ANTHROPIC_BASE_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••\n\n# then just run\nclaude` },
        { type: "p", text: "整个配置就这么简单。高难度工作用 claude-opus-4-8，日常编码用 claude-sonnet-5。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "验证并选择模型", blocks: [
        { type: "p", text: "先跑一个简短的提示确认密钥可用，然后设置你的默认模型。如果 Claude Code 报鉴权错误，请重新检查两个环境变量，并重启 shell 以确保它们已导出。" },
        { type: "list", items: [
          "日常编码：claude-sonnet-5。",
          "高难度重构和漫长会话：claude-opus-4-8。",
          "在控制台按请求查看 token 用量，以追踪消费。",
        ] },
      ] },
    ],
    faq: [
      { q: "如何把 Claude Code 指向 apitoken.sale？", a: "将 ANTHROPIC_BASE_URL 和 ANTHROPIC_API_KEY 设为你的 apitoken.sale 端点和密钥，然后运行 claude。" },
      { q: "Claude Code 的所有功能都能保留吗？", a: "能——只有计费方式改变，从订阅制变为折扣预付用量。" },
    ],
  };
