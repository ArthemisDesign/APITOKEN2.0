import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "在 Anthropic SDK 中使用自定义 Base URL",
    h1: "将 Anthropic SDK 指向 apitoken.sale",
    description: "只需把 base_url 设为 router.apitoken.sale，即可在官方 Anthropic Python 和 TypeScript SDK 中使用 apitoken.sale。同样的 SDK、同样的代码，每 token 成本更低。",
    keywords: ["anthropic sdk base url", "anthropic python sdk 自定义端点", "claude sdk base url", "anthropic typescript sdk", "claude api sdk 配置"],
    dek: "官方 Anthropic SDK 允许覆盖 Base URL，因此切换到 apitoken.sale 只是一行改动——你的模型 ID 和消息代码完全保持不变。",
    sections: [
      { h2: "Python", blocks: [
        { type: "code", code: `from anthropic import Anthropic\n\nclient = Anthropic(\n    base_url="https://router.apitoken.sale",\n    api_key="sk-pool-•••",\n)\nmsg = client.messages.create(\n    model="claude-opus-4-8",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Hello"}],\n)` },
      ] },
      { h2: "TypeScript", blocks: [
        { type: "code", code: `import Anthropic from "@anthropic-ai/sdk";\n\nconst client = new Anthropic({\n  baseURL: "https://router.apitoken.sale",\n  apiKey: "sk-pool-•••",\n});\nconst msg = await client.messages.create({\n  model: "claude-opus-4-8",\n  max_tokens: 1024,\n  messages: [{ role: "user", content: "Hello" }],\n});` },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "验证切换是否成功", blocks: [
        { type: "p", text: "改完 Base URL 后，发一次请求，确认你能收到正常的 Anthropic 响应。流式输出、工具调用和系统提示的表现都与 api.anthropic.com 完全一致——变的只有计费端点。" },
        { type: "list", items: [
          "返回 401 说明密钥或 Base URL 有误——两者都要重新检查。",
          "保持相同的模型 ID；消息相关的代码无需任何改动。",
          "在控制台按请求查看用量，确认消费和你的折扣。",
        ] },
      ] },
    ],
    faq: [
      { q: "我还能继续用官方 Anthropic SDK 吗？", a: "可以。把 base_url（Python）或 baseURL（TypeScript）设为 apitoken.sale，其余一切保持不变。" },
      { q: "模型 ID 会变吗？", a: "不会。继续使用相同的模型 ID，例如 claude-opus-4-8 和 claude-sonnet-5。" },
    ],
  };
