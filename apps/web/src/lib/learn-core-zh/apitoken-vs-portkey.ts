import type { LocalizedContent } from "../learn";
import { BASE, KEY } from "../learn-shared";

export const content: LocalizedContent = {
    title: "apiToken.sale 与 Portkey 对比（Claude 场景）",
    h1: "apiToken.sale 与 Portkey 对比：密钥供应商 vs AI 网关",
    description: "Portkey 是一个基于你自有厂商密钥做路由和可观测的 AI 网关。apiToken.sale 则直接提供折扣的 Claude 密钥和余额。本文讲清两者各自的适用场景——以及如何组合使用。",
    keywords: ["portkey 替代品", "portkey 替代方案", "apitoken 对比 portkey", "portkey claude api", "ai 网关 claude", "claude api 网关", "byok ai 网关", "portkey anthropic provider", "anthropic api 替代", "claude api 折扣", "便宜的 claude api"],
    dek: "搜索 Portkey 替代品的人，通常想要两样东西之一：更便宜的 Claude token，或者不带 Anthropic 账单的网关功能。Portkey 解决的是后者，apiToken.sale 解决的是前者。本文帮你判断自己需要哪一个——以及如何两者一起用。",
    sections: [
      { h2: "Portkey 管理的是你已有的密钥——它不卖密钥", blocks: [
        { type: "p", text: "apiToken.sale 和 Portkey 不是同一类产品的两种口味。Portkey 是一个 AI 网关：它架在你已有的厂商 API 密钥前面，在上面叠加路由、缓存和可观测能力。apiToken.sale 则是 Claude 密钥和余额的来源本身——一个原生 Anthropic 端点，统一 50% 折扣，且无需 Anthropic 账户。" },
        { type: "p", text: "这个区别决定了其他一切。只用 Portkey，你仍然要自带一个已充值的 Anthropic 账户——意味着要通过 Anthropic 自己的注册、账单国家和支付校验，并按官方全价支付 token 费用。网关改变的是请求的传输方式，永远不会改变背后厂商每 token 的收费。" },
      ] },
      { h2: "AI 网关的价值体现在哪里", blocks: [
        { type: "p", text: "如果你运营多个厂商账户，或者承载生产流量，网关这一层确实有用。它的功能集是围绕控制，而不是价格：" },
        { type: "list", items: [
          "当某个厂商报错或限流时，在多个目标之间做故障转移和负载均衡。",
          "自动重试，以及对重复提示词的响应缓存。",
          "覆盖你接入的所有厂商的请求日志、追踪和用量分析。",
          "护栏（Guardrails）和虚拟密钥，让团队成员和服务拿到权限受限的凭证，而不是你的原始厂商密钥。",
        ] },
        { type: "p", text: "Portkey 的网关是开源的，你可以把它自托管在自己的应用旁边，也可以用托管云服务省掉运维。无论哪种方式，模型 token 本身的账单都来自网关背后的那个厂商账户——而这笔账单恰恰是网关无法改善的。" },
      ] },
      { h2: "折扣的 Claude 密钥到底来自哪里", blocks: [
        { type: "p", text: `apiToken.sale 是这套组合的供应端。你充值一个预付余额——任意整数美元金额，支持银行卡或加密货币——然后用在 ${BASE} 的标准 Anthropic Messages API 上调用，密钥形如 ${KEY}。每个请求按 Anthropic 官方费率计量，然后在扣减余额之前统一减去 50% 的 B2C 折扣。余额永不过期，全程不需要 Anthropic 账户。` },
        { type: "table", headers: ["模型", "官方输入 / 输出（$ / 1M token）", "本站价格（−50%）"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "p", text: "由于端点讲的是原生 Messages API，Claude Code、Cursor 和官方 Anthropic SDK 只需改两行：Base URL 和密钥。同一把密钥还覆盖各自协议下受支持的 GPT、Gemini 和 Kimi 模型，同一份余额跟着你跨厂商使用。" },
        { type: "link", text: "按模型的完整定价（含缓存费率）", href: "/models" },
        { type: "link", text: "用免费计算器估算你的月度开销", href: "/tools/claude-api-cost-calculator" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额，可用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "让 Portkey 指向 apiToken.sale 密钥", blocks: [
        { type: "p", text: "两个产品可以干净地组合：Portkey 继续做路由和可观测，折扣的 apiToken.sale 密钥作为底层的 Anthropic 厂商。你保留网关的仪表盘和故障转移，而它记录的花费已经便宜了 50%。" },
        { type: "steps", items: [
          `创建 apiToken.sale 账户并在控制台生成密钥——形如 ${KEY}，对所有受支持的 Claude 模型有效。`,
          `在 Portkey 中添加一个 Anthropic 目标，用指向 ${BASE} 的自定义 host 覆盖其 Base URL，然后把 sk-pool 密钥粘贴为凭证。`,
          "照常让应用流量经过 Portkey。请求以标准 Anthropic Messages 格式到达 apiToken.sale 端点，因此模型 ID、流式输出和提示词缓存的行为与直连 Anthropic 完全一致。",
        ] },
        { type: "code", code: `// Portkey gateway config: Anthropic provider, discounted endpoint underneath
{
  "targets": [
    {
      "provider": "anthropic",
      "api_key": "${KEY}",
      "custom_host": "${BASE}",
      "override_params": { "model": "claude-sonnet-5" }
    }
  ]
}` },
        { type: "note", text: "目标里保留真实的 Anthropic 模型 ID（claude-opus-4-8、claude-sonnet-5、claude-haiku-4-5）——自定义 host 背后的端点是原生 Messages API，不是经过转换的形态。覆盖字段在网关配置里位于目标上，在托管控制台里位于厂商凭证上；思路相同：Portkey 把 Anthropic 格式的调用转发到 apiToken.sale 的 Base URL。" },
      ] },
      { h2: "同样的流量，两张不同的账单", blocks: [
        { type: "p", text: "以一个月实际的 agentic 编程（用 claude-sonnet-5）为例：10M 输入 token、2M 输出 token。按官方每百万 $3 / $15 的费率计算，就是 $30 + $30 = $60。把这部分流量用你自己的 Anthropic 密钥经过网关路由，$60 一分不少——你得到的是更好的日志，而不是更小的账单。同样的流量走 apiToken.sale 密钥只需 $30，因为折扣发生在密钥供应方，也就是计量发生的地方。" },
        { type: "list", items: [
          "只用网关：厂商账单全额照付，外加路由和可观测能力。",
          "只用折扣密钥：账单减半，直接调用，没有额外一跳。",
          "网关架在折扣密钥前面：账单减半，同时保留网关的控制能力。",
        ] },
      ] },
      { h2: "你真正需要的是哪一层？", blocks: [
        { type: "list", items: [
          "你只想要更便宜、工具能正常工作的 Claude——只用 apiToken.sale。改掉 Base URL 和密钥，搞定。",
          "你已经在给多个厂商账户充值，需要跨它们的故障转移、追踪和护栏——只用 Portkey，接受官方 token 价格。",
          "你既要生产级控制，又要更低的 Claude 账单——按上文把 Portkey 架在 apiToken.sale 密钥前面。",
        ] },
        { type: "p", text: "大多数在两者之间做比较的个人开发者和小团队都属于第一组：痛点在 Anthropic 账户和 token 价格，而不是缺一层路由。先用折扣密钥，等真正出现多厂商运维需求时再上加网关。" },
      ] },
    ],
    faq: [
      { q: "Portkey 能给我 Claude API 折扣吗？", a: "不能。Portkey 是架在你已有密钥之上的网关，你仍按厂商官方费率付费。折扣的 Claude 密钥和余额来自 apiToken.sale，它按官方费率计量后统一减去 50% 的 B2C 折扣。" },
      { q: "Portkey 和 apiToken.sale 能一起用吗？", a: `能。在 Portkey 里添加一个 Anthropic 目标，用 ${BASE} 作为自定义 host 覆盖其 Base URL，再粘贴你的 sk-pool 密钥——既保留 Portkey 的可观测能力，底层花费又享受折扣。` },
      { q: "用 Portkey 还需要 Anthropic 账户吗？", a: "只用 Portkey 的话，需要——它通过你自带的厂商密钥路由请求，背后得有一个已充值的 Anthropic 账户。用 apiToken.sale 密钥则完全不需要 Anthropic 账户。" },
      { q: "Portkey 是 Claude API 供应商吗？", a: "不是。它从不出售模型访问权限或 token 余额；它只是你的应用和你直接付费的厂商之间的控制层。apiToken.sale 正好相反：它提供密钥和预付余额，自身不加任何路由层。" },
      { q: "换密钥供应商后，我的 Anthropic SDK 代码还能用吗？", a: `能。apiToken.sale 在 ${BASE} 提供原生 Anthropic Messages API，官方 SDK、Claude Code 和 Cursor 都能继续工作——你只需改 Base URL 和 API 密钥。` },
    ],
  };
