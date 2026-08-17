import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API 最佳实践",
    h1: "Claude API 最佳实践",
    description: "在 apitoken.sale 上使用 Claude API 的实用最佳实践：模型选择、提示缓存、流式输出、密钥终身累计消费上限、到期日期，以及安全处理密钥。",
    keywords: ["claude api 最佳实践", "claude api 技巧", "claude api 生产环境", "claude api 使用规范", "anthropic api 最佳实践"],
    dek: "一份简短的清单，帮你在生产环境中从 Claude API 获得可靠又经济的结果。",
    sections: [
      { h2: "清单", blocks: [
        { type: "list", items: [
          "为每项任务挑选能胜任的最便宜模型；仅在需要时升级。",
          "缓存大而稳定的上下文，以大幅削减输入成本。",
          "为灵敏的智能体和界面使用流式响应。",
          "为每把密钥设置可选的终身累计消费上限和到期日期。",
          "用 Retry-After 和退避处理 429。",
          "关注 token 级用量明细，尽早发现浪费。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "把成本和可靠性控制住", blocks: [
        { type: "list", items: [
          "把 max_tokens 限制在每次响应实际所需的范围。",
          "对 429/5xx 采用指数退避重试，而非紧密循环。",
          "为不同环境使用名称清晰的单独密钥，泄露时无需更换所有客户端的密钥。",
          "每周复查 token 级用量，尽早发现回退。",
        ] },
      ] },
    ],
    faq: [
      { q: "最有效的最佳实践是什么？", a: "让模型与任务匹配，并缓存重复的上下文——两者结合最能削减成本。" },
      { q: "如何保护密钥安全？", a: "将密钥存入密钥管理器，设置合适的终身累计消费上限和到期日期，并立即吊销已暴露的密钥。" },
    ],
  };
