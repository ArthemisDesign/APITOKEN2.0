import type { LocalizedContent } from "../learn";
import { BASE, KEY } from "../learn-shared";

export const content: LocalizedContent = {
    title: "Claude Haiku API 接入",
    h1: "通过 API 使用 Claude Haiku 4.5",
    description: "通过 apitoken.sale 接入 Claude Haiku API：模型 ID claude-haiku-4-5、可直接运行的 Messages API 调用、流式输出与提示词缓存，价格一律为官方费率的 5 折。",
    keywords: ["claude haiku api", "claude haiku 4.5 api", "claude-haiku-4-5", "haiku api 密钥", "claude haiku 价格", "最便宜的 claude 模型", "最快的 claude 模型", "claude haiku 提示词缓存", "claude api 免费额度", "免费试用 claude api", "claude api 免费套餐"],
    dek: "Claude Haiku API 是高并发场景的归宿：分类、抽取、路由，以及一切延迟和单价比深度推理更重要的请求。Haiku 4.5 官方计费为每百万 token $1/$5，本站统一 5 折后为 $0.50/$2.50，并且与 Sonnet、Opus、GPT、Gemini、Kimi 共用一把密钥和一份预付余额。本文讲清楚它适合哪些负载、如何发出第一个请求，以及如何把难的那部分流量向上升级。",
    sections: [
      { h2: "Haiku 4.5 生来要承接的工作", blocks: [
        { type: "p", text: "Claude Haiku 4.5 是 Claude 家族中最快、成本最低的模型，通过标准的 Anthropic Messages API 即可访问，模型 ID 为 claude-haiku-4-5——请求格式、请求头、流式行为与 Sonnet、Opus 完全一致。只要延迟和单 token 价格比深度推理更重要，它就是正确的默认选择。通过 apitoken.sale 使用时按预付余额计费，统一比官方费率低 50%，无需订阅，也无需排队等待。" },
        { type: "p", text: "Haiku 的价值体现在流水线的边缘地带——请求短、频率高、彼此可互换的场景：" },
        { type: "list", items: [
          "分类与打标：工单分类、内容审核、意图识别——输入短、输出短，每天数千次调用。",
          "抽取与解析：在更大的模型接触数据之前，先从发票、邮件、日志或 HTML 中抽出结构化字段。",
          "路由与分诊：判断某个请求该交给哪个模型或工具，只把难的升级上去。",
          "对延迟敏感的对话：智能体内层循环、工具调用的粘合逻辑、自动补全式交互——用户正盯着加载圈看的场景。",
          "低成本预处理：在调用 Opus 或 Sonnet 之前，对长上下文做摘要、清洗和切分。",
        ] },
        { type: "note", text: "深度多步推理、高风险分析和超长生成不是 Haiku 的战场。如果某个任务反复达不到你的质量线，那是路由问题而不是提示词问题——把它交给 Sonnet 或 Opus。" },
      ] },
      { h2: "模型 ID、上下文窗口与输出上限", blocks: [
        { type: "p", text: "当前需要记住的 Haiku ID 只有一个：claude-haiku-4-5。它支持 Messages API 的完整能力集——系统提示词、多轮消息、工具调用、流式输出和提示词缓存——上下文窗口为 200K token，最大输出 64K token。这两个上限都低于 Opus 和 Sonnet 产品线，如果你习惯把大文档塞进单次调用，需要留意这一点。" },
        { type: "table", headers: ["规格", "数值"], rows: [
          ["模型 ID", "claude-haiku-4-5"],
          ["上下文窗口", "200K token"],
          ["最大输出", "64K token"],
          ["端点", "POST /v1/messages（Anthropic Messages 格式）"],
          ["鉴权请求头", "x-api-key"],
        ] },
        { type: "p", text: "无论是否流式，Messages API 都要求每个请求显式设置 max_tokens。把它设为你实际预期的最大响应长度，而不是模型上限——不设约束的上限加上啰嗦的输出习惯，正是廉价负载悄悄变贵的常见原因。" },
      ] },
      { h2: "发出你的第一个 Haiku 请求", blocks: [
        { type: "steps", items: [
          "注册账户并在控制台生成密钥——形如 sk-pool-…，所有支持的 Claude、GPT、Gemini 和 Kimi 模型通用。",
          `把任意 Anthropic 兼容客户端指向路由器：将 ANTHROPIC_BASE_URL 设为 ${BASE}，ANTHROPIC_API_KEY 设为你的密钥。官方 SDK 无需其他改动。`,
          "发送 POST /v1/messages，带上 x-api-key 和 anthropic-version 请求头，model 填 claude-haiku-4-5，并显式设置 max_tokens。",
        ] },
        { type: "code", code:
`curl ${BASE}/v1/messages \\
  -H "x-api-key: ${KEY}" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "content-type: application/json" \\
  -d '{
    "model": "claude-haiku-4-5",
    "max_tokens": 256,
    "messages": [
      {"role": "user", "content": "Classify this ticket as billing, bug or feature: \\"My invoice shows the wrong total.\\""}
    ]
  }'` },
        { type: "p", text: "返回的是标准 Messages 格式：一个内容块数组，外加包含输入、输出 token 数的 usage 对象。这份 usage 就是余额扣费的依据——按 Anthropic 官方费率计量，再减去统一的 50% 折扣后扣减。" },
      ] },
      { h2: "Haiku 单次、每百万、每月分别花多少钱", blocks: [
        { type: "p", text: "官方定价为每百万输入 token $1、每百万输出 token $5；本站折后为 $0.50/$2.50。输出 token 的价格是输入的五倍，所以在话多的负载上，控制响应长度比精简提示词更省钱。" },
        { type: "table", headers: ["计费项", "官方（$ / 百万 token）", "本站（−50%）"], rows: [
          ["输入", "$1.00", "$0.50"],
          ["输出", "$5.00", "$2.50"],
          ["缓存写入（5 分钟）", "$1.25", "$0.625"],
          ["缓存读取", "$0.10", "$0.05"],
        ] },
        { type: "p", text: "算一笔具体的账：一次分类调用，提示词 600 token、回答 80 token，共计量 680 token，折后成本约 $0.0005。每月十万次这样的调用约为 $50——到了这个量级，按 token 计价就不再是抽象概念，而是账单上实实在在的一行。" },
        { type: "link", text: "用免费成本计算器估算你自己的用量", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "用流式输出压低首 token 延迟", blocks: [
        { type: "p", text: "设置 \"stream\": true 后，端点返回的是 server-sent events 而非一次性阻塞响应：先是 message_start，随后是一系列携带生成文本的 content_block_delta 事件，最后以带有最终 usage 的 message_delta 和 message_stop 收尾。边收到边渲染，聊天界面的感知延迟就降到了首个 token——这正是 Haiku 速度优势的用武之地。流式改变的是延迟感受，不是价格：两种方式计量的 token 完全相同。" },
        { type: "note", text: "权威的 token 计数要以结尾的 message_delta 事件为准，不要自己数 delta。如果流在生成中途断开，不要密集重试：发一次新请求，并根据已收到的 usage 核对花费即可。" },
      ] },
      { h2: "缓存每次调用都在重发的前缀", blocks: [
        { type: "p", text: "高并发循环每次都在重发相同的前缀：系统提示词、标签定义、few-shot 示例。在这段稳定前缀的末尾打上 cache_control 断点，Anthropic 会把它放进一个短期缓存——TTL 五分钟，每次命中刷新。写入按输入费率的 1.25 倍计费，之后的每次读取只按 0.1 倍计费，50% 折扣在此基础上继续叠加。" },
        { type: "p", text: "把断点放在最后一个永不变化的内容块之后，每次请求不同的内容放在断点之后。打在每次都变的文本上的断点永远不会命中，只会白白付出写入溢价——按 Haiku 的费率这笔溢价不大，但放大到高并发就是纯粹的浪费。" },
        { type: "link", text: "Claude Haiku 4.5 价格详解（缓存费率、上下文、FAQ）", href: "/models/claude-haiku-4-5" },
      ] },
      { h2: "同一把密钥，把难的那部分升级到 Sonnet 或 Opus", blocks: [
        { type: "p", text: "一把密钥、一份余额覆盖所有支持的模型，所以路由只是客户端的一个 if 判断，不是一个基础设施工程。把大部分流量发给 Haiku；当输入很长、置信度偏低或任务确实需要多步推理时，把同一个 messages 数组换个 model 字段重发即可——claude-sonnet-5 或某个 Opus ID。生产流量大多数是简单流量，所以大部分花费停留在 Haiku 费率，而难题依然能拿到更强的模型。" },
        { type: "code", code:
`import anthropic

client = anthropic.Anthropic(
    base_url="${BASE}",
    api_key="${KEY}",
)

def answer(question: str) -> str:
    triage = client.messages.create(
        model="claude-haiku-4-5",
        max_tokens=8,
        system="Reply with one word: EASY for a simple lookup or short task, HARD if it needs multi-step reasoning.",
        messages=[{"role": "user", "content": question}],
    )
    verdict = triage.content[0].text.strip().upper()
    model = "claude-sonnet-5" if verdict.startswith("HARD") else "claude-haiku-4-5"
    reply = client.messages.create(
        model=model,
        max_tokens=1024,
        messages=[{"role": "user", "content": question}],
    )
    return reply.content[0].text` },
        { type: "note", text: "分诊本身也是一次 Haiku 调用，所以只有负载确实难易不均时才值得路由。如果 95% 的请求都很简单，直接全量调 Haiku 比为每个请求多付一次分诊往返更便宜。" },
        { type: "note", text: "通过 Google 或 GitHub 注册的新账户自带 $5 平台奖励余额——支持的所有 Claude、GPT、Gemini 和 Kimi 模型均可使用；邮箱密码注册的账户不享受该奖励。" },
      ] },
    ],
    faq: [
      { q: "Claude Haiku 4.5 在 API 中的模型 ID 是什么？", a: "claude-haiku-4-5。把它填进标准 Messages API 请求的 model 字段，带上 x-api-key 和 anthropic-version 请求头即可——请求格式与 Sonnet、Opus 完全相同。" },
      { q: "Claude Haiku API 每百万 token 多少钱？", a: "官方为每百万输入 token $1、每百万输出 token $5。在 apitoken.sale 上，每次请求按官方费率减去统一 50% 计费，即 $0.50/$2.50，缓存读取按输入费率的十分之一计量。" },
      { q: "写代码用 Haiku 4.5 够吗，还是必须用 Sonnet？", a: "Haiku 适合高并发、低复杂度的工作——分类、抽取、路由、智能体粘合逻辑。日常编码和智能体工作流建议默认用 Sonnet；在同一把密钥下，你可以把每个请求路由到能胜任它的最便宜档位。" },
      { q: "Haiku 4.5 的上下文和输出上限是多少？", a: "上下文窗口 200K token，最大输出 64K token——均低于 Opus 和 Sonnet 产品线。此外每个请求都必须显式设置 max_tokens。" },
      { q: "可以在 Cursor、Claude Code 或 Anthropic SDK 中调用 Haiku API 吗？", a: "可以。任何 Anthropic 兼容客户端都行：把 base URL 指向 apitoken.sale 路由器，用 x-api-key 鉴权，其余配置保持不变。" },
      { q: "如何免费试用 Claude Haiku API？", a: "用 Google 或 GitHub 注册，账户自带 $5 平台奖励余额，可用于 Haiku 以及所有其他支持的 Claude、GPT、Gemini 和 Kimi 模型。邮箱密码注册的账户不享受该奖励。" },
    ],
  };
