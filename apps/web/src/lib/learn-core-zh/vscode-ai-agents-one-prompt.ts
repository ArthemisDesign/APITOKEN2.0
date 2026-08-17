import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "在 VS Code 中用 Claude 运行免费 AI 智能体",
    h1: "用 Claude 运行免费的 VS Code AI 智能体",
    description: "用 apitoken.sale 的 Claude 密钥配置 Cline、Roo Code 等免费 VS Code 智能体——无需 Cursor Pro。一个端点，通用所有 Claude 模型，还享折扣。",
    keywords: ["免费 vscode ai 智能体", "cline roo code claude", "vscode claude 智能体", "免费的 cursor 替代品", "不用 cursor 在 vscode 用 claude"],
    dek: "无需 Cursor Pro 也能拥有智能体编码。免费的 VS Code 智能体接受任何兼容 Anthropic 的密钥，因此 Claude 可以用折扣余额在 VS Code 中运行。",
    sections: [
      { h2: "把智能体指向 Claude", blocks: [
        { type: "steps", items: [
          "安装一个免费的智能体扩展，例如 Cline 或 Roo Code。",
          "选择 Anthropic 作为 API 提供方。",
          "把 Base URL 设为 https://router.apitoken.sale，粘贴你的 sk-pool-••• 密钥，并选择一个模型，例如 claude-sonnet-5。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "为每项任务选对模型", blocks: [
        { type: "list", items: [
          "claude-sonnet-5——日常编码和智能体循环的默认之选。",
          "claude-opus-4-8——复杂重构、架构设计和漫长会话。",
          "claude-haiku-4-5——快速、廉价的编辑和高吞吐步骤。",
        ] },
        { type: "p", text: "由于一把密钥通用所有模型，你可以在扩展里按任务随时切换，无需更换账户或计费方式。" },
      ] },
    ],
    faq: [
      { q: "做 AI 编码需要 Cursor Pro 吗？", a: "不需要。Cline、Roo Code 等免费 VS Code 智能体都可搭配 apitoken.sale 的 Claude 密钥使用。" },
      { q: "我该选哪个模型？", a: "日常编码用 claude-sonnet-5；复杂任务用 claude-opus-4-8。" },
    ],
  };
