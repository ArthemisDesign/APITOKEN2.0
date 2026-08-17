import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Gemini API 快速入门",
    h1: "Gemini API 快速入门：用 curl 和 Google GenAI SDK 发起首个请求",
    description: "Gemini API 快速入门：通过 apiToken.sale 发起首个请求——用 curl 或 Google GenAI SDK 调用原生 generateContent，x-goog-api-key 鉴权、SSE 流式输出和显式 model ID。",
    keywords: ["gemini api 快速入门", "gemini api 教程", "gemini api curl 示例", "google genai sdk base url", "gemini generatecontent api", "gemini api python 示例", "gemini api 流式输出", "x-goog-api-key header", "如何调用 gemini api", "gemini api javascript sdk"],
    dek: "这份 Gemini API 快速入门带你在几分钟内跑通第一个可用请求：先对原生 generateContent 路由发一个 curl，再用官方 Google GenAI SDK（Python 或 JavaScript）完成同样的调用。只需改动 base URL 和密钥请求头——请求结构、流式输出和 usage 元数据与 Google 文档完全一致。",
    sections: [
      { h2: "一个端点，原生 Gemini 协议", blocks: [
        { type: "p", text: "要通过 apiToken.sale 发起第一个 Gemini API 请求，完全保留 Google 官方协议，只改两个值：base URL 换成 https://router.apitoken.sale，密钥换成你的 apiToken.sale 密钥，通过 x-goog-api-key 请求头发送。每个请求和响应都保持原生 generateContent 结构，因此 Google 官方文档、SDK 示例以及你现有的 Gemini 代码都可以原样使用。" },
        { type: "p", text: "一个密钥、一份预付余额覆盖所有支持的提供商——Gemini 与 Claude、GPT、Kimi 并列可用。Gemini 用量按 Google 官方 token 费率计量，从余额扣费前先应用固定 50% 折扣。你这边不需要任何 Google Cloud 项目或计费账户。" },
      ] },
      { h2: "创建密钥并查看你的模型目录", blocks: [
        { type: "steps", items: [
          "免费注册 apiToken.sale 账号并打开控制台——无需审批，没有 waitlist。",
          "生成一个 API 密钥。它形如 sk-pool-…，对 Gemini、Claude、GPT 和 Kimi 同样有效。",
          "用银行卡或加密货币充值任意整数美元金额；预付余额永不过期。",
          "将密钥导出为 APITOKEN_API_KEY，然后列出你的密钥实际可调用的模型：",
        ] },
        sourceBlock("gemini-api-quickstart", 1, 1),
        { type: "p", text: "从返回结果中挑一个显式的 model ID。gemini-3.6-flash 是首次文本调用的合适默认；客户端库内置的默认模型可能不在网关目录中，而路由器只服务它列出的 ID。" },
        { type: "note", text: "通过 Google 或 GitHub 注册的新账号会获得 $5 平台奖励金——可用于支持的 Claude、GPT、Gemini 和 Kimi 模型；用邮箱和密码注册的账号不享受该奖励。" },
      ] },
      { h2: "首个请求：用 curl 调用 generateContent", blocks: [
        sourceBlock("gemini-api-quickstart", 2, 0),
        { type: "p", text: "响应是标准的 Google 结构：读取 candidates[0].content.parts 并拼接其中的文本部分。同一份 JSON 还带有 usageMetadata，包含 prompt、candidate 和 total 的 token 计数，因此 token 与成本统计代码从第一次调用起就能工作。" },
        { type: "p", text: "发送大提示词之前，可以在同一模型路径上调用 :countTokens。它只返回 token 计数、不生成任何内容——在花钱生成之前免费估算输入量。" },
      ] },
      { h2: "用 streamGenerateContent 流式输出 token", blocks: [
        sourceBlock("gemini-api-quickstart", 3, 0),
        { type: "p", text: "?alt=sse 查询参数把响应切换为 server-sent events：每个事件是同一 candidate 结构中的一个增量 chunk，最后一个事件携带汇总的 usageMetadata。在 SDK 中，同一路由对应 Python 的 generate_content_stream 和 JavaScript 的 generateContentStream。" },
        { type: "p", text: "凡是面向用户的输出都用流式，让首批 token 立即渲染。对于只关心最终文本的批处理任务，普通的 generateContent 更易于解析和重试。" },
      ] },
      { h2: "官方 SDK：Python 和 JavaScript", blocks: [
        sourceBlock("gemini-api-quickstart", 4, 0),
        sourceBlock("gemini-api-quickstart", 4, 1),
        { type: "list", items: [
          "传裸 base URL https://router.apitoken.sale；SDK 配置中不要再附加 /v1beta。",
          "传具体的 model ID，比如 gemini-3.6-flash——永远不要依赖客户端默认值。",
          "把 APITOKEN_API_KEY 放在环境变量里，而不是写进源码。",
        ] },
        { type: "note", text: "如果 SDK 的每个请求都返回 404，检查请求路径里是否出现了重复的 /v1beta/v1beta 段。SDK 会自己拼接 API 版本；如果配置的 host 已包含 /v1beta，就会产生重复路径。" },
      ] },
      { h2: "前几次调用的成本", blocks: [
        { type: "p", text: "Gemini 调用按 Google 官方费率项精确结算——输入、缓存输入和输出——再叠加固定 50% 折扣。常用文本模型折后每 100 万 token 的价格：" },
        { type: "table", headers: ["模型", "每 100 万 token 输入 / 缓存 / 输出", "适合的首个任务"], rows: [
          ["gemini-3.6-flash", "$0.75 / $0.075 / $3.75", "日常编程、聊天和智能体"],
          ["gemini-3.1-flash-lite", "$0.125 / $0.0125 / $0.75", "分类、抽取、路由"],
          ["gemini-2.5-flash-lite", "$0.05 / $0.005 / $0.20", "最便宜的大批量文本"],
          ["gemini-3.1-pro-preview", "$1 / $0.10 / $6", "最硬的推理和评审"],
        ] },
        { type: "p", text: "Gemini 3.1 Flash Image（Nano Banana 2）跑在同一路由、用同一密钥；其生成图像输出是单独计价的费率项，详见图像指南。每次请求的花费和已应用的折扣都会在调用后显示在控制台中。" },
        { type: "link", text: "完整 Gemini 费率表，含长上下文与图像费率项", href: "/docs/learn/gemini-api-pricing" },
        { type: "link", text: "所有支持的 model ID 与价格", href: "/models" },
      ] },
      { h2: "排查首个响应的问题", blocks: [
        { type: "table", headers: ["状态码", "可能原因", "解决方法"], rows: [
          ["401", "缺少或错误的 x-goog-api-key", "重新核对密钥值和确切的请求头名称"],
          ["404", "/v1beta 重复，或 model ID 不在目录中", "传裸 host；从 GET /v1beta/models 中挑选 ID"],
          ["402", "预付余额耗尽", "在控制台充值任意整数美元金额"],
        ] },
        { type: "p", text: "不要在原生 Gemini 路由上发送 Authorization: Bearer 或 Anthropic 的 x-api-key 请求头——x-goog-api-key 是它们接受的唯一凭据。由于线上协议格式不变，日后切回 Google 自己的端点只需改一行 base URL。" },
        { type: "link", text: "在 Pro、Flash 和 Flash-Lite 之间如何选择", href: "/docs/learn/gemini-pro-vs-flash-vs-flash-lite" },
        { type: "link", text: "用 Nano Banana 2 生成图像", href: "/docs/learn/nano-banana-2-api-guide" },
      ] },
    ],
    faq: [
      { q: "官方 Google GenAI SDK 能配合 apiToken.sale 使用吗？", a: "可以。在 Python 中设置 HttpOptions(base_url)、在 JavaScript 中设置 httpOptions.baseUrl 为 https://router.apitoken.sale，并传入 apiToken.sale 密钥；请求和响应结构保持原生。" },
      { q: "Gemini API 请求用哪个请求头鉴权？", a: "x-goog-api-key，携带你的 sk-pool 密钥。原生 Gemini 路由不接受 Authorization: Bearer 或 Anthropic 的 x-api-key 请求头。" },
      { q: "如何流式输出 Gemini 的内容？", a: "用 x-goog-api-key 调用 /v1beta/models/{model}:streamGenerateContent?alt=sse，或使用 SDK 的 generate_content_stream / generateContentStream 方法。最后一个 SSE 事件携带汇总的 usageMetadata。" },
      { q: "为什么重复的 /v1beta 会返回 404？", a: "Google SDK 会在配置的 host 上自动追加 API 版本。只配置裸 host，最终请求中就只会有一个 /v1beta 段。" },
      { q: "应该先调用哪个 Gemini 模型？", a: "通用文本和编程任务从 gemini-3.6-flash 开始。批量分类迁移到 Flash-Lite 模型，最硬的推理任务交给 gemini-3.1-pro-preview。" },
      { q: "调用 countTokens 免费吗？", a: "免费。在模型路径上调用 :countTokens 只返回 token 计数、不做生成，因此可以在付费生成之前估算输入规模。" },
    ],
  };
