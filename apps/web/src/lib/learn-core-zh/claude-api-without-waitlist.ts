import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API 免排队免审批",
    h1: "无需排队即可接入 Claude API",
    description: "跳过 Anthropic 的排队和审批。在 apiToken.sale 注册账户、生成 Claude API 密钥，几分钟内发出第一个调用。",
    keywords: ["claude api 免排队", "claude api 即时开通", "claude api 无需审批", "快速获取 claude api 密钥", "claude api 无需 anthropic 账户", "购买 claude api", "claude api 接入", "claude api 额度", "claude api 充值", "claude api 中转", "claude api 服务商"],
    dek: "找「免排队的 Claude API」，通常意味着你已经在别处被注册流程耽搁过了。apiToken.sale 直接把排队整个去掉：注册账户、生成密钥，同一个会话里第一次 Messages API 调用就能成功。没有审批环节，没有销售电话，没有企业资质验证。",
    sections: [
      { h2: "直连路径慢在哪里", blocks: [
        { type: "p", text: "你现在就能拿到一把可用的 Claude API 密钥，无需排队、无需审批：在 apiToken.sale 注册，在控制台生成密钥，它从下一个请求起就能正常响应。这把密钥对接的是同一个 Anthropic Messages API，模型 ID 也完全一致，现有代码和工具原封不动即可使用。这篇文章里唯一需要你等待的，是你自己的打字速度。" },
        { type: "p", text: "有必要先说清楚「排队」到底指什么。Anthropic 自家的 Console 原则上是自助开通的，但「原则上」背后藏着真实的摩擦：注册账户、手机号验证、受支持的支付方式、购买额度，每一道都挡在你和第一个 token 之间。速率限制按用量等级（usage tier）划分，随累计消费逐步提升，所以新账户不管你准备付多少钱，都从最紧的一档开始。如果你的卡或所在地区不受支持，流程直接停在结账这一步。" },
        { type: "p", text: "这正是自助网关要补上的缺口。你跳过的「审批」并不是模型上的功能开关，而是围绕模型的账户开通、支付接入和等级预热。" },
      ] },
      { h2: "自助密钥到底是什么", blocks: [
        { type: "p", text: "apiToken.sale 签发的是自己的密钥——形如 sk-pool-•••——从你掌控的预付余额中扣费。不需要创建 Anthropic 账户，不用等邀请，从注册到第一个成功请求之间没有任何人工审核。同一把密钥覆盖受支持的 Claude、GPT、Gemini 和 Kimi 模型，不用为每个服务商各管一套凭证。" },
        { type: "table", headers: ["客户端协议", "端点", "认证头"], rows: [
          ["Anthropic Messages（Claude、Kimi）", "https://router.apitoken.sale/v1/messages", "x-api-key"],
          ["OpenAI 兼容（GPT 与通用通道）", "https://router.apitoken.sale/v1", "Authorization: Bearer"],
          ["原生 Gemini", "https://router.apitoken.sale", "x-goog-api-key"],
        ] },
        { type: "p", text: "对 Claude 来说，你保留的是原汁原味的 Messages API 形态：claude-opus-4-8、claude-sonnet-5、claude-haiku-4-5 以及受支持产品线中的其余模型，流式输出和工具调用（tool use）都完整保留。改的只有 base URL 和密钥——请求体、SDK 版本和解析代码都不用动。" },
      ] },
      { h2: "先发出第一个调用，再谈付费", blocks: [
        { type: "steps", items: [
          "用 Google 或 GitHub 创建账户——$5 平台奖励金正是这一步给的，所以先别急着掏卡。",
          "打开控制台生成密钥（形如 sk-pool-•••）。密钥出现的那一刻就已生效，没有激活排队。",
          "从终端发出一个请求，看着它出现在你的用量计量里。",
        ] },
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-haiku-4-5",\n    "max_tokens": 256,\n    "messages": [{"role":"user","content":"Reply with one sentence."}]\n  }'` },
        { type: "p", text: "用 Google 或 GitHub 创建的新账户自带 $5 平台奖励金，足够你在充值前用真实的受支持模型验证整条链路——鉴权、模型路由、计量。邮箱密码账户功能完全一样，但没有这笔奖励，所以登录方式要有意识地选。" },
      ] },
      { h2: "把编辑器或 agent 指向路由", blocks: [
        { type: "p", text: "协议没有任何变化，所以每个兼容 Anthropic 的工具都只需要配两个字段：base URL 加密钥。Claude Code 直接从环境变量读取：" },
        { type: "code", code: `# Claude Code\nexport ANTHROPIC_BASE_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••\n\n# then just run\nclaude` },
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : https://router.apitoken.sale\nAPI key  : sk-pool-•••\nModel    : claude-sonnet-5` },
        { type: "p", text: "同样的两个字段也适用于 Cline、Continue、Zed 和 Aider，官方 Anthropic SDK 则对应 base_url 和 api_key 两个参数。只要一个工具能用 Anthropic API，它在你创建密钥的当天就能在这里跑起来——这就是「免排队」的实际含义。" },
      ] },
      { h2: "「免排队」不意味着什么", blocks: [
        { type: "list", items: [
          "不意味着免费。从第一个调用起就按余额计量扣费；$5 的 Google/GitHub 奖励是起始额度，不是一个等级。",
          "不意味着匿名。你仍然要创建账户，而且奖励取决于创建方式——用 Google 或 GitHub，而不是邮箱密码。",
          "不意味着加密货币即时到账。银行卡充值几秒到账；加密货币充值要等网络确认交易后入账。",
          "不意味着企业采购流程消失。B2C 接入完全自助，唯一还需要坐下来谈的，是可协商的 B2B 批量定价。",
        ] },
        { type: "p", text: "这些都不是排队。它们只是预付服务的常规规则，我们把话说明白，是为了让你遇到的第一个「意外」仅仅是：整个开通流程平淡无奇。" },
      ] },
      { h2: "奖励金用完之后怎么付费", blocks: [
        { type: "p", text: "充值支持任意整数美元金额，通过安全的收银台服务商用银行卡或加密货币支付。余额是预付制，永不过期，只在 API 请求实际运行时扣减——没有后台自动续费的订阅，也没有需要硬凑的每月最低消费。" },
        { type: "p", text: "每个请求先折算为 Anthropic 官方 API 花费，再给予折扣：B2C 账户的每个请求自动按官方花费的 50% 结算。各模型的当前费率列在模型页面上，跑一个工作负载之前就可以先把成本算清楚。" },
        { type: "link", text: "对比受支持的 Claude 模型及其当前费率", href: "/models" },
      ] },
    ],
    faq: [
      { q: "Claude API 真的没有排队吗？", a: "没错。接入完全自助、即时生效——你在控制台生成密钥，下一个请求它就能用，中间没有任何人工审核。" },
      { q: "拿密钥需要 Anthropic 账户吗？", a: "不需要。apiToken.sale 签发自己的密钥和预付余额，不涉及 Anthropic 账户、邀请或审批——但你调用的仍是同一个 Anthropic Messages API，模型 ID 也完全一致。" },
      { q: "需要和销售谈吗？", a: "不需要。B2C 接入完全自助。只有可协商的 B2B 批量定价才需要沟通。" },
      { q: "不付钱可以先测试 API 吗？", a: "可以。用 Google 或 GitHub 创建的账户自带 $5 平台奖励金，足够对受支持的模型发起真实调用。邮箱密码账户不享受此奖励。" },
      { q: "哪些 Claude 模型可以立即使用？", a: "整条受支持产品线——Claude Opus 4.8 和 4.7、Sonnet 5 和 4.6、Haiku 4.5——同一把密钥全覆盖，按请求计量。" },
      { q: "我现有的 Anthropic SDK 代码能直接用吗？", a: "能。把客户端指向 https://router.apitoken.sale 并使用你的 apiToken.sale 密钥即可；请求体、流式输出和工具调用都不变。" },
    ],
  };
