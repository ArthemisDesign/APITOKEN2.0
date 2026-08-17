import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "使用 Claude API 进行流式输出",
    h1: "从 Claude API 流式获取响应",
    description: "如何在 apitoken.sale 上流式获取 Claude 响应，让编码智能体和界面更灵敏。与 Anthropic SSE 格式相同，计费方式与非流式一致。",
    keywords: ["claude api 流式", "claude sse", "流式获取 claude 响应", "anthropic 流式 api", "claude api 实时"],
    dek: "流式输出会在 token 生成时即刻发送，让智能体和聊天界面感觉即时响应。apitoken.sale 支持标准的 Anthropic 流式格式。",
    sections: [
      { h2: "如何流式输出", blocks: [
        { type: "p", text: "在请求中设置 \"stream\": true（或使用 SDK 的流式辅助方法）。网关会返回标准的 Anthropic 服务器发送事件（SSE）。" },
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "stream": true,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
      ] },
      { h2: "计费完全相同", blocks: [
        { type: "p", text: "流式与非流式请求的计费方式相同——都按输入和输出 token 计费——因此流式输出不会让你多花一分钱。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "什么时候值得用流式", blocks: [
        { type: "list", items: [
          "聊天和编码界面，用户会看着答案逐字出现。",
          "长生成任务，可以尽早渲染或处理部分输出。",
          "一旦发出工具调用就停止的智能体。",
        ] },
        { type: "p", text: "对于简短的批处理任务，非流式更简单；无论哪种方式，成本都一样。" },
      ] },
    ],
    faq: [
      { q: "apitoken.sale 支持流式输出吗？", a: "支持——标准的 Anthropic SSE 流式格式适用于编码智能体、IDE 和生产调用。" },
      { q: "流式输出会更贵吗？", a: "不会。流式与非流式请求按 token 计费的方式完全相同。" },
    ],
  };
