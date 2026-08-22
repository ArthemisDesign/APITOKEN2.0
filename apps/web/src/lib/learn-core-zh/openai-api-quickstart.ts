import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
  title: "OpenAI 兼容 API 快速上手——一个密钥调用 GPT-5.6",
  h1: "OpenAI 兼容 API 快速上手：从 curl 到官方 SDK",
  description: "OpenAI 兼容 API 快速上手：在 apiToken.sale 上通过 Responses 和 Chat Completions 调用 GPT-5.6 模型，支持 SSE 流式输出——一个 sk-pool 密钥、与 Claude 共用的预付余额，以及官方费率统一 50% 折扣。",
  keywords: ["openai 兼容 api", "openai 兼容 api 快速上手", "gpt-5.6 api", "responses api 示例", "chat completions 自定义 base url", "openai sdk base_url", "gpt api key 替代", "gpt-5.6-sol", "openai api 端点迁移", "gpt-5.6 每 token 价格"],
  dek: "想找一个五分钟内就能调通的 OpenAI 兼容 API？把任意 OpenAI 客户端指向 https://router.apitoken.sale/v1，用一个 sk-pool 密钥和与 Claude 共用的那份预付余额即可。Responses 和 Chat Completions 都支持 SSE 流式输出，GPT-5.6 用量按 OpenAI 官方 token 费率计费，再减去你的统一 50% 折扣。",
  sections: [
    { h2: "三步拿到第一个 GPT-5.6 响应", blocks: [
      { type: "p", text: "从 OpenAI 官方 API 迁移到这个端点，只需要换 base URL 和认证头。没有新 SDK 要学，没有适配层，也不需要单独的 GPT 账户——你可能已经在为 Claude 使用的那把密钥在这里就是同一份凭证，同一个预付余额同时计量两家提供方的用量。" },
      { type: "steps", items: [
        "创建免费账户并生成一个 API 密钥——形如 sk-pool-…，它已覆盖各自协议表面上受支持的 Claude、Gemini 和 Kimi 模型。",
        "把客户端指向 https://router.apitoken.sale/v1，用 Authorization: Bearer 认证——不要发 x-api-key；那个请求头属于 Anthropic Messages 表面，在这里会被拒绝。",
        "用 GET https://router.apitoken.sale/v1/models 确认已启用的模型集——统一目录按提供方给 ID 加命名空间（anthropic/*、openai/*、google/*）——然后发送下面的 Responses 请求。",
      ] },
      { type: "code", code: `curl https://router.apitoken.sale/v1/responses \\\n  -H "Authorization: Bearer $APITOKEN_API_KEY" \\\n  -H "Content-Type: application/json" \\\n  -d '{\n    "model": "gpt-5.6-sol",\n    "input": "Reply with exactly: connected"\n  }'` },
      { type: "p", text: "如果响应体里带回了输出文本，接入就完成了——你手里的其他客户端都只差一行配置改动，就能以同样的方式工作。" },
      { type: "note", text: "通过 Google 或 GitHub 创建的新账户自带 $5 平台奖励余额——可用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受该奖励。" },
    ] },
    { h2: "两个构造参数切换官方 SDK", blocks: [
      { type: "p", text: "官方 OpenAI SDK 无需改动即可使用。只需要改 base_url 和密钥；生产环境中密钥应放在服务端环境变量里——绝不要写进客户端代码或提交进仓库的文件。" },
      { type: "code", code: `import os\nfrom openai import OpenAI\n\nclient = OpenAI(\n    api_key=os.environ["APITOKEN_API_KEY"],\n    base_url="https://router.apitoken.sale/v1",\n)\n\nresponse = client.responses.create(\n    model="gpt-5.6-sol",\n    input="Reply with exactly: connected",\n)\nprint(response.output_text)` },
      { type: "p", text: "那些写死 Chat Completions 形状的框架——旧版 LangChain 链、LiteLLM 配置、大多数开源聊天 UI——在同一主机上用同样的模型 ID 和密钥即可工作：" },
      { type: "code", code: `completion = client.chat.completions.create(\n    model="gpt-5.6-sol",\n    messages=[{"role": "user", "content": "Hello"}],\n)\nprint(completion.choices[0].message.content)` },
      { type: "p", text: "新代码该选哪个表面？Responses。两个端点都以 SSE 流式输出，模型、定价和折扣完全一致，但 Responses 是当前 OpenAI 工具链围绕构建的表面——它把推理项和工具调用放进同一条类型化流里，并提供 response.output_text 这样的便利接口。Chat Completions 留给期待经典 messages 数组的客户端和框架；在一个表面上构建的任何代码都不会把你挡在另一个表面之外。" },
    ] },
    { h2: "模型 ID、逐 token 价格与 272K 陷阱", blocks: [
      { type: "p", text: "在提供服务的模型集由引擎固定并定价，GET https://router.apitoken.sale/v1/models 始终是实时答案。目前这条产品线涵盖三个 GPT-5.6 档位，外加两个为兼容性保留的上一代模型：" },
      { type: "table", headers: ["模型 ID", "档位", "官方输入 / 输出（$ / 1M）", "缓存输入"], rows: [
        ["gpt-5.6-sol（别名：gpt-5.6）", "旗舰", "$4 / $20（临时）", "$0.40"],
        ["gpt-5.6-terra", "均衡", "$2 / $12", "$0.20"],
        ["gpt-5.6-luna", "快速", "$0.20 / $1.20", "$0.02"],
        ["gpt-5.5", "上一代旗舰", "$5 / $30", "$0.50"],
        ["gpt-5.4", "上一代均衡", "$2.50 / $15", "$0.25"],
      ] },
      { type: "list", items: [
        "Sol 临时官方输入/缓存/缓存写入/输出费率截至 2026-11-21（含当日）为 $4/$0.40/$5/$20，统一五折后为 $2/$0.20/$2.50/$10；自 2026-11-22 UTC 起恢复标准输入 $5、输出 $30。",
        "按档位选型：最难的推理用 gpt-5.6-sol，日常主力用 gpt-5.6-terra，高并发低成本调用用 gpt-5.6-luna。别名 gpt-5.6 始终跟随旗舰。",
        "推理强度可按请求调整——所有模型支持 none 到 xhigh，GPT-5.6 系列还支持 max。",
        "每个模型都接受文本和图片输入，并在 Responses 和 Chat Completions 上都支持 SSE 流式输出。",
        "缓存输入单独定价，远低于新输入（Sol 促销期每 1M 为 $0.40 对 $4）——在多次调用间保持稳定的提示词前缀，省的是真金白银，而不是微优化。",
        "你的统一 50% B2C 折扣在这里的作用方式与 Claude 用量完全一致——一个余额、一个费率，官方费用减半。",
      ] },
      { type: "note", text: "272K 阈值就是那个陷阱：一旦超过，OpenAI 长上下文费率会对整个请求生效——输入 2 倍、输出 1.5 倍，而不只是超出部分。按 Sol 促销价，270K 输入加 2K 输出官方成本为 $1.12，273K 加 2K 为 $2.244。在越过边界之前，拆分过大的上下文或裁剪历史记录。" },
      { type: "link", text: "完整的逐模型规格与折后价格", href: "/models" },
    ] },
    { h2: "这个端点能做什么——以及不能做什么", blocks: [
      { type: "p", text: "这是一项独立的 OpenAI 兼容服务，不是 OpenAI Platform。它提供模型发现、流式 Responses 和 Chat Completions，以及 GPT Image 2 专用的生成与编辑路由。音频、文件、realtime、assistants、batch 和 fine-tuning 端点不可用——如果你的应用依赖这些，它就不适合迁移。不过对于纯文本和视觉聊天负载，这里的表面是完整的：标准的生成或流式循环里没有任何环节会碰到缺失的端点。" },
      { type: "p", text: "错误以标准 OpenAI 信封返回——{\"error\":{\"message\",\"type\",\"param\",\"code\"}}——现有的错误处理代码可以直接沿用。三个状态码几乎覆盖了你在接入时会看到的一切：" },
      { type: "list", items: [
        "401——密钥错误、已吊销，或者你发了 x-api-key 而不是 Authorization: Bearer。在应用之外用 curl 复现，隔离出是哪一环坏了。",
        "402——共享预付余额需要充值；任何重试或退避都修不好空余额。",
        "404——该模型 ID 未在你的密钥上启用；查一下 GET https://router.apitoken.sale/v1/models，不要想当然地认为 OpenAI 文档里的名字在这里也存在。",
      ] },
    ] },
  ],
  faq: [
    { q: "现有的 OpenAI SDK 能配自定义 base URL 用吗？", a: "可以——给官方客户端传 api_key 和 base_url=\"https://router.apitoken.sale/v1\"，其他一切保持不变。生产环境中把密钥放在服务端环境变量里。" },
    { q: "一个 API 密钥真的能同时覆盖 GPT、Claude、Gemini 和 Kimi 吗？", a: "是的。一个 sk-pool 密钥和一个预付余额服务全部四家提供方；每个表面使用各自文档规定的协议和认证头（这里用 Bearer，Anthropic Messages 端点用 x-api-key）。" },
    { q: "新项目选 Responses API 还是 Chat Completions？", a: "Responses。两者都以 SSE 流式输出，模型和定价相同，但 Responses 是当前 OpenAI SDK 和工具链围绕构建的表面；Chat Completions 留给期待经典形状的客户端。" },
    { q: "为什么 OpenAI 兼容端点返回 401？", a: "几乎总是认证头的问题：这个端点要的是 Authorization: Bearer sk-pool-…，而 Anthropic 式配置里的 x-api-key 头在这里会返回 401。" },
  ],
};
