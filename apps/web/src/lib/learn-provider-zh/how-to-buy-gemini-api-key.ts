import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "如何购买 Gemini API 密钥",
    h1: "如何购买 Gemini API 密钥",
    description: "无需 Google Cloud 计费账户即可购买 Gemini API 密钥：支持银行卡或加密货币预付费充值，提供原生 generateContent 端点，一个余额同时调用 Gemini、GPT、Claude 和 Kimi，官方费用一律五折。",
    keywords: ["购买 gemini api 密钥", "gemini api 密钥怎么买", "gemini api 密钥", "gemini api 购买", "gemini api 无需 google cloud", "gemini api 预付费余额", "加密货币购买 gemini api", "gemini api 银行卡付款", "便宜 gemini api", "gemini api 五折", "gemini api 密钥即时开通"],
    dek: "在 apiToken.sale 购买 Gemini API 密钥就像买预付费额度一样：注册账户，用银行卡或加密货币充值任意整数美元，然后在仪表板生成密钥。该密钥基于原生 Google Gemini 协议认证——x-goog-api-key、generateContent、官方 SDK——所有 token 消耗一律按官方价五折结算。全程不需要 Google Cloud 项目、计费账户或等待名单。",
    sections: [
      { h2: "从付款到可用密钥，只要五分钟", blocks: [
        { type: "p", text: "在这里购买 Gemini API 密钥是预付费消费，不是订阅：注册、充值、生成一把 sk-pool 密钥。密钥在下一次请求时即刻生效——整个流程中没有审批、等待名单或人工审核环节。" },
        { type: "steps", items: [
          "用 Google、GitHub 或邮箱加密码创建账户。通过 Google 和 GitHub 注册可获得 $5 平台赠金，可用于支持的 Gemini、GPT、Claude 和 Kimi 模型；邮箱/密码注册没有赠金。",
          "充值任意整数美元。收银台通过安全的支付服务商接受银行卡和加密货币，余额永不过期。",
          "打开仪表板生成 API 密钥。它形如 sk-pool-…，立即覆盖所有支持的提供商，而不仅是 Gemini。",
          "用本指南最后一节的 curl 验证密钥。返回 200 且有真实输出，说明密钥、余额和链路端到端全部打通。",
        ] },
      ] },
      { h2: "不需要 Google Cloud 项目或计费账户", blocks: [
        { type: "p", text: "直接从 Google 购买 Gemini 访问权限，意味着要有绑定了计费资料的 AI Studio 或 Google Cloud 账户，而对很多买家来说，恰恰卡在这一层。apiToken.sale 持有网关账户和上游计费，你只需要准备登录方式（Google、GitHub 或邮箱）和一种付款方式。" },
        { type: "p", text: "你拿到的不是改造过的代理 API。网关提供的是原生 Gemini 协议——与 Google 自己的端点相同的 URL 语法、请求体和响应结构——所以现有 Gemini 代码只需改两处配置即可继续运行：base URL 和密钥。" },
      ] },
      { h2: "一把密钥覆盖整个 Gemini 目录", blocks: [
        { type: "p", text: "一把密钥覆盖受支持的 Gemini 产品线；切换档位只需改请求路径中的模型 ID。每 1M token 的代表性文本价格：" },
        { type: "table", headers: ["模型 ID", "档位", "官方输入 / 输出", "五折之后"], rows: [
          ["gemini-3.6-flash", "Flash —— 日常默认", "$0.75 / $3.75 促销价", "$0.375 / $1.875"],
          ["gemini-3.1-pro-preview", "Pro —— 最难的推理任务", "$2 / $12", "$1 / $6"],
          ["gemini-3.1-flash-lite", "Flash-Lite —— 批量环节", "$0.25 / $1.50", "$0.125 / $0.75"],
          ["gemini-2.5-flash-lite", "最低文本价格底线", "$0.10 / $0.40", "$0.05 / $0.20"],
        ] },
        { type: "list", items: [
          "Gemini 3.1 Pro Preview 请求超过 200K 输入 token 时，整个请求按长上下文费率计费：每 1M 输入 $4、输出 $18。",
          "Gemini 3.1 Flash Image（Nano Banana 2）在同一路由上生成图像；图像输出官方价为每 1M 图像 token $60，折扣后 $30。",
          "Gemini 3.6 Flash 的 Google $0.75/$3.75 促销价持续到 2026-12-31；2027-01-01 恢复 $1.50/$7.50 标准价。",
          "文本模型的缓存输入按新鲜输入费率的 10% 计费，重度使用提示词缓存的负载最终成本更低。",
        ] },
        { type: "link", text: "Gemini 3.6 Flash 的价格、上下文和输出上限", href: "/models/gemini-3-6-flash" },
        { type: "link", text: "Gemini 完整价格拆解，含缓存和图像部分", href: "/docs/learn/gemini-api-pricing" },
      ] },
      { h2: "原生协议：官方 SDK、流式输出、免费 token 计数", blocks: [
        { type: "p", text: "由于线路格式就是原版 Gemini，官方 Google GenAI SDK 只需改 base URL 和密钥即可使用：" },
        sourceBlock("how-to-buy-gemini-api-key", 3, 1),
        { type: "list", items: [
          "流式输出在同一模型路径上使用 streamGenerateContent?alt=sse，返回增量分块。",
          "countTokens 走同一路径且免费——在花钱生成之前，先用它估算大提示词的用量。",
          "密钥放在环境变量（如 APITOKEN_API_KEY）里，绝不写进源代码。",
        ] },
        { type: "note", text: "SDK 的 base URL 只填裸域名。Google SDK 会自行附加 /v1beta；如果你的 base URL 已经以 /v1beta 结尾，重复的路径段会让每次调用都返回 404。" },
      ] },
      { h2: "预付费余额与五折折扣如何结算", blocks: [
        { type: "p", text: "没有月费，也没有席位授权费；余额只在请求运行时才被消耗。每次调用分三步结算：" },
        { type: "list", items: [
          "请求先按 Google 官方 token 费率计量，包括缓存输入和长上下文部分。",
          "减去你的生效折扣——B2C 账户的每次请求都按官方费用固定五折。",
          "净额从预付费余额中扣除，因此 $50 余额可覆盖 $100 官方费率用量。",
        ] },
        { type: "note", text: "余额归零后，请求会以余额不足的错误失败，直到你再次充值——没有透支，也不会从你的银行卡产生意外扣款。" },
      ] },
      { h2: "同一余额还能调用 GPT、Claude 和 Kimi", blocks: [
        { type: "p", text: "这把密钥并不限于 Gemini。一个预付费余额支撑全部四家受支持的提供商；每家提供商不同的只是端点、认证头和模型 ID：" },
        { type: "table", headers: ["提供商", "Base URL", "认证头"], rows: [
          ["Gemini", "https://router.apitoken.sale", "x-goog-api-key"],
          ["Claude 和 Kimi", "https://router.apitoken.sale/v1/messages", "x-api-key"],
          ["GPT", "https://router.apitoken.sale/v1", "Authorization: Bearer"],
        ] },
        { type: "p", text: "实际使用中，这意味着一个 Gemini 原型可以加挂 Claude 或 GPT 兜底，而无需第二个账户、第二张账单或第二套凭证要管理。" },
      ] },
      { h2: "用一个请求验证密钥", blocks: [
        { type: "p", text: "在把密钥接入项目之前，先发一个最小的 generateContent 调用。它只花不到一分钱，却能验证整条链路——密钥、余额、端点：" },
        sourceBlock("how-to-buy-gemini-api-key", 6, 1),
        { type: "list", items: [
          "401 —— 密钥缺失或输错，或者认证头不是 x-goog-api-key；x-api-key 和 Authorization: Bearer 属于其他提供商通道。",
          "404 —— 模型 ID 不在目录中，或者 SDK 的 base URL 配置错误导致 /v1beta 在 URL 中出现两次。",
          "402 / 余额不足 —— 余额已耗尽；充值任意整数美元即可。",
          "429 —— 触发限流；遵循 Retry-After 响应头并降低并发。",
        ] },
      ] },
    ],
    faq: [
      { q: "购买 Gemini API 密钥需要 Google Cloud 项目吗？", a: "不需要。网关账户和上游计费由 apiToken.sale 持有；你的客户端只需自定义 base URL，并通过 x-goog-api-key 发送 sk-pool 密钥。" },
      { q: "Gemini 请求用哪个认证头？", a: "x-goog-api-key。不要在原生 Gemini 路由上发送 Anthropic 的 x-api-key 或 OpenAI 风格的 Authorization: Bearer——每个提供商通道有自己的认证头。" },
      { q: "可以用加密货币购买 Gemini API 密钥吗？", a: "可以。收银台通过安全的支付服务商接受银行卡和加密货币，充值金额为任意整数美元，余额永不过期。" },
      { q: "试用密钥最便宜的方式是什么？", a: "用 Google 或 GitHub 注册的账户自带 $5 平台赠金，而 Gemini 2.5 Flash-Lite 折扣后每 1M token 输入 $0.05、输出 $0.20——足够做大量测试。" },
      { q: "同一把密钥能调用 GPT、Claude 和 Kimi 吗？", a: "可以。密钥和余额在所有受支持的提供商之间共享；你只需切换端点、认证头和模型 ID，账户始终不变。" },
      { q: "这里的 Gemini 协议和 Google 官方 API 一致吗？", a: "一致——generateContent、streamGenerateContent?alt=sse、countTokens 和官方 Google GenAI SDK 都原样可用。只有 base URL 和密钥不同。" },
    ],
  };
