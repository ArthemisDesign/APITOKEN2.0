import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "apiToken.sale vs ProxyAPI：Claude API 转售商对比",
    h1: "apiToken.sale vs ProxyAPI 对比",
    description: "对比 Claude API 转售商：apiToken.sale 提供原生 Anthropic 端点、官方消费统一 50% 折扣、银行卡或加密货币支付，一把密钥通用所有模型。",
    keywords: ["proxyapi 替代品", "apitoken vs proxyapi", "proxyapi claude", "不用 proxyapi 用 claude api", "claude api 转售商", "anthropic api 替代方案", "claude api 折扣", "便宜的 claude api", "没有 anthropic 账号用 claude api", "claude api 对比 anthropic", "claude api 哪个好"],
    dek: "找 ProxyAPI 替代品的开发者通常想要两件事：不用 Anthropic 账号就能用 Claude，以及更低的账单。apiToken.sale 两者都给——原生 Anthropic Messages API 端点，官方 Claude 消费统一 50% 折扣，支持银行卡或加密货币付款。这篇对比拆解两个服务真正不同的地方：协议保真度、定价机制和密钥管控。",
    sections: [
      { h2: "apiToken.sale 是真正的 ProxyAPI 替代品吗？", blocks: [
        { type: "p", text: "是——而且对重度使用 Claude 的负载来说，它是更直接的选择。两个服务都能让你在没有 Anthropic 账号的情况下用上 Claude，但 apiToken.sale 提供的是原生 Anthropic Messages API，按官方 token 费率统一打 5 折；而标准转售商按标价转售，甚至加价。如果你实际消耗 token 的模型就是 Claude，那么「原生协议 + 真实折扣」这个组合就是这场对比的全部答案。" },
        { type: "p", text: "两边的账号门槛完全一样：都不要 Anthropic 账号、不等 waitlist、也不要求特定国家的账单资料。你真正要比较的不是能不能访问，而是经济性和保真度——每个 token 花多少钱，以及端点是原生讲 Anthropic 协议，还是通过一层适配器做翻译。" },
      ] },
      { h2: "统一 50% 折扣对 Claude 账单意味着什么", blocks: [
        { type: "p", text: "每个请求都按 Anthropic 官方 token 费率计量，然后在从预付余额扣费之前统一应用 50% 的 B2C 折扣。没有订阅档位，也没有按席位收费——充值、消费，在控制台里逐行看用量。" },
        { type: "table", headers: ["模型", "官方输入 / 输出（每 1M token，美元）", "apiToken.sale（−50%）"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "list", items: [
          "支持银行卡或加密货币充值；余额永不过期，项目冲刺期和空闲月份都只花实际用掉的钱。",
          "一把预付密钥、一份余额同时覆盖 Opus、Sonnet 和 Haiku——同一把密钥还能用于支持的 GPT、Gemini 和 Kimi 模型，不用为每个提供方各养一个转售商。",
          "按请求逐笔审计消费，带模型和 token 明细，而不是月底对着一张发票反推。",
        ] },
        { type: "link", text: "完整分模型定价（含缓存费率）", href: "/models" },
        { type: "link", text: "用免费计算器估算你的每月 Claude 花费", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "原生 Anthropic Messages API 与「翻译型」端点的差别", blocks: [
        { type: "p", text: "大多数对比页面会跳过协议问题，但它决定了你的工具链迁移后能不能活下来。apiToken.sale 在 https://router.apitoken.sale 上提供标准的 Anthropic Messages API：同样的端点、同样的模型 ID（claude-opus-4-8、claude-sonnet-5、claude-haiku-4-5）、同样的请求和响应格式，正是你的代码现在所期望的。Claude Code、Cursor 和 Anthropic 官方 SDK 无需改动即可使用——你只改 base URL，不改应用。你和 Claude 之间没有适配层。" },
        { type: "p", text: "这很重要，因为 Messages API 承载的那些特性正是适配器容易丢的：stream:true 的 SSE 流式输出、通过 cache_control 断点实现的提示词缓存（缓存命中部分按更低的 cache-read 费率计费）、tool-use 块和 system 提示词。一个把你的请求重新序列化成别家提供方格式的代理，可能悄悄丢掉其中某一项——等你发现时，往往是缓存负载突然按无缓存的价格出账了。" },
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"claude-sonnet-5","max_tokens":256,"messages":[{"role":"user","content":"ping"}]}'` },
      ] },
      { h2: "改一次 base URL，从 ProxyAPI 切过来", blocks: [
        { type: "steps", items: [
          "注册免费账号，在控制台生成密钥（形如 sk-pool-•••）。一把密钥覆盖所有支持的 Claude 模型。",
          "把工具指向原生端点：Claude Code 设置 export ANTHROPIC_BASE_URL=https://router.apitoken.sale 和 ANTHROPIC_API_KEY，或者把同样的一对值填进 Cursor 的 Anthropic 提供方设置。",
          "先发一个真实请求，确认两件事：响应是正常的 Anthropic 格式，控制台里出现一条已应用 50% 折扣的计量记录。然后把旧转售商的凭据从 shell rc 文件和工具设置里删掉，避免任何东西静默回退。",
        ] },
        { type: "code", code: `# Claude Code — ~/.zshrc or ~/.bashrc\nexport ANTHROPIC_BASE_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••\n\n# Cursor → Settings → Models → Anthropic API\n# Base URL : https://router.apitoken.sale\n# API key  : sk-pool-•••` },
        { type: "note", text: "切换失败几乎总逃不出两个原因：旧 shell 还 export 着旧转售商的变量（启动 claude 之前先开一个新终端），以及在 Cursor 里改的是 OpenAI 提供方而不是 Anthropic 提供方。改完出现 401，说明密钥或 base URL 不对——按这个顺序复查两者。" },
      ] },
      { h2: "密钥护栏：终身消费上限与到期时间", blocks: [
        { type: "p", text: "转售商的密钥往往是「全有或全无」：谁拿到密钥，谁就能花余额。apiToken.sale 的密钥带两条可以在控制台显式设置的护栏——封顶累计总消费的终身消费上限，以及可选的到期日期（到期后密钥停止认证）。对于放在 CI 任务或共用工作站上的密钥，这就是「密钥泄露」和「预算泄露」之间的区别。" },
        { type: "list", items: [
          "终身消费上限：累计用量达到你设的上限后，密钥停止扣费。",
          "可选到期日期：密钥按时间表自动失效，而不是等谁想起来去吊销。",
          "按请求计量：每次调用的模型、token 数和折后费用，控制台里都能看到。",
        ] },
      ] },
      { h2: "什么时候留在多提供方转售商更合理", blocks: [
        { type: "p", text: "对自己的负载要诚实。如果 Claude 只是偶尔用用的配角模型，而你已经在现在的转售商那里接着好几个其他提供方，那留下来也说得过去——毕竟是一套你已经调通了的集成。迁移的理由在 Claude 是日常主力时最强：Claude Code 会话、agent 循环、批处理任务——任何 50% 折扣和协议保真度会在数百万 token 上复利放大的场景。" },
        { type: "p", text: "「多提供方」这个论点其实双向成立：同一把 apiToken.sale 密钥和余额同样支持 GPT、Gemini 和 Kimi 模型，把 Claude 迁过来不会让技术栈的其他部分搁浅。迁移中途出了问题，支持团队在 Telegram 或 apitokensale@gmail.com 用英语和俄语响应；万一需要退款，也通过同一支持渠道走原支付方退回。" },
      ] },
    ],
    faq: [
      { q: "用 Claude 的话，apiToken.sale 比 ProxyAPI 便宜吗？", a: "apiToken.sale 按 Anthropic 官方 token 费率计量，再统一应用 50% 的 B2C 折扣——Claude Sonnet 5 折算下来是每 1M 输入/输出 token $1.50 / $7.50，而不是官方的 $3 / $15。标准转售商按标价卖，甚至在标价之上加价，在重度 Claude 负载下差距会迅速累积。" },
      { q: "从 ProxyAPI 切换后，Claude Code 和 Cursor 还能用吗？", a: "能。apiToken.sale 提供原生 Anthropic Messages API，Claude Code、Cursor 和官方 SDK 只需把 base URL 改成 https://router.apitoken.sale 并配上密钥——流式、tool use 和提示词缓存的行为与 api.anthropic.com 完全一致。" },
      { q: "需要 Anthropic 账号或特定的账单国家吗？", a: "不需要——和 ProxyAPI 一样，apiToken.sale 完全移除了 Anthropic 账号门槛。注册，用银行卡或加密货币充值，预付余额永不过期。" },
      { q: "迁移工作负载之前，有免费试用 apiToken.sale 的方式吗？", a: "通过 Google 或 GitHub 创建的新账号自带 $5 平台奖励金，可用于支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账号没有该奖励。足够跑一次真实的 Claude Code 会话，自己对比计量成本。" },
      { q: "把 Claude 流量迁走后，还能继续用 GPT 或 Gemini 吗？", a: "可以——同一把预付密钥和余额同时覆盖支持的 GPT、Gemini 和 Kimi 模型，以及 Claude Opus、Sonnet 和 Haiku，统一到一把密钥不会让其他提供方搁浅。" },
    ],
  };
