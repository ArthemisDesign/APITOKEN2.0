import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "在 OpenCode 中使用 Kimi API",
    h1: "在 OpenCode 中运行 Kimi K3 与 Kimi for Coding",
    description: "通过 apiToken.sale 将 OpenCode 连接到 Kimi：路由器插件、密钥范围模型目录、明确 kimi/* ID 和一个预付费 API 密钥。",
    keywords: ["kimi opencode", "kimi api opencode", "kimi k3 opencode", "kimi for coding 配置", "opencode 自定义提供商", "kimi 编程代理"],
    dek: "OpenCode 能明确寻址 Kimi 命名空间并消费路由器实时目录，因此可在 K3 与 Kimi for Coding 之间安全切换，无需手工维护模型限制。",
    sections: [
      { h2: "安装并验证", blocks: [
        { type: "steps", items: [
          "运行 apiToken.sale OpenCode 安装器；它会合并路由器插件并备份现有配置。",
          "重启 OpenCode，让插件获取按密钥作用域的模型目录。",
          "使用明确命名空间模型运行一个确定性提示。",
        ] },
        sourceBlock("kimi-api-for-opencode", 0, 1),
      ] },
      { h2: "安全选择 Kimi 模型", blocks: [
        { type: "list", items: [
          "apitoken/kimi/kimi-for-coding — 经济型编程默认。",
          "apitoken/kimi/kimi-for-coding-highspeed — 双倍 token 费率换取更低延迟。",
          "apitoken/kimi/k3-256k — 较小上下文模式的 K3 推理。",
          "apitoken/kimi/k3 — 目录开放时使用完整 1M K3。",
        ] },
        { type: "note", text: "Claude Code 与 Kimi Code 也支持 Kimi，但配置不同：Claude Code 必须固定每个 model tier，Kimi Code 则使用明确的 OpenAI 兼容 provider block。" },
      ] },
    ],
    faq: [
      { q: "OpenCode 支持 Kimi 吗？", a: "支持。apiToken.sale 路由器插件注册实时 Kimi 命名空间，模型写作 apitoken/kimi/{model}。" },
      { q: "为什么使用插件而不是静态模型列表？", a: "插件让 ID、限制和可用性与密钥实时目录一致，已下线或不可用别名不会留在本地配置中。" },
      { q: "Claude Code 也能使用 Kimi 吗？", a: "可以，但配置不同。将 Claude Code 指向 Anthropic 端点，并把 main、Opus、Sonnet、Haiku 与 subagent model variables 固定到同一个 Kimi 别名。" },
    ],
  };
