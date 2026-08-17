import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "在 Claude Code 中使用 Kimi K3",
    h1: "在 Claude Code 中运行 Kimi K3 与 Kimi for Coding",
    description: "通过 apiToken.sale 为 Claude Code 配置 Kimi K3 或 Kimi for Coding：固定所有 model tier、保留 1M 上下文并验证端点。",
    keywords: ["kimi claude code", "kimi k3 claude code", "kimi for coding claude code", "claude code 自定义模型", "claude code kimi api", "k3 1m claude code"],
    dek: "Claude Code 原生使用 Anthropic Messages，因此可以直接运行 Kimi。可靠配置会把每个内部 model tier 固定到同一个 Kimi 别名，否则主会话可能正常，而 subagent 因继承 Claude 模型而失败。",
    sections: [
      { h2: "固定连接与所有 model tier", blocks: [
        sourceBlock("kimi-api-for-claude-code", 0, 0),
        { type: "p", text: "Anthropic 路由使用裸订阅别名。对于 k3-256k 或 kimi-for-coding 等 256K 模型，保留 tier pins，但去掉两个 1M 上下文变量。" },
      ] },
      { h2: "验证路由，而不是模型自我介绍", blocks: [
        { type: "list", items: [
          "打开 /status，确认 Anthropic base URL 为 apiToken.sale。",
          "不要询问模型身份：Claude Code 的 system prompt 可能让任何后端自称 Claude。",
          "将 none/off 视为关闭 K3 推理，而不是选择另一模型。实测覆盖仍按 K3 费率结算；kimi-k2.6 不是可公开寻址的模型。",
          "长期固定别名前先检查 GET /v1/models。",
        ] },
      ] },
    ],
    faq: [
      { q: "Claude Code 支持 Kimi K3 吗？", a: "支持。将 Claude Code 指向 https://router.apitoken.sale，并把每个 model tier 固定到已准入的 Kimi 订阅别名。" },
      { q: "为什么必须固定所有 Claude Code model variables？", a: "Claude Code 会为主会话、tiers 与 subagents 分别选模型。未固定的 tier 可能继承 Claude ID，只在后台路径运行时失败。" },
      { q: "如何在 Claude Code 中保留 K3 的完整 1M 上下文？", a: "使用 k3 或 k3[1m]，并将 CLAUDE_CODE_MAX_CONTEXT_TOKENS 与 CLAUDE_CODE_AUTO_COMPACT_WINDOW 都设为 1048576。" },
    ],
  };
