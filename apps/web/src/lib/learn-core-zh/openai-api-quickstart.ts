import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "OpenAI 兼容 API 快速上手——一个密钥调用 GPT-5.6",
    h1: "OpenAI 兼容 API 快速上手：Responses 与 Chat Completions",
    description: "通过 apiToken.sale 的 OpenAI 兼容 API 运行 GPT-5.6 模型——Responses 与 Chat Completions 支持 SSE 流式输出，一个 sk-pool 密钥与 Claude 共用余额，享统一 50% 折扣。",
    keywords: ["openai 兼容 api", "gpt-5.6 api", "responses api", "chat completions 自定义 base url", "openai sdk base_url", "gpt api 密钥", "gpt-5.6 价格"],
    dek: "你的 sk-pool 密钥不只是 Claude 专用。同一个密钥和预付余额还通过 OpenAI 兼容端点提供 GPT-5 系列——标准的 Responses 与 Chat Completions 调用、官方 OpenAI SDK、SSE 流式输出，以及同样的统一 50% 折扣。",
    sections: [
      { h2: "三步完成第一次 GPT 调用", blocks: [
        { type: "steps", items: [
          "创建免费账户并生成一个 API 密钥（形如 sk-pool-…）——该密钥同时已覆盖 Claude 模型。",
          "将客户端指向 https://router.apitoken.sale/v1，使用 Authorization: Bearer 认证——不要用 x-api-key，那是 Anthropic 表面的请求头。",
          "用 GET https://router.apitoken.sale/v1/models 确认已启用的模型——统一目录按提供方命名 ID（anthropic/*、openai/*、google/*）——然后发送 Responses 请求。",
        ] },
        { type: "code", code: `curl https://router.apitoken.sale/v1/responses \\\n  -H "Authorization: Bearer $APITOKEN_API_KEY" \\\n  -H "Content-Type: application/json" \\\n  -d '{\n    "model": "gpt-5.6-sol",\n    "input": "Reply with exactly: connected"\n  }'` },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额——适用于支持的 Claude、GPT、Gemini 与 Kimi 模型；邮箱密码账户不参与。" },
      ] },
      { h2: "使用官方 OpenAI SDK", blocks: [
        { type: "p", text: "官方 SDK 无需改动——只需更换 base_url 和密钥。生产环境请把密钥放在服务端环境变量中。" },
        { type: "code", code: `import os\nfrom openai import OpenAI\n\nclient = OpenAI(\n    api_key=os.environ["APITOKEN_API_KEY"],\n    base_url="https://router.apitoken.sale/v1",\n)\n\nresponse = client.responses.create(\n    model="gpt-5.6-sol",\n    input="Reply with exactly: connected",\n)\nprint(response.output_text)` },
        { type: "p", text: "如果客户端需要，Chat Completions 也在同一主机上提供——模型 ID 和密钥不变。" },
        { type: "code", code: `completion = client.chat.completions.create(\n    model="gpt-5.6-sol",\n    messages=[{"role": "user", "content": "Hello"}],\n)\nprint(completion.choices[0].message.content)` },
      ] },
      { h2: "可用的 GPT 模型", blocks: [
        { type: "p", text: "模型集在引擎中固定定价；GET https://router.apitoken.sale/v1/models 始终是实时答案。目前涵盖三个 GPT-5.6 档位和两个上一代模型：" },
        { type: "table", headers: ["模型 ID", "档位", "官方输入 / 输出（$ / 1M）", "缓存输入"], rows: [
          ["gpt-5.6-sol（别名：gpt-5.6）", "旗舰", "$5 / $30", "$0.50"],
          ["gpt-5.6-terra", "均衡", "$2 / $12", "$0.20"],
          ["gpt-5.6-luna", "快速", "$0.20 / $1.20", "$0.02"],
          ["gpt-5.5", "上一代旗舰", "$5 / $30", "$0.50"],
          ["gpt-5.4", "上一代均衡", "$2.50 / $15", "$0.25"],
        ] },
        { type: "list", items: [
          "推理强度可按请求调整——所有模型支持 none 到 xhigh，GPT-5.6 系列还支持 max。",
          "所有模型支持文本与图片输入，并在 Responses 和 Chat Completions 上提供 SSE 流式输出。",
          "超过 272K 输入 token 的请求按 OpenAI 长上下文费率计费：整个请求输入 2 倍、输出 1.5 倍。",
          "你的 B2C 折扣与 Claude 用量完全一致——一个余额、一个费率，按官方费用 50% 折扣。",
        ] },
        { type: "link", text: "完整的模型规格与折后价格", href: "/models" },
      ] },
      { h2: "端点的覆盖范围", blocks: [
        { type: "p", text: "这是独立的 OpenAI 兼容服务，并非 OpenAI Platform。它提供模型目录、流式 Responses 与 Chat Completions，以及 GPT Image 2 专用生成和编辑路由。音频、文件、realtime、assistants、batch 与 fine-tuning 端点不可用。" },
        { type: "note", text: "错误以 OpenAI 信封返回——{\"error\":{\"message\",\"type\",\"param\",\"code\"}}。401 表示密钥或认证头错误（应使用 Bearer 而非 x-api-key）；402 表示共享预付余额需要充值；404 表示模型 ID 未启用——请查询 GET https://router.apitoken.sale/v1/models。" },
      ] },
    ],
    faq: [
      { q: "同一个密钥还能用于 GPT 之外的模型吗？", a: "能。同一个 sk-pool 密钥和余额也支持 Claude、Gemini 与 Kimi；请使用对应提供商文档中的协议和认证请求头。" },
      { q: "OpenAI 兼容端点使用哪个认证头？", a: "Authorization: Bearer sk-pool-…。x-api-key 仅用于 Anthropic 表面——把它发给 OpenAI 端点会返回 401。" },
      { q: "选 Responses 还是 Chat Completions？", a: "两者都支持 SSE 流式输出。新代码和官方 SDK 用 Responses；需要经典形状的客户端和框架用 Chat Completions。" },
      { q: "GPT 用量如何计费？", a: "按官方 OpenAI 费率逐 token 计费——包括缓存输入和长上下文定价——然后在计入预付余额前减去你的 50% B2C 统一折扣，与 Claude 用量完全一致。" },
    ],
  };
