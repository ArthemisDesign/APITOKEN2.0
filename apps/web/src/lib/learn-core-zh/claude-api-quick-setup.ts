import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API 快速上手：几分钟内从密钥到首次调用",
    h1: "Claude API 快速上手：完成配置并发出第一次调用",
    description: "Claude API 快速上手指南：创建一把密钥，把任意兼容 Anthropic 的客户端指向 router.apitoken.sale，然后用 curl、Python、TypeScript 或你的 IDE 发出第一个 /v1/messages 请求。",
    keywords: ["claude api 快速上手", "claude api 配置", "claude api 第一个请求", "anthropic messages api", "claude api base url", "claude api curl 示例", "claude api hello world", "claude api 密钥", "claude api 入门教程", "claude api 使用教程", "购买 claude api 额度"],
    dek: "这份 Claude API 快速上手指南带你在几分钟内从新账户走到一次完成的 /v1/messages 调用。你只需要三样东西：一把 sk-pool 密钥、router.apitoken.sale 这个 Base URL，以及两个 HTTP 请求头。之后的一切都是标准的 Anthropic Messages API——同一份代码不做任何改动就能跑在官方端点上。",
    sections: [
      { h2: "Claude API 快速上手到底需要什么", blocks: [
        { type: "p", text: "一套能跑通的 Claude API 配置，既不是装一个 SDK，也不是一周的接入流程——它只是一个带两个请求头的 HTTP POST。注册、生成密钥、发出 messages 请求：第一个 2xx 通常比你读这页时泡的那杯咖啡到得还快。该端点讲的是原汁原味的 Anthropic Messages 协议，也就是说，每一个为 Claude 写的教程、SDK 和编程 agent 天生就知道怎么跟它对话。" },
        { type: "list", items: [
          "一个免费账户——无需审核、没有等待名单，也不要求 Anthropic 账户。",
          "一把 API 密钥（形如 sk-pool-…），通用于所有受支持的模型，包括 Claude、GPT、Gemini 和 Kimi。",
          "Base URL https://router.apitoken.sale——新集成的唯一端点。",
          "每个请求带两个请求头：x-api-key 放你的密钥，以及 anthropic-version: 2023-06-01。",
        ] },
      ] },
      { h2: "创建密钥并选定端点", blocks: [
        { type: "steps", items: [
          "用 Google、GitHub 或邮箱注册，然后打开控制台——没有审核队列。",
          "生成密钥。它只显示一次；把它存进环境变量，不要写进源代码。",
          "把客户端的 Base URL 设为 https://router.apitoken.sale，并确认它向 POST /v1/messages 发送请求。",
        ] },
        { type: "code", code: `Base URL:  https://router.apitoken.sale\nEndpoint:  POST /v1/messages\nHeaders:   x-api-key: sk-pool-•••\n           anthropic-version: 2023-06-01` },
        { type: "p", text: "密钥在下一次请求时即刻生效——没有激活延迟。如果余额为空，先充值：支持任意整数美元金额，所以充一美元就足够把整条链路端到端验证一遍。" },
      ] },
      { h2: "用 curl 发出第一个请求", blocks: [
        { type: "p", text: "在把任何东西接进应用之前，先用最小的调用验证通路。max_tokens 在 Messages API 上是必填的——漏掉它是首次调用最常见的错误。" },
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-opus-4-8",\n    "max_tokens": 1024,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
        { type: "p", text: "成功的响应是一个 JSON 对象，它的 content 字段是一个块数组——纯文本回复就是一个 type 为 text 的块。配置阶段每次调用都值得读两个字段：stop_reason 告诉你模型是正常结束（end_turn）还是撞上了你设的 max_tokens 上限；usage 则报告本次计费的确切 input_tokens 和 output_tokens。如果 content 返回为空且 stop_reason: max_tokens，应该调高上限，而不是原样重试同一个请求。" },
      ] },
      { h2: "用 Python 或 TypeScript 发同一个调用", blocks: [
        { type: "p", text: "官方 Anthropic SDK 接受自定义 Base URL，所以从 curl 迁移到真正的代码只是覆盖一个参数的事。模型 ID、消息结构、系统提示词和工具调用的行为与直连 api.anthropic.com 时完全一致。" },
        { type: "code", code: `from anthropic import Anthropic\n\nclient = Anthropic(\n    base_url="https://router.apitoken.sale",\n    api_key="sk-pool-•••",\n)\nmsg = client.messages.create(\n    model="claude-opus-4-8",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Hello"}],\n)\nprint(msg.content[0].text)` },
        { type: "code", code: `import Anthropic from "@anthropic-ai/sdk";\n\nconst client = new Anthropic({\n  baseURL: "https://router.apitoken.sale",\n  apiKey: "sk-pool-•••",\n});\nconst msg = await client.messages.create({\n  model: "claude-opus-4-8",\n  max_tokens: 1024,\n  messages: [{ role: "user", content: "Hello" }],\n});` },
        { type: "link", text: "完整的 SDK 走查：anthropic-sdk-base-url", href: "/docs/learn/anthropic-sdk-base-url" },
      ] },
      { h2: "做 UI 之前先打开流式输出", blocks: [
        { type: "p", text: "凡是有人盯着等的东西——聊天、代码补全、带可见进度的 agent 循环——都应该走流式。在同一个请求体里加上 \"stream\": true，响应就变成 Server-Sent Events：一个 message_start 信封、一串携带文本片段的 content_block_delta 事件，最后是一个 message_stop。客户端负责把片段拼起来；请求的其他部分完全不变。" },
        { type: "code", code: `curl -N https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-opus-4-8",\n    "max_tokens": 1024,\n    "stream": true,\n    "messages": [{"role":"user","content":"Count to five."}]\n  }'` },
        { type: "note", text: "流式有两个坑：不加 -N（或 HTTP 客户端的无缓冲模式）时，curl 会把整个 SSE 响应体缓冲起来，看起来和非流式调用一模一样；最终的 usage 统计是在结尾的 message_delta 事件里到达的，而不是在 JSON 响应体里——如果你按请求统计花费，要去那里读。" },
      ] },
      { h2: "把 IDE 或编程 agent 指向同一把密钥", blocks: [
        { type: "p", text: "因为端点在协议上完全一致，任何带 Anthropic 供应商设置的工具都只要改两个字段。以 Cursor 为例：Settings → Models → Anthropic API，填入 Base URL、粘贴密钥，然后选一个当前的模型 ID。" },
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : https://router.apitoken.sale\nAPI key  : sk-pool-•••\nModel    : claude-opus-4-8` },
        { type: "p", text: "同样的两字段改动也适用于 Cline、Continue 这类 VS Code 扩展，以及从环境变量读取 ANTHROPIC_BASE_URL 和 ANTHROPIC_API_KEY 的终端 agent。一把密钥、一份预付余额，覆盖所有工具。" },
        { type: "link", text: "Cursor 专用指南：claude-api-key-for-cursor", href: "/docs/learn/claude-api-key-for-cursor" },
        { type: "link", text: "当前模型阵容与各模型定价", href: "/models" },
      ] },
      { h2: "首次调用报错？逐条解码", blocks: [
        { type: "p", text: "几乎所有失败的首次调用都落在这四种状态码里。响应体也要读——错误以 Anthropic 错误信封返回，message 里会点名出问题的字段。" },
        { type: "table", headers: ["状态码", "含义", "解决办法"], rows: [
          ["400 Bad Request", "请求体格式有误——通常是缺了 max_tokens 或模型 ID 未知", "设置 max_tokens；使用当前的模型 ID，例如 claude-opus-4-8"],
          ["401 Unauthorized", "x-api-key 缺失或错误，或请求发到了错误的 Base URL", "重新确认密钥已完整粘贴，且 Base URL 是 https://router.apitoken.sale"],
          ["402 / 余额不足", "预付余额不足以支付本次请求", "按任意整数美元金额充值后重试"],
          ["429 Too Many Requests", "触发了并发或速率上限", "遵守 Retry-After 请求头并降低并发"],
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台奖励余额——适用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "Claude API 快速上手该用哪个 Base URL？", a: "在任意兼容 Anthropic 的工具中使用 https://router.apitoken.sale，并向 /v1/messages 发送请求。仍在旧主机 https://api.apitoken.sale 上的既有集成继续可用——对新接入来说，统一 router 是推荐端点。" },
      { q: "Claude API 需要哪个鉴权请求头？", a: "发送携带密钥的 x-api-key 和 anthropic-version: 2023-06-01，与官方 Anthropic API 完全一致。这个入口不要用 Authorization: Bearer——那个请求头属于 OpenAI 兼容通道。" },
      { q: "需要 Anthropic 账户或绑定的信用卡吗？", a: "不需要 Anthropic 账户——用 Google、GitHub 或邮箱注册即可获得自己的 sk-pool 密钥。余额是预付制：按任意整数美元金额充值，只在请求实际运行时扣费。" },
      { q: "验证配置跑通的最便宜方式是什么？", a: "充最小的整数美元金额，发一个 max_tokens: 1 的请求——一次成功的 2xx 就在一次调用里同时证明了鉴权、端点和计费。通过 Google 或 GitHub 注册的新账户还自带 $5 平台奖励余额，足够完全覆盖这次测试。" },
      { q: "为什么密钥有效，第一次调用还是返回 400？", a: "几乎都是缺了 max_tokens 字段，或用了未启用的模型 ID——Messages API 会拒绝没有 max_tokens 的请求。使用当前的模型 ID（例如 claude-opus-4-8）并设置明确的 token 上限。" },
      { q: "同一把密钥能用于流式输出和工具调用吗？", a: "可以。流式只是在同一个请求上加 \"stream\": true 标志，工具调用遵循标准的 Anthropic schema——不需要单独的密钥、套餐或端点。" },
    ],
  };
