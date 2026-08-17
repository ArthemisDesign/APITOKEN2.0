import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "在 VS Code 中使用 Claude API（Cline、Continue）",
    h1: "在 VS Code 中使用 Claude API",
    description: "使用 apitoken.sale 密钥，通过 Cline 或 Continue 在 VS Code 中运行 Claude。把 Anthropic Base URL 设为 router.apitoken.sale，即可按 token 折扣付费。",
    keywords: ["claude api vs code", "cline claude api", "continue claude api", "vscode 用 claude", "vscode anthropic api 密钥"],
    dek: "Cline、Continue 等免费的 VS Code 智能体接受任何兼容 Anthropic 的端点，因此你可以用折扣余额在 VS Code 里用 Claude 编码。",
    sections: [
      { h2: "Cline", blocks: [
        { type: "code", code: `# Cline → Settings\nAPI Provider : Anthropic\nBase URL     : https://router.apitoken.sale\nAPI Key      : sk-pool-•••\nModel        : claude-opus-4-8` },
      ] },
      { h2: "Continue", blocks: [
        { type: "code", code: `// ~/.continue/config.json\n{\n  "models": [{\n    "title": "Claude via apiToken.sale",\n    "provider": "anthropic",\n    "apiBase": "https://router.apitoken.sale",\n    "apiKey": "sk-pool-•••",\n    "model": "claude-opus-4-8"\n  }]\n}` },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "选哪个扩展与故障排查", blocks: [
        { type: "p", text: "Cline 适合作为自主编辑的默认之选；Continue 更轻量，适合内联对话和补全。两者都免费，且都使用你的预付余额。" },
        { type: "list", items: [
          "401 Unauthorized：API 密钥或 Base URL 有误。",
          "找不到模型：使用当前的模型 ID，例如 claude-sonnet-5 或 claude-opus-4-8。",
          "缓慢或 429：降低并发并遵守 Retry-After。",
        ] },
      ] },
    ],
    faq: [
      { q: "哪些 VS Code 扩展可以用？", a: "任何支持兼容 Anthropic 端点的扩展都可以，包括 Cline 和 Continue，均可搭配 apitoken.sale 密钥使用。" },
      { q: "需要付费扩展吗？", a: "不需要。Cline 和 Continue 都是免费的；你只为消耗预付余额的 Claude API 用量付费。" },
    ],
  };
