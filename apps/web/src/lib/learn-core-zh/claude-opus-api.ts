import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude Opus API 接入",
    h1: "通过 API 使用 Claude Opus 4.8",
    description: "用一把 apitoken.sale 密钥接入 Claude Opus 4.8 和 4.7，统一按官方费率 5 折计费——每 100 万 token 仅 $2.50/$12.50。预付费、无需 Anthropic 账号，走标准 Messages API。",
    keywords: ["claude opus api", "claude opus 4.8 api", "claude opus api 密钥", "claude opus api 价格", "claude opus api 费用", "claude opus 折扣", "无 anthropic 账号用 opus api", "claude opus 提示词缓存", "claude api 免费额度", "免费试用 claude api"],
    dek: "Claude Opus API 是 Anthropic 的顶级档位，适合高难度推理、多文件重构和长时间运行的智能体会话。在 apitoken.sale 上，Opus 4.8 和 Opus 4.7 与其他所有支持的模型共用同一把预付费密钥和余额，按官方费率计量后再打 5 折。本文讲清楚真实价格、一个可直接运行的请求，以及如何让长时间 Opus 任务更省钱。",
    sections: [
      { h2: "一把密钥用遍两个现行 Opus 模型", blocks: [
        { type: "p", text: "Claude Opus API 走的是标准的 Anthropic Messages API：把 base URL 设为 https://router.apitoken.sale，用 x-api-key 请求头认证，model 传 claude-opus-4-8 即可。apitoken.sale 在同一把预付费密钥上提供 Opus 4.8 和 Opus 4.7，官方 token 费率统一 5 折——不需要 Anthropic 账号，没有账单地区限制，也没有 waitlist。" },
        { type: "p", text: "Opus 适合租来做那些“答错比 token 更贵”的活：架构决策、复杂重构、长时间自主运行的智能体。日常任务交给更便宜的 Claude 更划算，本文第四节专门讲这种分工。" },
      ] },
      { h2: "Opus 在这里每 token 多少钱", blocks: [
        { type: "p", text: "每个请求都按 Anthropic 官方费率卡精确计量各项用量——输入、输出和缓存——统一的 50% B2C 折扣在从预付余额扣款前先行减去。不向上取整，也不打包捆绑。" },
        { type: "table", headers: ["模型", "官方 输入 / 输出（$ / 100 万）", "本站（−50%）"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
        ] },
        { type: "p", text: "提示词缓存（prompt caching）在同一张费率卡上作为独立计费项计量，折扣同样适用：" },
        { type: "table", headers: ["缓存计费项（Opus）", "官方（$ / 100 万）", "本站（−50%）"], rows: [
          ["缓存写入（5 分钟 TTL）", "$6.25", "$3.125"],
          ["缓存读取", "$0.50", "$0.25"],
        ] },
        { type: "note", text: "Opus 4.8 在整个 100 万 token 上下文窗口内保持标准价格——没有长上下文溢价——单次响应最多返回 128K 输出 token。推荐使用自适应思考（adaptive thinking）模式，思考 token 按输出计费。" },
        { type: "link", text: "Claude Opus 4.8 价格详解（缓存费率、上下文、FAQ）", href: "/models/claude-opus-4-8" },
      ] },
      { h2: "两分钟内发出你的第一个 Opus 请求", blocks: [
        { type: "p", text: "线上协议就是 Anthropic 的原生格式。如果你的代码已经在调 api.anthropic.com，只需要改两处：base URL 和密钥。" },
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-opus-4-8",\n    "max_tokens": 1024,\n    "messages": [{"role": "user", "content": "Review this diff for regressions"}]\n  }'` },
        { type: "p", text: "Claude Code 等 Anthropic 原生工具直接从环境变量读取同样的配置：" },
        { type: "code", code: `export ANTHROPIC_BASE_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••` },
        { type: "p", text: "只会说 OpenAI chat completions 的工具可以改用 OpenAI 兼容通道 https://router.apitoken.sale/v1——Authorization: Bearer sk-pool-•••，模型 ID 不变。无论走哪条路，请求都从同一个预付余额按同一个折扣价扣费。" },
      ] },
      { h2: "什么时候 Opus 值得用，什么时候不值得", blocks: [
        { type: "list", items: [
          "复杂重构和多文件改动——一处错误会顺着代码库连锁扩散。",
          "架构、规划和高风险的评审工作。",
          "看重一致性和提示词缓存复用的长时间智能体会话。",
          "作为编排者或顾问的一遍处理，审查并引导更便宜模型产生的输出。",
        ] },
        { type: "p", text: "日常编码用 Sonnet 5 就够了：质量接近 Opus，token 价格只有 40%；Haiku 4.5 则以 Opus 输入价五分之一的成本覆盖高并发、对延迟敏感的任务。因为一把密钥、一个余额覆盖所有档位，路由是按请求决定的——你改的是模型 ID，而不是换服务商。" },
      ] },
      { h2: "让长时间 Opus 会话跑得起", blocks: [
        { type: "list", items: [
          "缓存稳定的前缀。系统提示词、工具定义和仓库上下文都应放在缓存断点之后：Opus 上缓存读取的官方价是每 100 万 token $0.50，而新鲜输入要 $5，你的折扣会再减一半。",
          "只缓存会重复的内容。缓存写入比普通输入贵（官方 $6.25 对 $5 / 100 万），所以同一前缀至少要发送两次，断点才划算。",
          "max_tokens 按任务实际需要设置上限；长对话先总结再续，别把完整历史反复重发。",
          "把子任务下放一档：搜索、提取和草稿交给 Haiku 或 Sonnet，Opus 只留给真正难的步骤。",
        ] },
        { type: "p", text: "这些手段与折扣是叠加的：缓存和路由降低 token 数量，5 折费率降低每个 token 的单价，每一笔计费项都能在控制台的用量明细里看到。" },
        { type: "link", text: "运行前先估算 Opus 工作负载的成本", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "先免费用，再预付费", blocks: [
        { type: "steps", items: [
          "用 Google 或 GitHub 注册账号——通过这两个渠道注册的新账号自带 $5 平台奖励金，足够跑真正的 Opus 调用。邮箱/密码注册的账号没有这笔奖励。",
          "打开控制台生成密钥（形如 sk-pool-…）。即刻生效，通用于支持的 Claude、GPT、Gemini 和 Kimi 模型。",
          "把你的工具指向 https://router.apitoken.sale 并配上这把密钥，用奖励余额发一个真实的 Opus 请求。",
          "奖励快用完时，通过安全的收银渠道用银行卡或加密货币（USDT、BTC 及其他主流币种）充值任意整数美元金额。余额永不过期，也没有订阅。",
        ] },
        { type: "note", text: "用 Google 或 GitHub 创建的新账号自带 $5 平台奖励金——适用于支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱/密码注册的账号不享受该奖励。" },
        { type: "p", text: "配置过程中遇到任何问题，支持团队可通过 Telegram 以英语和俄语解答，也可发邮件到 apitokensale@gmail.com。" },
      ] },
    ],
    faq: [
      { q: "没有 Anthropic 账号怎么拿到 Claude Opus API 密钥？", a: "在 apiToken.sale 用 Google 或 GitHub 注册，然后在控制台生成密钥——即刻生效，没有 waitlist，也不查账单地区。新的 Google/GitHub 账号自带 $5 平台奖励金。" },
      { q: "API 里 Opus 的模型 ID 填什么？", a: "当前一代填 claude-opus-4-8，上一代填 claude-opus-4-7。两者共用同一把密钥和预付余额，切换只需要改请求里的一个字段。" },
      { q: "Claude Opus API 每 token 多少钱？", a: "官方价是每 100 万输入 token $5、每 100 万输出 token $25。在 apiToken.sale，每次调用都统一打 5 折，同一请求只需 $2.50/$12.50；缓存计费项单独按各自的折扣价计量。" },
      { q: "写代码用 Opus 比 Sonnet 值得吗？", a: "做高难度推理、复杂重构和长时间智能体运行，值得。日常编码 Sonnet 5 就能给出接近 Opus 的质量，token 价格只有 40%——很多团队就在一把密钥上按任务路由。" },
      { q: "我只是偶尔用 Opus，预付余额会过期吗？", a: "不会。余额永不过期，没有订阅也没有月度最低消费，闲置不花一分钱，偶尔跑一次 Opus 只是慢慢消耗余额而已。" },
    ],
  };
