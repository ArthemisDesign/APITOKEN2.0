import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "如何购买 Kimi API 密钥",
    h1: "如何购买 Kimi API 密钥",
    description: "购买一个预付费 API 密钥即可使用 Kimi K3 和 Kimi for Coding，支持银行卡或加密货币付款，可调用 Anthropic Messages 或 OpenAI 兼容端点，费用仅为官方 Kimi 价格的 50%。",
    keywords: ["购买 kimi api 密钥", "kimi api 密钥", "kimi k3 api", "kimi for coding api key", "moonshot kimi api", "kimi api 预付费", "kimi api 按量付费", "kimi api 无需 moonshot 账户", "kimi api anthropic 兼容", "便宜 kimi api"],
    dek: "在这里购买 Kimi API 密钥，意味着一个预付费密钥解锁整个 kimi/* 命名空间——K3 和 Kimi for Coding——价格仅为官方 token 费率的一半。注册后按整数美元用银行卡或加密货币充值，即可调用 Anthropic Messages 或 OpenAI 兼容通道。本文带你走完购买流程、首次付费请求，以及资金变动前值得了解的计费规则。",
    sections: [
      { h2: "购买 Kimi API 密钥实际得到什么", blocks: [
        { type: "p", text: "你购买的不是 Kimi 专属密钥，也不需要 Moonshot 账户。一个 apiToken.sale 密钥——形如 sk-pool-…——同时覆盖 Kimi 命名空间和受支持的 Claude、GPT、Gemini 模型，所有请求都按官方提供商价格的 50% 从同一个预付余额中结算。" },
        { type: "p", text: "购买本身只需几分钟：创建账户，在仪表板生成密钥，然后用银行卡或加密货币充值任意整数美元。没有单独的 Kimi 套餐，没有订阅制，也没有每月最低消费——余额是预付费的，永不过期，只按真实用量扣减。" },
      ] },
      { h2: "一次操作，从注册到可用密钥", blocks: [
        { type: "steps", items: [
          "创建 apiToken.sale 账户。用 Google 或 GitHub 注册可获得 $5 平台奖励金——邮箱/密码注册同样可用，但初始余额为零。",
          "打开仪表板并生成 API 密钥。密钥即时生效，没有审核环节或排队名单。",
          "用银行卡或加密货币充值任意整数美元。每笔充值相互独立，可以先充一小笔，之后再追加。",
          "用你的密钥读取 GET https://router.apitoken.sale/v1/models，从返回的目录中挑选一个 kimi/* ID。响应按密钥作用域过滤，只列出当前对你可路由且已定价的模型。",
        ] },
        { type: "note", text: "用 Google 或 GitHub 注册的新账户自带 $5 平台奖励金——可用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱/密码注册的账户不享受该奖励。" },
      ] },
      { h2: "用一次付费请求验证购买", blocks: [
        { type: "p", text: "Kimi 在路由器上原生支持 Anthropic Messages 协议，因此最省钱的验证方式是一次带较小 token 上限的非流式调用。一个往返即可同时验证认证、模型别名和计量。" },
        sourceBlock("how-to-buy-kimi-api-key", 2, 1),
        { type: "p", text: "200 响应会返回 content 块和一个 Anthropic 风格的 usage 对象，现有的 usage 解析逻辑可以直接复用。仪表板会展示同一份消耗并附 token 级明细，让你精确看到这次请求从余额里扣了多少。" },
        { type: "note", text: "402 响应表示余额耗尽，而不是密钥或模型别名有问题。充值后重试完全相同的请求即可——密钥仍然有效。" },
      ] },
      { h2: "该为哪个 Kimi 别名付费", blocks: [
        { type: "p", text: "路由器上的公开 Kimi ID 是订阅别名，不是官方开放平台的 ID。Kimi 分别公布缓存命中、缓存未命中和输出三档费率，apiToken.sale 对每一档都恰好收一半。以下数字均为每 1M token 的价格。" },
        { type: "table", headers: ["公开别名", "官方 命中 / 未命中 / 输出", "五折后实付"], rows: [
          ["kimi/k3 · k3-256k · k3[1m]", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
          ["kimi/kimi-for-coding", "$0.19 / $0.95 / $4", "$0.095 / $0.475 / $2"],
          ["kimi/kimi-for-coding-highspeed", "$0.38 / $1.90 / $8", "$0.19 / $0.95 / $4"],
        ] },
        { type: "list", items: [
          "K3 提供 256K 和 1M 两种上下文写法；任务不需要完整上下文窗口时选 k3-256k。",
          "Kimi for Coding 是低成本编程的默认选择；highspeed 别名恰好是基础 token 费率的两倍，只留给对延迟敏感的工作。",
          "推理（reasoning）token 按输出费率计费，属于输出的一部分——不是单独的收费项。",
          "切勿换成 kimi-k2.7-code 之类的官方 ID。路由器只接受 GET /v1/models 展示的别名，而该响应才是权威来源，因为可用性会随提供商容量和账户策略变化。",
        ] },
        { type: "link", text: "Kimi 费率全解析：缓存档位、别名与花费控制", href: "/docs/learn/kimi-api-pricing" },
      ] },
      { h2: "两种 wire 格式，同一个密钥和余额", blocks: [
        { type: "p", text: "Kimi 在路由器上是一个提供商命名空间，而不是第四种协议。Anthropic 原生工具——Anthropic SDK、Claude Code、Kimi Code——用 x-api-key 头调用 POST /v1/messages。OpenAI 兼容客户端则通过通用 /v1 通道以 Bearer token 访问相同的 kimi/* 别名。" },
        sourceBlock("how-to-buy-kimi-api-key", 4, 1),
        { type: "note", text: "Messages 路由接受 stream: true，但提供商边界处的 chunk 增量性仍在实测验证中。对 chunk 时序有严格要求时请用非流式调用，并在依赖流式行为前先在自己的集成测试中加以固定。" },
        { type: "link", text: "在 Claude Code 中运行 Kimi，并锁定每个模型档位", href: "/docs/learn/kimi-api-for-claude-code" },
      ] },
      { h2: "付款、退款与余额规则，先了解清楚", blocks: [
        { type: "list", items: [
          "充值接受任意整数美元，可用银行卡或加密货币支付——每笔充值可以换不同的付款方式。",
          "预付余额永不过期，只被所有受支持提供商的真实 API 用量消耗。",
          "包括 $5 注册奖励在内的免费额度总是先于付费余额扣减，因此早期测试不会动到你的充值。",
          "一笔充值只有在完全未使用的情况下才能在 5 个日历日内退款；一旦动用了其中任何部分，该笔充值即为最终消费。退款沿原支付渠道原路返回，奖励额度一律不可退。",
          "客服在 Telegram 或 apitokensale@gmail.com 以英文和俄文提供支持——咨询账单问题时请附上你的账户邮箱和订单号。",
        ] },
        { type: "p", text: "在预付费平台上，实用的策略是小额、多次充值。最终用不上的充值并不会损失——它会无限期留在余额里——但保持单笔金额较小，能在你早早改变主意时让退款窗口真正有意义。" },
      ] },
      { h2: "购买之后，把花费控制在边界内", blocks: [
        { type: "p", text: "给密钥设置一个终身花费上限，失控的循环就无法掏空余额；如果密钥是为固定项目签发的，再给它设一个过期日期。这两项控制都在仪表板中密钥旁边。" },
        { type: "p", text: "接下来最快的读物是快速上手（SDK 接线）和模型目录（实时价格）。你刚买的这个密钥同样可以调用受支持的 Claude、GPT 和 Gemini 模型，做跨模型对比不需要任何额外的配置成本。" },
        { type: "link", text: "Kimi API 快速上手：curl 与 Anthropic Python SDK 全流程", href: "/docs/learn/kimi-api-quickstart" },
        { type: "link", text: "对比所有受支持的模型及其实时价格", href: "/models" },
      ] },
    ],
    faq: [
      { q: "购买 Kimi API 密钥需要 Moonshot 账户吗？", a: "不需要。账户、密钥、余额和计费全部来自 apiToken.sale；你这边无需单独的 Kimi 套餐，也无需注册 Moonshot。" },
      { q: "这里的 Kimi API 多少钱？", a: "官方价格的一半。Kimi for Coding 每 1M 缓存命中 / 缓存未命中 / 输出 token 分别为 $0.095 / $0.475 / $2，K3 为 $0.15 / $1.50 / $7.50，highspeed 别名恰好是 Kimi for Coding 基础费率的两倍。" },
      { q: "Kimi 用哪个端点和请求头？", a: "Anthropic Messages 端点 https://router.apitoken.sale/v1/messages，使用 x-api-key；或通用 OpenAI 兼容 /v1 通道，使用 Authorization: Bearer。两者接受相同的 kimi/* 别名，并从同一余额扣费。" },
      { q: "可以用加密货币支付 Kimi API 密钥吗？", a: "可以。充值接受任意整数美元，可用银行卡或加密货币支付，且每笔充值都可以更换付款方式。" },
      { q: "付费前有免费测试 Kimi 的方式吗？", a: "有。用 Google 或 GitHub 注册的新账户自带 $5 平台奖励金，它先于任何付费余额扣减，用在 Kimi 上和用在其他受支持模型上一样。" },
      { q: "余额在任务中途归零会怎样？", a: "请求会返回 402，直到你充值。密钥仍然有效，余额永不过期，充值任意整数美元即可立即恢复服务。" },
    ],
  };
