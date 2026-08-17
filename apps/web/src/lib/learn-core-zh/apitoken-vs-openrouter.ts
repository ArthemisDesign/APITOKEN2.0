import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "apiToken.sale vs OpenRouter：Claude 网关怎么选",
    h1: "apiToken.sale vs OpenRouter：哪种 Claude 网关更适合你的技术栈",
    description: "面向 Claude 用户的 OpenRouter 替代方案实测对比：原生 Anthropic Messages API 加 50% 统一折扣，对比多提供方路由层。",
    keywords: ["openrouter 替代", "openrouter alternative", "apitoken vs openrouter", "claude api 网关", "openrouter claude", "openrouter vs anthropic api", "claude api 折扣", "便宜的 claude api", "原生 anthropic 端点", "claude api 中转", "best claude api"],
    dek: "因为 Claude 是你的主力模型而在找 OpenRouter 替代方案？OpenRouter 把数百个模型统一收敛到一套 OpenAI 兼容 API 后面；apiToken.sale 则直接给你原生 Anthropic Messages API，B2C 消费按官方价统一打五折。这篇对比覆盖协议保真度、计价方式和真实迁移成本。",
    sections: [
      { h2: "Claude 是主力模型时的简短结论", blocks: [
        { type: "p", text: "OpenRouter 是一个路由层：它把众多提供方的数百个模型收敛到一套 OpenAI 兼容 API 后面，并为每个请求挑选上游。apiToken.sale 是相反的专精路线——一家预付制转售商，直接暴露标准 Anthropic Messages API，B2C 账户按官方 Claude 消费统一打五折。如果 Claude 是你的主力模型，原生端点既更简单也更便宜：你的 Anthropic SDK、Claude Code 和 Cursor 配置不用加任何适配器就能继续用。如果你的负载真的要在长尾提供方之间来回切换，OpenRouter 的抽象才是你花钱买来的东西。" },
        { type: "p", text: "这两个服务真正重叠的地方只有注册页：两边都能让你在没有 Anthropic 账户、不等 waitlist、不受账单国家限制的情况下开始调 Claude。再往后，代码讲的协议、每个请求的计价方式、以及哪些 Anthropic 专有特性能活着走完全程，全都分道扬镳——而这些细节决定哪个网关该进生产环境。" },
      ] },
      { h2: "原生端点 vs 归一化层", blocks: [
        { type: "p", text: "架构差异比品牌对比更重要。apiToken.sale 在 https://router.apitoken.sale 上终止你的请求，讲的是原汁原味的 Anthropic Messages API——与 api.anthropic.com 相同的端点、请求结构、SSE 流式和响应格式，中间没有任何转换层。OpenRouter 则接受一种统一的 OpenAI 风格请求格式，再把它翻译成你所路由到的提供方和模型的格式，上游不可用时还可以自动 fallback。" },
        { type: "table", headers: ["维度", "apiToken.sale", "OpenRouter"], rows: [
          ["Claude 协议", "原生 Anthropic Messages API", "统一 OpenAI 兼容 schema，带提供方路由"],
          ["Claude 模型 ID", "裸 Anthropic ID：claude-opus-4-8、claude-sonnet-5、claude-haiku-4-5", "带提供方前缀的 slug"],
          ["其他提供方", "同一把密钥、同一份余额可用 GPT、Gemini 和 Kimi", "来自众多提供方的数百个模型"],
          ["Claude 计价", "官方消费统一减 50%（B2C）", "按模型的提供方费率"],
          ["计费方式", "预付余额，永不过期，支持银行卡或加密货币充值", "预付 credits"],
          ["需要 Anthropic 账户", "不需要", "不需要"],
        ] },
        { type: "p", text: "在 apiToken.sale 这边，一份余额、一把密钥就覆盖 Opus、Sonnet 和 Haiku，不用按提供方分别管资金。代价是广度：OpenRouter 的目录大得多，这也是选它的正当理由。" },
      ] },
      { h2: "提示词缓存、工具调用和流式：哪些能原样带过来", blocks: [
        { type: "p", text: "因为端点就是原生 Messages API，每一项 Anthropic 专有能力的表现都和直连 Anthropic 完全一致。这正是与归一化层的实际差别——后者的提供方专有字段必须先映射进一套通用 schema。" },
        { type: "list", items: [
          "stream: true 的 SSE 流式，包括增量 token delta。",
          "工具调用（function calling），使用标准 tool 和 tool_result 块。",
          "通过 cache_control 断点启用提示词缓存，按相同的缓存读/写费率计量。",
          "系统提示词、视觉输入，以及完整的 messages 请求结构。",
          "你的代码已经在用的同一批模型 ID——没有别名，没有前缀。",
        ] },
        { type: "note", text: "从 OpenRouter 迁移时的坑：那边的配置常带提供方前缀的模型 slug。换成裸 Anthropic ID——claude-sonnet-5，而不是路由别名——否则原生端点会按未知模型拒绝请求。" },
      ] },
      { h2: "一百万 token 的账", blocks: [
        { type: "p", text: "apiToken.sale 跑的不是更便宜的模型，也不是更慢的档位。每个请求按 Anthropic 官方 token 费率计量，然后减去 50% 的 B2C 统一折扣，净额从预付余额扣除。余额永不过期，用银行卡或加密货币充值，闲置的几周一分钱不花。B2B 用量定价单独谈。" },
        { type: "table", headers: ["模型", "官方输入 / 输出（每 1M 美元）", "apiToken.sale（−50%）"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "p", text: "在这组对比里，只有 apiToken.sale 直接给 Claude 消费打折——路由层只是透传提供方费率，因为路由才是它的产品，而不是余额。在 agentic 编程会话和重度缓存负载上，token 量最大，统一折扣带来的绝对节省也集中在这里。" },
        { type: "link", text: "完整的分模型定价，含缓存费率", href: "/models" },
        { type: "link", text: "用免费计算器估算你的月度开销", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "一口气把 OpenRouter 架构迁过来", blocks: [
        { type: "steps", items: [
          "在控制台创建免费账户并生成密钥——形如 sk-pool-…，同一把密钥可通用于支持的 Claude、GPT、Gemini 和 Kimi 模型。",
          "换端点。Anthropic 原生客户端指向 https://router.apitoken.sale，密钥放在 x-api-key 头里；已经在讲 OpenAI 请求格式的代码改用 https://router.apitoken.sale/v1，配 Authorization: Bearer。",
          "把任何带提供方前缀的模型 slug 换成裸 Anthropic ID，然后发一个真实请求，确认响应和 usage 计费都正常。",
        ] },
        { type: "code", code: `# Claude Code / Anthropic SDKs\nexport ANTHROPIC_BASE_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••` },
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"claude-sonnet-5","max_tokens":1024,"messages":[{"role":"user","content":"ping"}]}'` },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户自带 $5 平台奖励余额——适用于支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "OpenRouter 仍然是正确工具的地方", blocks: [
        { type: "p", text: "作为 Claude 场景下的 OpenRouter 替代方案，并不意味着什么都要换掉它。当你需要它的广度时，OpenRouter 确实很强：" },
        { type: "list", items: [
          "不用挨个注册提供方，就能在长尾模型之间做实验。",
          "某个上游挂掉时，自动 fallback 把请求改路由到另一个上游。",
          "为混合多种模型家族的技术栈提供单一 OpenAI 兼容接口。",
        ] },
        { type: "p", text: "如果你的生产流量以 Claude 为主、偶尔调 GPT、Gemini 或 Kimi，一把 apiToken.sale 密钥已经覆盖这个集合——而且 Claude 部分按官方半价回来。两者并用也是正经架构：重度 Claude 流量走折扣原生端点，长尾走路由器。" },
      ] },
      { h2: "一分钟决策清单", blocks: [
        { type: "list", items: [
          "Claude 是你的主力模型，想要原生 Messages API 加直接折扣——选 apiToken.sale。",
          "你要跨众多提供方路由，看重统一抽象甚于分模型定价——选 OpenRouter。",
          "你需要提示词缓存、工具调用和流式的表现与 Anthropic 文档完全一致——原生端点少一层翻译。",
          "你想要银行卡或加密货币充值、永不过期的余额，而不是按量烧 credits——选 apiToken.sale。",
          "两家都不需要 Anthropic 账户；真正的选择是折扣加保真度，还是路由广度。",
        ] },
      ] },
    ],
    faq: [
      { q: "apiToken.sale 是 Claude 场景下好的 OpenRouter 替代方案吗？", a: "如果 Claude 是你的主力模型，是的。你拿到的是原生 Anthropic Messages API，按官方消费统一打五折，而不是一套归一化的多提供方 schema。如果你需要一套 API 后面接几十个提供方，OpenRouter 更对口。" },
      { q: "从 OpenRouter 迁过来要重写代码吗？", a: "不用。Anthropic 原生工具只需要换 base URL 和密钥——https://router.apitoken.sale 配 x-api-key。已经在讲 OpenAI 请求格式的代码可以走 /v1 的 OpenAI 兼容通道，用 Authorization: Bearer。" },
      { q: "apiToken.sale 会像典型转售商那样加价吗？", a: "不会。请求按 Anthropic 官方 token 费率计量，扣除 50% B2C 统一折扣后才从你的预付余额里扣——是折扣，不是加价。" },
      { q: "走 apiToken.sale 提示词缓存还能用吗？", a: "能。端点讲的是原生 Messages API，所以 cache_control 断点、工具调用和 SSE 流式的表现与直连 Anthropic 完全一致，按相同的缓存费率计量。" },
      { q: "OpenRouter 和 apiToken.sale 可以同时用吗？", a: "可以。常见做法是把重度 Claude 流量路由到折扣原生端点，同时保留 OpenRouter 跑长尾模型和跨提供方 fallback。" },
    ],
  };
