import type { LocalizedContent } from "../learn";
import { BASE, KEY } from "../learn-shared";

export const content: LocalizedContent = {
  title: "Claude Sonnet API 接入",
  h1: "Claude Sonnet API 接入：Sonnet 5 与 Sonnet 4.6",
  description: "通过 apitoken.sale 接入 Claude Sonnet API：Sonnet 5 和 4.6 的模型 ID、Messages API 示例、流式输出、提示词缓存，以及统一按官方价 5 折计费。",
  keywords: ["claude sonnet api", "claude sonnet 5 api", "claude-sonnet-5", "claude sonnet api 价格", "claude sonnet 4.6 api", "sonnet api 密钥", "claude sonnet api 示例", "claude messages api 流式", "claude sonnet 提示词缓存", "最适合编码的 claude 模型", "claude api 免费额度", "免费试用 claude api"],
  dek: "Claude Sonnet API 是日常编码和智能体任务的默认档位——交互式编辑够快，真正的工具调用循环也够强。本文介绍线上可用的模型 ID、一次完整的 Messages API 调用、流式输出、提示词缓存，以及 Sonnet 在 apitoken.sale 上按官方价统一 5 折的实际成本。",
  sections: [
    { h2: "Claude Sonnet API：模型、ID 与限额", blocks: [
      { type: "p", text: "Claude Sonnet API 是 Anthropic 的均衡档位，走标准 Messages API：把模型 ID 和消息列表 POST 到 /v1/messages，返回文本、工具调用和 token 用量。在 apitoken.sale 上协议形态完全一致——把任何兼容 Anthropic 的客户端指向路由器的 base URL，用 x-api-key 鉴权，代码其余部分一行都不用改。同一份预付费余额下有两代 Sonnet 在线：claude-sonnet-5 和 claude-sonnet-4-6。" },
      { type: "table", headers: ["模型 ID", "上下文", "最大输出", "官方 输入 / 输出（$ / 1M）", "本站（−50%）"], rows: [
        ["claude-sonnet-5", "1M tokens", "128K tokens", "$3 / $15", "$1.50 / $7.50"],
        ["claude-sonnet-4-6", "1M tokens", "128K tokens", "$3 / $15", "$1.50 / $7.50"],
      ] },
    ] },
    { h2: "发出你的第一个 Sonnet 请求", blocks: [
      { type: "p", text: "只要调用过 Anthropic 的 Messages API，这就是同一个请求，只是换了 base URL 和密钥。一把 apitoken.sale 密钥覆盖平台上所有受支持的 Claude、GPT、Gemini 和 Kimi 模型，所以下面这个调用也是其他所有模型的模板。" },
      { type: "steps", items: [
        "注册账号并在控制台生成密钥——形如 sk-pool-…，签发后即可使用。",
        `向 ${BASE} 发送 POST /v1/messages，带上 x-api-key 和 anthropic-version 两个请求头，与直连 Anthropic 时完全相同。`,
        "把 model 字段设为 claude-sonnet-5（或 claude-sonnet-4-6），从响应的 usage 对象读取用量——费用按官方费率减 50% 从你的预付费余额中扣除。",
      ] },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "messages": [\n      {"role": "user", "content": "Refactor this function for readability."}\n    ]\n  }'` },
    ] },
    { h2: "Sonnet 5 还是 Sonnet 4.6：该发哪个 ID", blocks: [
      { type: "p", text: "两个 ID 标价相同，所以选择是行为层面的，与预算无关。Sonnet 5 编码和智能体能力更强，是所有新项目的合理默认；它当前处于官方介绍期费率，引擎始终在套用你的折扣之前先应用当时的有效费率。不传 thinking 参数时它默认启用自适应思考（adaptive thinking），推理深度随任务伸缩，而不是吃一份固定预算。Sonnet 4.6 同样支持自适应思考，effort 默认为 high；如果你的提示词、评测集和回归基线都是按它调好的，继续用它就是正确选择。" },
      { type: "list", items: [
        "新项目或没有明确偏好：选 claude-sonnet-5。",
        "提示词和评测套件是按 4.6 的行为调优的：留在 claude-sonnet-4-6，直到你重新建立基线。",
        "上下文窗口、输出上限、价格全都相同——以后迁移只需改一行模型 ID。",
      ] },
      { type: "link", text: "模型目录中的 Claude Sonnet 4.6", href: "/models/claude-sonnet-4-6" },
    ] },
    { h2: "Token 定价（含介绍期费率）", blocks: [
      { type: "p", text: "两代 Sonnet 的标准官方费率都是每 1M 输入 token $3、每 1M 输出 token $15；本站在此基础上统一打 5 折，即 $1.50 / $7.50。Anthropic 公布的 Sonnet 5 介绍期价格为 $2 / $10，持续到 2026-08-31，引擎始终先应用当时的有效官方费率再算你的折扣——所以介绍期内你的花费会自动跟随更低的官方费率。输出 token 价格是输入的五倍，因此在高频对话型负载里，控制响应长度比压缩提示词更能省钱。" },
      { type: "note", text: "介绍期结束后，有效官方费率会回到标准的 $3 / $15，你的折后花费也随之变动——你这边不需要换密钥、改套餐或改代码。" },
      { type: "link", text: "Claude Sonnet 5 详细定价（缓存费率、上下文、FAQ）", href: "/models/claude-sonnet-5" },
    ] },
    { h2: "用提示词缓存压低重复上下文的成本", blocks: [
      { type: "p", text: "智能体循环每轮都要重发同一段前缀：系统提示词、工具定义、仓库上下文。Messages API 允许你用 cache_control 断点标记这段前缀；Anthropic 随后会把它放进一个短生命周期缓存（TTL 五分钟，命中即刷新），之后对它的读取只按输入价的一个零头计费。在 Sonnet 负载上这是最大的单一降本杠杆，而且可以与 5 折折扣叠加。" },
      { type: "table", headers: ["缓存操作（$ / 1M）", "官方", "本站（−50%）"], rows: [
        ["5 分钟缓存写入", "$3.75", "$1.875"],
        ["缓存读取", "$0.30", "$0.15"],
      ] },
      { type: "p", text: "把断点放在最后一个稳定块之后——系统提示词加工具定义加检索到的上下文——把每轮变化的内容放在断点之后。打在每次调用都变化的文本上的断点永远不会命中，只会让你白付写入溢价。" },
      { type: "link", text: "在成本计算器中估算缓存负载", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "流式响应与长输出", blocks: [
      { type: "p", text: "设置 stream: true 后，API 返回 server-sent events 而不是一次阻塞式响应：先是 message_start，然后是一串携带逐字生成文本的 content_block_delta 事件，最后是 message_delta 和 message_stop。在 UI 里增量渲染这些 delta，并从结尾事件中取最终 token 用量——那才是计费的依据。流式改变的是延迟体感，不是价格：同样的 token 怎么传都按同样的量计费。" },
      { type: "p", text: "128K 的输出上限意味着一次完整文件重写或一次长结构化抽取都能装进单个响应。要有意识地使用这个余量——不设上限的 max_tokens 习惯加上冗长输出，正是便宜的 Sonnet 负载悄悄变贵的常见原因。" },
      { type: "note", text: "如果流在生成中途断开，不要在紧凑循环里反复重试：发起一次全新请求，并基于你实际收到的结尾 usage 核对花费。" },
    ] },
    { h2: "一份余额打通 Sonnet、Opus 和 Haiku", blocks: [
      { type: "p", text: "Sonnet 与目录中其余模型共用同一把密钥和同一份预付费余额，这让模型路由变得很简单：批量分类和抽取发给 Haiku，编码和智能体默认用 Sonnet，只有真正困难的推理才升级到 Opus。切换档位只是在相同请求结构上换一个模型 ID——不需要新凭证、不需要单独的计费关系，你和任何档位之间也没有等待名单。" },
      { type: "note", text: "通过 Google 或 GitHub 注册的新账号自带 $5 平台奖励余额——可用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码注册的账号不享受该奖励。" },
    ] },
  ],
  faq: [
    { q: "Claude Sonnet 5 在 API 里的模型 ID 是什么？", a: "claude-sonnet-5。把它作为 Messages API 请求的 model 字段传入即可；上一代是 claude-sonnet-4-6。两者都能在同一把 apitoken.sale 密钥上使用。" },
    { q: "Claude Sonnet API 每个 token 多少钱？", a: "标准官方费率为每 1M 输入 token $3、每 1M 输出 token $15，另列有持续到 2026-08-31 的介绍期价格 $2 / $10。apitoken.sale 对有效官方费率统一打 5 折，因此按标准费率计算的花费为 $1.50 / $7.50。" },
    { q: "Sonnet 足够胜任编码智能体吗，还是必须上 Opus？", a: "Sonnet 5 是日常编码和智能体工作流的推荐默认模型——接近 Opus 的质量，token 价格低得多。把 Opus 留给最难的推理和长时高风险会话。" },
    { q: "我能在 Cursor、Claude Code 或 Anthropic SDK 里用 Claude Sonnet API 吗？", a: "可以。任何兼容 Anthropic 的客户端都行：把 base URL 指向 apitoken.sale 路由器，用 x-api-key 鉴权，其余配置保持不变。" },
    { q: "Sonnet 支持提示词缓存和 1M token 上下文吗？", a: "Sonnet 5 和 Sonnet 4.6 都提供 1M token 上下文窗口、128K 最大输出和提示词缓存——缓存读取官方价为每 1M token $0.30，折后 $0.15。" },
    { q: "如何免费试用 Claude Sonnet API？", a: "用 Google 或 GitHub 注册，账号自带 $5 平台奖励余额，可用于 Sonnet 以及所有其他受支持的 Claude、GPT、Gemini 和 Kimi 模型。邮箱密码注册的账号不享受该奖励。" },
  ],
};
