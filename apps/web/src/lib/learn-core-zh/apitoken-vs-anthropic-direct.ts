import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "apiToken.sale 对比 Anthropic 官方直购",
    h1: "apiToken.sale 对比直接向 Anthropic 购买",
    description: "对比 apiToken.sale 与 Anthropic 官方直购：完全相同的 Messages API 和模型，但统一立省 50%、无需 Anthropic 账户、支持银行卡或加密货币支付。",
    keywords: ["claude api 对比 anthropic 官方", "apitoken 对比 anthropic", "anthropic api 替代", "比 anthropic 官方更便宜的 claude api", "claude api 转售", "claude api 折扣", "无 anthropic 账户购买 claude api", "claude api vs anthropic", "便宜的 claude api", "最好用的 claude api"],
    dek: "apiToken.sale 并不是另一套 API——它就是同一套 Anthropic Messages API，从预付余额中以折扣价转售。本文只对比 apiToken 与 Anthropic 官方直购真正存在差异的方面：价格、上手门槛和支付方式。线路协议、模型 ID 和流式行为完全不变。",
    sections: [
      { h2: "它和 Anthropic 是同一套 API 吗？", blocks: [
        { type: "p", text: "是的。apiToken.sale 提供的就是同一套 Anthropic Messages API，模型 ID 也完全相同——claude-opus-4-8、claude-sonnet-5、claude-haiku-4-5——请求与响应格式和你代码里已经预期的一致。差异只在商业层面，不在技术层面：B2C 对官方消费统一打 5 折，不要求 Anthropic 账户或开票国家，支持银行卡或加密货币支付。" },
        { type: "p", text: "具体来说，你的客户端库所依赖的一切行为都与 Anthropic 文档描述的完全一致：向 /v1/messages 发 POST，请求体 JSON 相同，返回相同的内容块和 usage 对象，并以相同的事件序列通过 server-sent events 流式输出。工具调用、system 提示词和提示词缓存遵循同一套规则，因为请求最终落到同一个上游 API——不同的只是端点主机名和它前面的计费层。" },
        { type: "list", items: [
          "相同的 Messages API 端点与 SSE 流式输出。",
          "相同的模型 ID：claude-opus-4-8、claude-sonnet-5、claude-haiku-4-5。",
          "相同的请求/响应格式、请求头语义和错误结构。",
          "相同的提示词缓存机制：缓存写入与读取的计量方式一致。",
        ] },
      ] },
      { h2: "统一 5 折为什么是真的", blocks: [
        { type: "p", text: "这个折扣既不是更便宜的模型档位，也不是降级线路。每个请求先按精确的用量分项——输入 token、输出 token、缓存写入与读取——折算成 Anthropic 官方消费，然后才从中减去你的 B2C 统一 50% 折扣，净额从预付的汇集余额中扣减。预付加汇集就是全部机制：你提前为余额充值，这正是低于标价的价格能够持续的原因。" },
        { type: "table", headers: ["模型", "Anthropic 官方输入 / 输出（每 1M token 美元）", "apiToken.sale（−50%）"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "p", text: "每个请求都会在控制台中列出所用模型和 token 级别的明细，你可以自己对照 Anthropic 公布的价格核验这笔账。" },
        { type: "link", text: "用 Claude API 成本计算器估算你的月度开销", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "无需 Anthropic 账户、排队或开票国家校验", blocks: [
        { type: "p", text: "官方直购意味着要注册 Anthropic Console 账户、所在国家属于受支持的开票地区、并使用 Anthropic 接受的支付方式。对很多开发者来说这没问题；对其他所有人来说，这正是大家寻找替代方案的原因。apiToken.sale 把这道门槛彻底移除：不用创建 Anthropic 账户，没有排队名单，也没有开票国家要求。" },
        { type: "list", items: [
          "用银行卡或加密货币充值任意整数美元金额。",
          "余额永不过期，也没有订阅——闲置不花一分钱。",
          "一把密钥（形如 sk-pool-…）即可通用支持的 Claude、GPT、Gemini 和 Kimi 模型，全部从同一份余额扣费。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额，适用于支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "把现有的 Anthropic 集成指向 apiToken.sale", blocks: [
        { type: "p", text: "因为协议完全一致，迁移只是改一个 base URL，而不是重写代码。官方 SDK 和 Anthropic 兼容工具都把端点暴露为配置项。" },
        { type: "steps", items: [
          "免费注册 apiToken.sale 账户，在控制台生成 API 密钥——没有审核环节。",
          "把客户端指向 https://router.apitoken.sale：Python SDK 设置 base_url，TypeScript SDK 设置 baseURL，Claude Code 等读取环境变量的工具则设置 ANTHROPIC_BASE_URL。",
          "发一个请求，确认返回的是正常的 Anthropic Messages 响应且带 usage 对象；然后在控制台查看同一请求，核对折扣后的扣费。",
        ] },
        { type: "code", code: "export ANTHROPIC_BASE_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••\n\n# Claude Code and Anthropic-compatible tools now run on your prepaid balance\nclaude" },
        { type: "note", text: "只设置一个凭证变量。如果 ANTHROPIC_API_KEY 和 ANTHROPIC_AUTH_TOKEN 都被导出，客户端会同时发送两个请求头，请求会被拒绝——取消其中一个。模型 ID 保持不变；你的消息构造代码一行都不用动。" },
      ] },
      { h2: "什么时候 Anthropic 官方直购仍是正确选择", blocks: [
        { type: "p", text: "如果你的组织已经有顺畅的 Anthropic 开票流程、企业协议，或者有要求与供应商直接签约的采购规定，直购可能更适合你。如果你需要的是与 Anthropic 本身协商的合同条款，而不是自助的预付余额，同理。对其他所有人——独立开发者、小团队，以及受地域或支付渠道限制的人——直购并不能带来折扣路线已经提供的任何东西。" },
      ] },
      { h2: "给大多数 Claude API 买家的结论", blocks: [
        { type: "p", text: "技术层面两者打平：同样的 API、同样的模型、同样的流式行为。决策最终归结为价格和门槛。apiToken.sale 是同一个 Claude，B2C 对官方消费统一 5 折，支持银行卡或加密货币支付，余额永不过期，没有账户门槛。Anthropic 官方直购是同一个 Claude，按标价收费，且要过 Console 账户这一关。除非你的采购流程要求后者，这笔账本身已经说明了一切。" },
        { type: "link", text: "按模型查看缓存价格与上下文窗口的详情页", href: "/models" },
      ] },
    ],
    faq: [
      { q: "apiToken.sale 是真正的 Claude API 吗？", a: "是的——它提供同一套 Anthropic Messages API、相同的端点、流式输出和模型 ID（claude-opus-4-8、claude-sonnet-5、claude-haiku-4-5）。只有定价和开通方式不同。" },
      { q: "为什么 apiToken.sale 比直接向 Anthropic 购买更便宜？", a: "余额是预付且汇集的，并对每个请求的 Anthropic 官方消费统一套用 50% 的 B2C 折扣。模型和 API 完全相同，不同的只是计费层。" },
      { q: "使用 apiToken.sale 需要 Anthropic 账户吗？", a: "不需要。没有 Anthropic 账户、排队名单或开票国家要求——用银行卡或加密货币给余额充值，即可拿到一把密钥。" },
      { q: "我现有的 Anthropic SDK 代码能不改就用吗？", a: "可以。把 base URL 设为 https://router.apitoken.sale（Python 用 base_url，TypeScript 用 baseURL，读取环境变量的工具用 ANTHROPIC_BASE_URL），模型 ID 和消息代码保持不变。" },
      { q: "apiToken.sale 新账户有免费额度吗？", a: "通过 Google 或 GitHub 创建的账户自带 $5 平台奖励额度，可用于支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受该奖励。" },
    ],
  };
