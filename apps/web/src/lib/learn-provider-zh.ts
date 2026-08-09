import { learnProviderEn } from "./learn-provider-en";
import type { LearnBlock, LocalizedContent } from "./learn";

function sourceBlock(slug: string, sectionIndex: number, blockIndex: number): LearnBlock {
  const article = learnProviderEn.find((entry) => entry.slug === slug);
  if (!article) throw new Error("Unknown provider guide: " + slug);
  const block = article.sections[sectionIndex]?.blocks[blockIndex];
  if (!block) throw new Error("Missing provider guide block: " + slug + "/" + sectionIndex + "/" + blockIndex);
  return block;
}

export const learnProviderZh: Record<string, LocalizedContent> = {
  "how-to-buy-gpt-api-key": {
    title: "如何购买 GPT API 密钥",
    h1: "如何购买 GPT API 密钥",
    description: "购买预付费 GPT API 密钥，支持银行卡或加密货币付款，通过 OpenAI 兼容端点使用 GPT-5.6、GPT-5.5 和 GPT Image 2，官方费用五折。",
    keywords: ["购买 gpt api 密钥", "gpt api 密钥", "购买 openai api", "gpt-5.6 api", "openai 兼容 api", "预付费 gpt api"],
    dek: "一个 apiToken.sale 密钥即可使用 GPT 目录，无需单独的 OpenAI Platform 账户。充值后设置 OpenAI 兼容端点，每次请求按官方费用的 50% 结算。",
    sections: [
      { h2: "三步获取 GPT 密钥", blocks: [
        { type: "steps", items: [
          "创建 apiToken.sale 账户，在仪表板生成密钥。",
          "使用银行卡或加密货币充值任意整数美元，无固定套餐或月费。",
          "将 base URL 设为 https://router.apitoken.sale/v1，使用 Authorization: Bearer，并从 GET /v1/models 选择模型。",
        ] },
        sourceBlock("how-to-buy-gpt-api-key", 0, 1),
      ] },
      { h2: "密钥包含哪些能力", blocks: [
        { type: "list", items: [
          "Responses 与 Chat Completions，均支持增量 SSE 流。",
          "GPT-5.6 Sol、Terra、Luna、旧版 GPT，以及独立的 GPT Image 2 路由。",
          "同一密钥和余额也可用于支持的 Claude、Gemini 与 Kimi 模型。",
          "每次请求均按官方费用享受固定 50% B2C 折扣。",
        ] },
        { type: "note", text: "请把密钥放在服务端环境变量中。GPT 使用 Authorization: Bearer；x-api-key 与 x-goog-api-key 分别属于 Anthropic 和 Gemini 协议。" },
      ] },
    ],
    faq: [
      { q: "需要 OpenAI 账户吗？", a: "不需要。密钥、余额和计费都由 apiToken.sale 提供，客户端只需自定义 base URL 和 Bearer 密钥。" },
      { q: "一个密钥能同时调用 GPT 和 Claude 吗？", a: "可以。同一个 sk-pool 密钥和余额覆盖所有支持的提供商，只需切换端点和认证头。" },
      { q: "这是 OpenAI Platform 吗？", a: "不是。这是独立的 OpenAI 兼容网关，拥有自己的账户、预付余额和模型目录。" },
    ],
  },
  "gpt-api-pricing": {
    title: "GPT API 定价详解",
    h1: "GPT API 定价：输入、缓存、输出与长上下文",
    description: "了解 GPT-5.6 Sol、Terra、Luna 的输入、缓存输入、缓存写入、输出和长上下文价格，以及 apiToken.sale 固定五折。",
    keywords: ["gpt api 定价", "gpt-5.6 价格", "gpt api 成本", "gpt token 价格", "gpt-5.6 sol 价格", "便宜 gpt api"],
    dek: "GPT 成本由精确的 token 项组成，而不是按请求收费。模型层级、缓存 token 和输入长度决定官方费用，apiToken.sale 再减免 50%。",
    sections: [
      { h2: "当前 GPT-5.6 费率", blocks: [
        { type: "table", headers: ["模型", "官方输入 / 缓存 / 输出", "五折后价格"], rows: [
          ["gpt-5.6-sol", "$5 / $0.50 / $30", "$2.50 / $0.25 / $15"],
          ["gpt-5.6-terra", "$2 / $0.20 / $12", "$1 / $0.10 / $6"],
          ["gpt-5.6-luna", "$0.20 / $0.02 / $1.20", "$0.10 / $0.01 / $0.60"],
        ] },
        { type: "p", text: "以上均为每 100 万 token。gpt-5.6 是 gpt-5.6-sol 的别名，价格相同，并非独立费率。" },
      ] },
      { h2: "缓存写入与长上下文", blocks: [
        { type: "list", items: [
          "GPT-5.6 缓存写入按普通输入的 125% 计费，缓存读取按 10% 计费。",
          "输入超过 272K token 后，整次请求使用 2 倍输入和 1.5 倍输出费率。",
          "推理 token 已包含在输出中，不会作为额外项目重复收费。",
          "仪表板记录终态 usage 与折扣后的精确扣费。",
        ] },
        { type: "note", text: "切换层级通常比压缩提示词节省更多：Terra 每 token 是 Sol 的 40%，Luna 仅为 4%。应按任务难度路由。" },
      ] },
    ],
    faq: [
      { q: "GPT-5.6 每 100 万 token 多少钱？", a: "官方 Sol 为 $5 输入/$30 输出，Terra 为 $2/$12，Luna 为 $0.20/$1.20；apiToken.sale 对每个项目固定五折。" },
      { q: "什么是 cached input？", a: "由提供商缓存命中的重复提示前缀。同一个 token 不会同时按缓存输入和新输入收费。" },
      { q: "长上下文费率何时生效？", a: "输入超过 272K token 时，整次请求使用 2 倍输入和 1.5 倍输出费率，然后再应用折扣。" },
    ],
  },
  "gpt-5-6-sol-vs-terra-vs-luna": {
    title: "GPT-5.6 Sol、Terra 与 Luna 对比",
    h1: "GPT-5.6 Sol、Terra 与 Luna 对比",
    description: "从价格、推理强度、上下文和适用场景比较 GPT-5.6 Sol、Terra 与 Luna，为编程和生产任务选择合适模型。",
    keywords: ["gpt-5.6 sol 对比 terra", "gpt-5.6 terra 对比 luna", "最佳 gpt-5.6 模型", "gpt-5.6 模型", "gpt-5.6 对比", "编程 gpt 模型"],
    dek: "GPT-5.6 家族共享 400K 上下文、128K 最大输出和完整推理强度范围。实际差异在于每个 token 购买的能力与速度。",
    sections: [
      { h2: "按任务选择", blocks: [
        { type: "table", headers: ["层级", "适合场景", "官方输入 / 输出"], rows: [
          ["Sol", "高难推理、长周期代理、复杂代码审查", "$5 / $30"],
          ["Terra", "日常编程、生产对话、均衡代理", "$2 / $12"],
          ["Luna", "分类、抽取、路由和大批量简单任务", "$0.20 / $1.20"],
        ] },
        { type: "p", text: "Terra 是稳妥默认项：保留 Sol 的控制能力和上下文，token 价格仅 40%。评测显示质量不足时升级 Sol，确定性批量任务交给 Luna。" },
      ] },
      { h2: "三者共同点", blocks: [
        { type: "list", items: [
          "400K 上下文，最大输出 128K。",
          "文本和图像输入，文本输出。",
          "Responses 与 Chat Completions 均支持 SSE。",
          "GPT-5.6 家族支持从 none 到 max 的推理强度。",
          "同一端点、密钥和余额可按任务切换模型。",
        ] },
      ] },
    ],
    faq: [
      { q: "哪款 GPT-5.6 最适合编程？", a: "日常编程从 Terra 开始；最难的架构和代理任务用 Sol，便宜的确定性子任务用 Luna。" },
      { q: "Sol、Terra、Luna 需要不同端点吗？", a: "不需要。三者共用 OpenAI 兼容 base URL 和密钥，只修改 model ID。" },
      { q: "Terra 支持 max 推理强度吗？", a: "支持。Sol、Terra 与 Luna 使用同一套 GPT-5.6 推理强度，包括 max。" },
    ],
  },
  "gpt-image-2-api-guide": {
    title: "GPT Image 2 API 指南",
    h1: "使用 GPT Image 2 API 生成和编辑图像",
    description: "通过 apiToken.sale 使用 GPT Image 2：准确端点、model ID、参考图限制、token 定价与固定五折。",
    keywords: ["gpt image 2 api", "gpt-image-2", "openai 图像生成 api", "gpt 图像编辑 api", "gpt image 价格", "图像生成 api"],
    dek: "GPT Image 2 使用独立图像路由，但与 GPT 文本模型共享 apiToken.sale 密钥和余额。可通过提示词生成图像，也可编辑最多五张 PNG 参考图。",
    sections: [
      { h2: "调用生成路由", blocks: [
        sourceBlock("gpt-image-2-api-guide", 0, 0),
        { type: "p", text: "编辑时向 /v1/images/edits 发送 multipart/form-data，使用同一模型并最多附带五张 PNG。当前接口每次返回一张非流式 PNG。" },
      ] },
      { h2: "图像计费方式", blocks: [
        { type: "table", headers: ["计费项", "官方每 100 万 token", "本站价格"], rows: [
          ["文本输入", "$5", "$2.50"],
          ["图像输入", "$8", "$4"],
          ["图像输出", "$30", "$15"],
        ] },
        { type: "list", items: [
          "缓存文本和图像输入按普通输入的 25% 计费。",
          "gpt-image-2 是固定快照 gpt-image-2-2026-04-21 的别名。",
          "图像 usage 与 GPT、Claude、Gemini 请求共用预付余额。",
        ] },
      ] },
    ],
    faq: [
      { q: "GPT Image 2 使用什么端点？", a: "新图像使用 POST /v1/images/generations，参考图编辑使用 POST /v1/images/edits。" },
      { q: "GPT Image 2 能编辑现有图像吗？", a: "可以。edits 路由通过 multipart/form-data 接受最多五张 PNG 参考图。" },
      { q: "需要单独的图像密钥或余额吗？", a: "不需要。它使用与其他模型相同的 Bearer 密钥和预付余额。" },
    ],
  },
  "how-to-buy-gemini-api-key": {
    title: "如何购买 Gemini API 密钥",
    h1: "如何购买 Gemini API 密钥",
    description: "购买预付费 Gemini API 密钥，支持银行卡或加密货币付款，使用原生 Gemini 端点，并以一个账户调用 Gemini、GPT、Claude 和 Kimi，官方费用五折。",
    keywords: ["购买 gemini api 密钥", "gemini api 密钥", "google gemini api", "预付费 gemini api", "gemini api 付款", "便宜 gemini api"],
    dek: "apiToken.sale 密钥无需单独配置 Google Cloud 计费即可访问原生 Gemini API。充值一次，密钥通过 x-goog-api-key 发送，并与所有支持的提供商共享余额。",
    sections: [
      { h2: "三步获取 Gemini 密钥", blocks: [
        { type: "steps", items: [
          "创建 apiToken.sale 账户，在仪表板生成 sk-pool 密钥。",
          "使用银行卡或加密货币充值任意整数美元，余额不会过期。",
          "将 Gemini base URL 设为 https://router.apitoken.sale，通过 x-goog-api-key 认证，并从 GET /v1beta/models 选择模型。",
        ] },
        sourceBlock("how-to-buy-gemini-api-key", 0, 1),
      ] },
      { h2: "可用能力", blocks: [
        { type: "list", items: [
          "原生 Gemini 协议上的 Pro、Flash 和 Flash-Lite 文本模型。",
          "Gemini 3.1 Flash Image（Nano Banana 2）图像生成。",
          "Google 形状的 generateContent、streamGenerateContent 和 countTokens。",
          "固定 50% B2C 折扣，并与 GPT、Claude、Kimi 共用密钥和余额。",
        ] },
        { type: "note", text: "Google SDK 的 base URL 应填写裸域名。SDK 会自行附加 /v1beta；重复前缀会返回 404。" },
      ] },
    ],
    faq: [
      { q: "需要 Google Cloud 项目吗？", a: "不需要。网关账户和计费由 apiToken.sale 管理，客户端只需自定义 base URL 和 sk-pool 密钥。" },
      { q: "Gemini 使用哪个认证头？", a: "x-goog-api-key。原生 Gemini 路由不使用 Anthropic x-api-key 或 OpenAI Authorization: Bearer。" },
      { q: "同一密钥能调用 GPT 与 Gemini 吗？", a: "可以。密钥和余额共享，只需按提供商切换端点、协议和 model ID。" },
    ],
  },
  "gemini-api-quickstart": {
    title: "Gemini API 快速入门",
    h1: "Gemini API 快速入门：curl 与 Google GenAI SDK",
    description: "通过 curl 或 Google GenAI SDK 发起首个 Gemini API 请求：原生 generateContent、x-goog-api-key 和明确的 Gemini model ID。",
    keywords: ["gemini api 快速入门", "gemini api 教程", "google genai sdk base url", "gemini generatecontent", "gemini api curl", "gemini api 示例"],
    dek: "网关保留原生 Google Gemini 协议。只需修改 base URL 和 API key，继续使用 generateContent 与官方 SDK 结构，并始终明确指定模型。",
    sections: [
      { h2: "使用 curl 发起首个请求", blocks: [
        sourceBlock("gemini-api-quickstart", 0, 0),
        { type: "p", text: "增量输出使用 streamGenerateContent?alt=sse。生成前可在同一模型路径调用 countTokens，免费估算输入 token。" },
      ] },
      { h2: "使用官方 Python SDK", blocks: [
        sourceBlock("gemini-api-quickstart", 1, 0),
        { type: "list", items: [
          "SDK 配置只传裸 base URL，不要附加 /v1beta。",
          "明确传入 model ID；客户端自动默认模型可能不在网关目录中。",
          "把 APITOKEN_API_KEY 放在环境变量中，不要写入源码。",
        ] },
      ] },
    ],
    faq: [
      { q: "官方 Google GenAI SDK 能用吗？", a: "可以。将 HttpOptions(base_url) 设为 https://router.apitoken.sale 并提供 apiToken.sale 密钥，请求与响应结构保持原生。" },
      { q: "如何流式输出 Gemini？", a: "使用 /v1beta/models/{model}:streamGenerateContent?alt=sse 与 x-goog-api-key，或 SDK 对应的流式方法。" },
      { q: "为什么重复 /v1beta 会 404？", a: "Google SDK 会自动添加 API 版本。只配置裸域名，最终 URL 中应只有一个 /v1beta。" },
    ],
  },
  "gemini-api-pricing": {
    title: "Gemini API 定价详解",
    h1: "Gemini API 定价：Pro、Flash、Flash-Lite 与图像输出",
    description: "比较 Gemini Pro、Flash、Flash-Lite 和 Nano Banana 2 的价格，包括缓存输入、长上下文、图像输出和 apiToken.sale 固定五折。",
    keywords: ["gemini api 定价", "gemini api 成本", "gemini token 价格", "gemini flash 价格", "gemini pro 价格", "便宜 gemini api"],
    dek: "Gemini 价格取决于模型层级、缓存输入、输出模态，以及 Pro 的上下文长度。网关精确结算官方计费项，再应用 50% 折扣。",
    sections: [
      { h2: "代表性文本模型费率", blocks: [
        { type: "table", headers: ["模型", "官方输入 / 缓存 / 输出", "五折后价格"], rows: [
          ["gemini-3.1-pro-preview", "$2 / $0.20 / $12", "$1 / $0.10 / $6"],
          ["gemini-3.6-flash", "$1.50 / $0.15 / $7.50", "$0.75 / $0.075 / $3.75"],
          ["gemini-3.1-flash-lite", "$0.25 / $0.025 / $1.50", "$0.125 / $0.0125 / $0.75"],
          ["gemini-2.5-flash-lite", "$0.10 / $0.01 / $0.40", "$0.05 / $0.005 / $0.20"],
        ] },
        { type: "p", text: "以上均为每 100 万 token。缓存输入是提供商报告的独立 usage 项，不会与同一批新输入 token 重复相加。" },
      ] },
      { h2: "长上下文与图像", blocks: [
        { type: "list", items: [
          "Gemini 3.1 Pro Preview 输入超过 200K 后，整次请求按 $4 输入/$18 输出每 100 万计费。",
          "Gemini 3.1 Flash Image 的文本输出为 $3，图像输出为 $60 每 100 万 image token。",
          "Flash Image 的缓存输入按完整输入费率收费，不享受文本模型缓存折扣。",
          "精确计算官方项目后，再应用固定 50% B2C 折扣。",
        ] },
      ] },
    ],
    faq: [
      { q: "最便宜的 Gemini 模型是什么？", a: "在已发布文本层级中，Gemini 2.5 Flash-Lite 官方为 $0.10 输入/$0.40 输出，五折后为 $0.05/$0.20。" },
      { q: "Gemini 长上下文何时加价？", a: "Gemini 3.1 Pro Preview 输入超过 200K token 时，整次请求使用更高输入、缓存和输出费率。" },
      { q: "Gemini 图像输出如何计费？", a: "Gemini 3.1 Flash Image 官方为 $60/百万 image-output token，五折后为 $30。" },
    ],
  },
  "gemini-pro-vs-flash-vs-flash-lite": {
    title: "Gemini Pro、Flash 与 Flash-Lite 对比",
    h1: "Gemini Pro、Flash 与 Flash-Lite 对比",
    description: "从价格、上下文、推理与适用场景比较 Gemini Pro、Flash 和 Flash-Lite，为编程、代理和大规模 API 选择模型。",
    keywords: ["gemini pro 对比 flash", "gemini flash 对比 flash lite", "最佳 gemini 模型", "gemini 模型对比", "编程 gemini 模型", "gemini 3.6 flash"],
    dek: "将模型层级作为路由选择：Pro 处理最难推理，Flash 作为编程默认，Flash-Lite 处理便宜的大规模步骤。一个密钥即可使用三者。",
    sections: [
      { h2: "按任务选择", blocks: [
        { type: "table", headers: ["层级", "适合场景", "推荐当前 ID"], rows: [
          ["Pro", "高难推理、规划、深度代码库和文档分析", "gemini-3.1-pro-preview"],
          ["Flash", "日常编程、多模态代理、均衡生产流量", "gemini-3.6-flash"],
          ["Flash-Lite", "分类、抽取、路由和便宜预处理", "gemini-3.1-flash-lite"],
          ["Image", "图像生成与编辑", "gemini-3.1-flash-image"],
        ] },
        { type: "p", text: "Gemini 3.6 Flash 是多数新文本任务的最佳起点。仅把最难请求升级到 Pro，把确定性批量任务降到 Flash-Lite。" },
      ] },
      { h2: "上下文与成本取舍", blocks: [
        { type: "list", items: [
          "当前文本模型提供 1M 上下文和最多 64K 输出。",
          "Pro 在 200K 输入后有长上下文溢价；Flash 与 Flash-Lite 在窗口内保持固定费率。",
          "文本模型缓存输入通常是新输入价格的 10%。",
          "大请求前使用 countTokens，并依据实际评测而非模型名称路由。",
        ] },
      ] },
    ],
    faq: [
      { q: "哪款 Gemini 最适合编程？", a: "从 Gemini 3.6 Flash 开始。复杂架构和审查升级到 3.1 Pro Preview，便宜的确定性步骤用 Flash-Lite。" },
      { q: "Flash-Lite 上下文更小吗？", a: "不是。已发布文本 Flash-Lite 保留 1M 上下文，优势是简单任务上的成本和延迟。" },
      { q: "切换层级需要新密钥吗？", a: "不需要。保持同一 Gemini base URL 与 x-goog-api-key，只修改 model ID。" },
    ],
  },
  "nano-banana-2-api-guide": {
    title: "Nano Banana 2 API 指南",
    h1: "使用 Nano Banana 2 API 生成图像",
    description: "通过原生 Gemini API 使用 Gemini 3.1 Flash Image（Nano Banana 2）：准确 model ID、generateContent、图像输出定价和固定五折。",
    keywords: ["nano banana 2 api", "gemini 3.1 flash image api", "gemini 图像生成 api", "nano banana api 密钥", "gemini 图像价格", "google 图像 api"],
    dek: "Nano Banana 2 是 Gemini 3.1 Flash Image 的公开名称。它使用原生 generateContent，接受多模态输入，并与文本模型共用余额返回渲染图像。",
    sections: [
      { h2: "使用准确 model ID", blocks: [
        sourceBlock("nano-banana-2-api-guide", 0, 0),
        { type: "p", text: "按 MIME type 解析返回 parts：文本 part 是说明，图像 part 是渲染资产。API 中使用 gemini-3.1-flash-image，而不是营销昵称。" },
      ] },
      { h2: "限制与价格", blocks: [
        { type: "list", items: [
          "128K 上下文，最多 32K 输出，小于文本 Flash 系列。",
          "官方文本输入/输出为 $0.50/$3 每百万，图像输出为 $60。",
          "apiToken.sale 五折后为 $0.25/$1.50，图像输出 $30。",
          "该图像模型的缓存输入仍按完整 $0.50 输入费率计费。",
        ] },
        { type: "note", text: "只需文本时使用文本 Flash。只有响应必须包含渲染图像时才使用 Flash Image，其图像输出单独计费。" },
      ] },
    ],
    faq: [
      { q: "Nano Banana 2 的 API model ID 是什么？", a: "原生 Gemini generateContent 路由上的 gemini-3.1-flash-image。" },
      { q: "Nano Banana 2 图像输出多少钱？", a: "官方 $60/百万 image-output token，apiToken.sale 固定五折后 $30。" },
      { q: "需要单独图像 API 密钥吗？", a: "不需要。使用同一 sk-pool 密钥放在 x-goog-api-key 中，并共享预付余额。" },
    ],
  },
  "how-to-buy-kimi-api-key": {
    title: "如何购买 Kimi API 密钥",
    h1: "如何购买 Kimi API 密钥",
    description: "购买一个预付费 API 密钥，通过 Anthropic Messages 或 OpenAI 兼容客户端使用 Kimi K3 和 Kimi for Coding，官方 API 费用五折。",
    keywords: ["购买 kimi api 密钥", "kimi api 密钥", "kimi k3 api", "kimi for coding api", "moonshot kimi api", "预付费 kimi api"],
    dek: "Kimi 以独立模型命名空间发布在统一路由器上。可使用原生 Anthropic Messages 路由或 OpenAI 兼容客户端，并与 Claude、GPT、Gemini 共享预付余额。",
    sections: [
      { h2: "三步获取访问权限", blocks: [
        { type: "steps", items: [
          "创建 apiToken.sale 账户并生成 sk-pool 密钥。",
          "使用银行卡或加密货币充值任意整数美元，用户侧无需另购 Kimi 套餐。",
          "读取 GET https://router.apitoken.sale/v1/models，从密钥的实时目录选择 kimi/* ID。",
        ] },
        sourceBlock("how-to-buy-kimi-api-key", 0, 1),
      ] },
      { h2: "Kimi 路由有何不同", blocks: [
        { type: "list", items: [
          "Kimi 是独立提供商命名空间，而不是第四种 wire format：可使用 POST /v1/messages 与 x-api-key，或统一 OpenAI 兼容 /v1 路由。",
          "公开 ID 是 kimi/k3、kimi/kimi-for-coding 等订阅别名，不是内部费率模型名。",
          "K3 有 256K 与 1M 上下文写法，Kimi for Coding 有普通与 High Speed 别名。",
          "实时 /v1/models 是权威来源，因为可用性受提供商容量和密钥策略影响。",
        ] },
      ] },
    ],
    faq: [
      { q: "Kimi 需要单独 API 密钥吗？", a: "不需要。同一个 sk-pool 密钥和余额覆盖 Kimi 与其他支持的提供商。" },
      { q: "Kimi 使用哪个端点？", a: "Anthropic Messages 使用 https://router.apitoken.sale/v1/messages；OpenAI 兼容客户端使用 /v1 Chat Completions。两者都接受公开 kimi/* ID。" },
      { q: "为什么先检查 /v1/models？", a: "目录按密钥作用域返回当前可路由且可定价的模型。" },
    ],
  },
  "kimi-api-quickstart": {
    title: "Kimi API 快速入门",
    h1: "使用 Anthropic SDK 快速接入 Kimi API",
    description: "通过 apiToken.sale 调用 Kimi K3 与 Kimi for Coding：Anthropic Messages、x-api-key、命名空间 model ID、流式输出和共享余额。",
    keywords: ["kimi api 快速入门", "kimi api 教程", "kimi anthropic api", "kimi k3 api 示例", "kimi for coding api", "kimi api curl"],
    dek: "Kimi 在统一路由器上使用 Anthropic Messages 协议。现有 Anthropic 客户端只需自定义 base URL、apiToken.sale 密钥与明确的 kimi/* model ID。",
    sections: [
      { h2: "使用 curl 发起首个请求", blocks: [
        sourceBlock("kimi-api-quickstart", 0, 0),
        { type: "p", text: "设置 stream: true 即可获得增量 SSE。终态 usage 采用 Anthropic 结构，因此现有 usage 解析器可以继续使用。" },
      ] },
      { h2: "使用 Anthropic Python SDK", blocks: [
        sourceBlock("kimi-api-quickstart", 1, 0),
        { type: "note", text: "不要替换成 kimi-k2.7-code 等 Open Platform ID。公开路由器接受 GET /v1/models 返回的订阅别名；OpenAI 兼容客户端可通过统一 /v1 路由调用相同 Kimi 别名。" },
      ] },
    ],
    faq: [
      { q: "Anthropic SDK 能调用 Kimi 吗？", a: "可以。将 base_url 指向 https://router.apitoken.sale，并从按密钥目录中选择 kimi/* model ID。" },
      { q: "Kimi 支持流式输出吗？", a: "支持。设置 stream: true，消费标准的增量 Anthropic SSE 事件。" },
      { q: "应该从哪个 model ID 开始？", a: "编程默认选 kimi/kimi-for-coding；需要 K3 推理但不需要 1M 窗口时选 kimi/k3-256k。" },
    ],
  },
  "kimi-api-pricing": {
    title: "Kimi API 定价详解",
    h1: "Kimi API 定价：缓存命中、未命中、输出与速度",
    description: "了解 Kimi K3、Kimi for Coding 与 High Speed 的缓存命中、未命中、输出费率、别名映射和 apiToken.sale 固定五折。",
    keywords: ["kimi api 定价", "kimi k3 价格", "kimi for coding 价格", "kimi token 成本", "kimi k2.7 code 价格", "便宜 kimi api"],
    dek: "Kimi 分别公布缓存命中、缓存未命中和输出费率。apiToken.sale 按实际服务模型定价，保持计费项互斥，再应用固定 50% 折扣。",
    sections: [
      { h2: "公开别名对应的官方费率", blocks: [
        { type: "table", headers: ["公开别名", "官方命中 / 未命中 / 输出", "五折后价格"], rows: [
          ["kimi/k3 · k3-256k · k3[1m]", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
          ["kimi/kimi-for-coding", "$0.19 / $0.95 / $4", "$0.095 / $0.475 / $2"],
          ["kimi/kimi-for-coding-highspeed", "$0.38 / $1.90 / $8", "$0.19 / $0.95 / $4"],
        ] },
        { type: "p", text: "以上均为每 100 万 token。Kimi 自动缓存，且没有独立缓存写入价格；新缓存 token 视为未命中，而不是免费或隐藏的第四项。" },
      ] },
      { h2: "如何控制成本", blocks: [
        { type: "list", items: [
          "Kimi for Coding 是公开 Kimi 集合中成本最低的通用编程选项。",
          "只有延迟收益值得两倍 token 费率时才使用 High Speed。",
          "任务不需要大窗口时，选择 k3-256k 而不是完整 1M 写法。",
          "设置密钥终身消费上限，并在仪表板检查终态 usage。",
        ] },
        { type: "note", text: "推理 token 是输出的子集，按输出费率结算，不会作为独立项目再次收费。" },
      ] },
    ],
    faq: [
      { q: "Kimi for Coding 多少钱？", a: "官方为 $0.19/百万缓存命中、$0.95/百万缓存未命中、$4/百万输出；apiToken.sale 收取一半。" },
      { q: "为什么缓存命中与未命中价格不同？", a: "Kimi 自动缓存重复上下文。终态 usage 标识缓存命中输入，每个互斥项目使用自己的官方费率。" },
      { q: "High Speed 更贵吗？", a: "是。缓存命中、未命中与输出费率均为基础 Kimi for Coding 的两倍。" },
    ],
  },
  "kimi-k3-vs-kimi-for-coding": {
    title: "Kimi K3 与 Kimi for Coding 对比",
    h1: "Kimi K3 与 Kimi for Coding 对比",
    description: "从上下文、推理控制、延迟和 token 价格比较 Kimi K3、K3 256K、Kimi for Coding 与 High Speed。",
    keywords: ["kimi k3 对比 kimi for coding", "kimi k3 api", "kimi k2.7 code", "最佳 kimi 编程模型", "kimi 模型对比", "kimi highspeed"],
    dek: "K3 面向推理与长上下文，Kimi for Coding 面向经济型编程。High Speed 用两倍费率换取延迟，K3 别名则选择 256K 或 1M 窗口。",
    sections: [
      { h2: "模型家族映射", blocks: [
        { type: "table", headers: ["公开 ID", "上下文", "适合场景"], rows: [
          ["kimi/kimi-for-coding", "256K", "日常编程与经济型代理循环"],
          ["kimi/kimi-for-coding-highspeed", "256K", "速度收益值得成本的低延迟编程"],
          ["kimi/k3-256k", "256K", "不需要完整窗口的 K3 推理"],
          ["kimi/k3 · kimi/k3[1m]", "1M", "大型代码库、文档与高难推理"],
        ] },
        { type: "p", text: "k3[1m] 是 K3 1M 模式的兼容写法，而不是独立模型。路由器会规范化为提供商实际接受的 k3。" },
      ] },
      { h2: "推理与路由", blocks: [
        { type: "list", items: [
          "K3 支持 low、high、max 推理强度，默认 high。",
          "Kimi for Coding 与 High Speed 始终启用 thinking。",
          "固定别名前先检查按密钥 /v1/models 目录。",
          "实用路由策略是日常代码用 Kimi for Coding，大型或困难工作升级到 K3。",
        ] },
      ] },
    ],
    faq: [
      { q: "哪款 Kimi 最适合编程？", a: "Kimi for Coding 是经济型默认。高难推理或长上下文选 K3，只有低延迟值得双倍费率时选 High Speed。" },
      { q: "k3 与 k3[1m] 是不同模型吗？", a: "不是。两者选择同一 K3 1M 模式，方括号形式是兼容别名。" },
      { q: "能直接请求内部官方模型 ID 吗？", a: "不能。请使用路由器目录返回的公开订阅别名，不要使用 kimi-k2.7-code 等费率 ID。" },
    ],
  },
  "kimi-api-for-opencode": {
    title: "在 OpenCode 中使用 Kimi API",
    h1: "在 OpenCode 中运行 Kimi K3 与 Kimi for Coding",
    description: "通过 apiToken.sale 将 OpenCode 连接到 Kimi：路由器插件、实时模型目录、明确 kimi/* ID、流式输出和一个预付费 API 密钥。",
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
  },
  "kimi-api-for-claude-code": {
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
  },
  "kimi-api-for-kimi-code": {
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
  },
};
