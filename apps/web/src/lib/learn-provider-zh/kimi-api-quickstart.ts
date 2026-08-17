import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Kimi API 快速入门：K3 与 Kimi for Coding",
    h1: "Kimi API 快速入门：用 curl 或 Anthropic SDK 发起第一次调用",
    description: "Kimi API 快速入门：通过 apiToken.sale 用 curl 或官方 Anthropic SDK 调用 Kimi K3 与 Kimi for Coding——x-api-key 认证、kimi/* 模型 ID、一个预付余额、官方价格五折。",
    keywords: ["kimi api 快速入门", "kimi api 教程", "kimi anthropic api", "kimi k3 api 示例", "kimi for coding api", "kimi api curl", "kimi api python", "kimi api key", "moonshot kimi api", "kimi api openai 兼容"],
    dek: "这份 Kimi API 快速入门带你在五分钟左右从新账号走到第一个可用的 Kimi K3 响应：一个 base URL、一个 x-api-key 请求头、一个带命名空间的模型 ID。Kimi 在 apiToken.sale 路由器上说 Anthropic Messages 协议，官方 Anthropic SDK 只需改一个 base_url 即可使用。同一个预付密钥和余额也覆盖 Claude、GPT 和 Gemini。",
    sections: [
      { h2: "三步发出你的第一个 Kimi 请求", blocks: [
        { type: "p", text: "拿到第一个 Kimi 响应的最快路径，是向 https://router.apitoken.sale/v1/messages 发一个 POST：在 x-api-key 请求头里放上你的 apiToken.sale 密钥，在请求体里写上 kimi/* 模型 ID。不需要新 SDK，也不需要适配层——这个端点说 Anthropic Messages 协议，任何已经在和 Claude 对话的客户端都能直接和 Kimi 对话。用量与你的 Claude、GPT、Gemini 流量从同一个预付余额中结算。" },
        { type: "steps", items: [
          "注册免费账号并生成一个 API 密钥——形如 sk-pool-…，已覆盖支持的 Claude、GPT、Gemini 和 Kimi 模型。",
          "用银行卡或加密货币充值任意整数美元金额；Kimi 不需要单独套餐，也没有按提供商划分的余额。",
          "把密钥导出为 APITOKEN_API_KEY，然后发送下面的请求。返回带 content block 的 JSON 就说明路由已通。",
        ] },
        sourceBlock("kimi-api-quickstart", 0, 2),
        { type: "p", text: "响应是标准的 Anthropic Messages 对象，其终态 usage 块也遵循 Anthropic 结构——输入、输出 token 数加上缓存两项——所以现有的 usage 解析器和成本追踪器无需改动即可继续使用。" },
        { type: "note", text: "使用 Google 或 GitHub 注册的新账号自带 $5 平台赠金——可用于支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱/密码注册的账号不享受该赠金。" },
      ] },
      { h2: "官方 Anthropic SDK 只需多传一个参数", blocks: [
        { type: "p", text: "由于线上协议就是 Anthropic Messages，官方 Anthropic Python SDK 可以原样使用。两个构造函数参数完成切换：api_key 从环境变量读取密钥，base_url 把客户端指向路由器。密钥要放在服务端环境变量里——永远不要写进客户端代码或提交到仓库的文件。" },
        sourceBlock("kimi-api-quickstart", 1, 1),
        { type: "note", text: "base_url 要传路由器根地址，不要带路径：SDK 会自己追加 /v1/messages，如果 base_url 以 /v1 结尾，请求会变成 /v1/v1/messages 并返回 404。TypeScript SDK 同样以 baseURL 接收根地址，并会自动发送 x-api-key 和 anthropic-version 请求头。" },
        { type: "p", text: "系统提示词、多轮历史记录和工具调用的请求体，序列化方式与直连 api.anthropic.com 完全一致——只有模型 ID 和每 token 价格不同。" },
      ] },
      { h2: "模型 ID 是公布了费率的订阅别名", blocks: [
        { type: "p", text: "路由器通过带命名空间的订阅别名来寻址 Kimi，GET https://router.apitoken.sale/v1/models 是你的密钥对应的权威列表——可用性可能取决于提供商容量和账号策略，所以请读取目录，而不是从外部文档里硬编码一个名字。" },
        sourceBlock("kimi-api-quickstart", 2, 1),
        { type: "table", headers: ["公开别名", "上下文", "官方 命中 / 未命中 / 输出", "五折后实付"], rows: [
          ["kimi/kimi-for-coding", "256K", "$0.19 / $0.95 / $4", "$0.095 / $0.475 / $2"],
          ["kimi/kimi-for-coding-highspeed", "256K", "$0.38 / $1.90 / $8", "$0.19 / $0.95 / $4"],
          ["kimi/k3-256k", "256K", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
          ["kimi/k3 · kimi/k3[1m]", "1M", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
        ] },
        { type: "p", text: "以上均为每 100 万 token。Kimi 把输入拆成缓存命中和缓存未命中两档，因为它的缓存是自动的；highspeed 别名恰好是基础 Kimi for Coding token 费率的两倍，只在延迟真正重要的场景使用。" },
        { type: "note", text: "不要替换成 kimi-k2.7-code 这类 Open Platform 官方 ID。公开路由器接受 GET /v1/models 返回的订阅别名；官方资费名虽然看起来正确，但会返回模型错误。" },
        { type: "link", text: "各模型完整规格与折后价格", href: "/models" },
      ] },
      { h2: "同一批别名在 OpenAI 兼容通道同样可用", blocks: [
        { type: "p", text: "如果你的技术栈建立在 OpenAI SDK 上——或者所用框架写死了 Chat Completions 格式——完全不需要走 Anthropic 接口。路由器的统一 /v1 通道通过 Chat Completions 提供完全相同的 kimi/* 别名：" },
        sourceBlock("kimi-api-quickstart", 3, 1),
        { type: "note", text: "认证请求头要和通道匹配：OpenAI 兼容的 /v1 接口要求 Authorization: Bearer sk-pool-…，在这里发 x-api-key 会返回 401。Anthropic Messages 接口正好相反——必须 x-api-key，Bearer 会被拒绝。" },
      ] },
      { h2: "用量记账、流式与计费行为", blocks: [
        { type: "list", items: [
          "每次调用都返回 Anthropic 结构的终态 usage，按请求的 token 与成本追踪无需任何改动。",
          "Kimi 没有单独的缓存写入价格：新缓存的 token 按缓存未命中计费，重复上下文自动走更便宜的缓存命中档。",
          "推理 token 是输出的子集，按输出费率结算——不会作为独立 token 类别再计一次费。",
          "路由接受 stream: true，但上游与公开 chunk 的增量性仍在实网验证中。chunk 到达时序影响用户体验时，请使用非流式模式。",
          "402 响应表示预付余额需要充值；每次请求的已结算用量可在仪表板查看，密钥终身消费上限会限制单个密钥的总消耗。",
        ] },
        { type: "p", text: "每次调用先按上面列出的 Kimi 官方 token 费率计量，再从余额扣款前减去固定五折优惠——与同一密钥下 Claude、GPT 和 Gemini 用量的计费规则完全一致。" },
        { type: "link", text: "缓存分档、别名映射与消费控制见定价指南", href: "/docs/learn/kimi-api-pricing" },
      ] },
      { h2: "一个密钥接入所有编程代理", blocks: [
        { type: "p", text: "你刚验证过的端点和密钥，就是各编程代理配置所用的同一套凭据，从脚本切换到代理循环不需要任何新的购买或配置。每个代理有自己的配置约定，分别由专门指南覆盖：" },
        { type: "list", items: [
          "Claude Code 原生支持 Anthropic Messages——把它指向路由器，并把每个内部模型档位固定到一个 Kimi 别名。",
          "Kimi Code 在 config.toml 中接收 OpenAI 兼容的 provider 配置块，在同一入口下以 kimi/k3、openai/* 或 google/* 寻址模型。",
          "OpenCode 通过 apiToken.sale 插件消费路由器按密钥划分的实时目录，已下线的别名不会残留在本地配置里。",
        ] },
        { type: "link", text: "在 Claude Code 中运行 Kimi K3 与 Kimi for Coding", href: "/docs/learn/kimi-api-for-claude-code" },
        { type: "link", text: "把 Kimi Code 接入统一目录", href: "/docs/learn/kimi-api-for-kimi-code" },
        { type: "link", text: "在 OpenCode 中使用 Kimi API", href: "/docs/learn/kimi-api-for-opencode" },
      ] },
    ],
    faq: [
      { q: "可以用 Anthropic SDK 调用 Kimi 吗？", a: "可以。把它的 base_url 指向 https://router.apitoken.sale，并从按密钥划分的目录中选择 kimi/* 模型 ID——import、流式代码和错误处理都保持不变。" },
      { q: "应该从哪个模型 ID 开始？", a: "经济型编程默认选 kimi/kimi-for-coding；需要 K3 推理但不需要完整 1M 窗口时选 kimi/k3-256k。先用 GET /v1/models 确认你的密钥可用哪些模型。" },
      { q: "Kimi 路由可以设置 stream: true 吗？", a: "路由接受该参数，但上游与公开 chunk 的增量性仍在实网验证中。chunk 到达时序重要时请使用非流式模式。" },
      { q: "为什么路由器拒绝 Kimi 官方模型名？", a: "公开路由器接受 GET /v1/models 返回的订阅别名，不接受 kimi-k2.7-code 这类 Open Platform ID。官方资费名虽然看起来正确，但会返回模型错误。" },
      { q: "Kimi for Coding 每百万 token 多少钱？", a: "官方刊例价为每百万缓存命中 token $0.19、每百万缓存未命中 token $0.95、每百万输出 token $4；apiToken.sale 收取一半。highspeed 别名恰好是基础费率的两倍。" },
      { q: "Kimi 的用量和 Claude、GPT、Gemini 共用余额吗？", a: "是的。一个 sk-pool 密钥和一个预付余额覆盖全部四个提供商；用银行卡或加密货币充值任意整数美元，每个提供商都从同一个池中扣费。" },
    ],
  };
