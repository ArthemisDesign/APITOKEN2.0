import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "在 Kimi Code 中使用 apiToken.sale",
    h1: "在 Kimi Code 中运行 Kimi、Claude、GPT 与 Gemini",
    description: "通过 OpenAI 兼容 provider config 将 Kimi Code 连接到 apiToken.sale，声明 namespaced 模型并保护 config.toml 中的 API 密钥。",
    keywords: ["kimi code api", "kimi code 自定义提供商", "kimi code config toml", "kimi code api 密钥", "kimi code k3", "kimi code openai 兼容"],
    dek: "Kimi Code 接受自定义 OpenAI 兼容 provider，因此一个 apiToken.sale provider 条目可以访问统一目录。每个模型仍需以真实 namespace 和经核验的上下文窗口单独声明。",
    sections: [
      { h2: "安装并声明 provider", blocks: [
        sourceBlock("kimi-api-for-kimi-code", 0, 0),
        { type: "note", text: "不要执行 /login；那会把 CLI 绑定到 Kimi membership。Kimi Code 只在 config.toml 中保存 custom-provider credentials，因此文件包含明文密钥，必须限制权限。" },
      ] },
      { h2: "启动、验证并添加模型", blocks: [
        sourceBlock("kimi-api-for-kimi-code", 1, 0),
        { type: "list", items: [
          "/status 必须显示 https://router.apitoken.sale/v1 为 provider base URL。",
          "model 字段使用统一目录命名空间，例如 kimi/k3、openai/gpt-5.6-terra 或 google/gemini-3.6-flash。",
          "在 config.toml 中为每个额外模型声明经核验的 max_context_size；Kimi Code 用它决定何时压缩上下文。",
        ] },
      ] },
    ],
    faq: [
      { q: "Kimi Code 能使用 apiToken.sale 密钥吗？", a: "可以。添加 base_url 为 https://router.apitoken.sale/v1 的 OpenAI 兼容 provider，并把密钥保存在 Kimi Code config.toml。" },
      { q: "Kimi Code 能运行 Kimi 之外的模型吗？", a: "可以。同一个 provider 条目访问统一目录；用 namespaced ID 与正确上下文限制声明每个 Claude、GPT、Gemini 或 Kimi 模型。" },
      { q: "为什么 chmod 600 很重要？", a: "Kimi Code 不从 shell 读取 custom-provider credentials。原始 API 密钥位于 config.toml，因此文件应只允许你的账户读取。" },
    ],
  };
