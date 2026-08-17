import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "在俄罗斯及受限地区使用 Claude API",
    h1: "在俄罗斯使用 Claude API",
    description: "通过 apiToken.sale 从俄罗斯及其他受限地区访问 Claude API——无需 Anthropic 账户，银行卡或加密货币付款，一把密钥通用所有 Claude 模型。",
    keywords: ["claude api 俄罗斯", "俄罗斯使用 claude api", "claude api 受限地区", "claude api 充值", "claude api 加密货币支付", "claude api 没有外国银行卡", "anthropic 不支持的国家", "claude api russia", "claude api without foreign card", "claude api 免 vpn"],
    dek: "每一个在俄罗斯搜索 Claude API 接入方法的人，最终都会撞上同一堵墙：Anthropic 要求受支持的账单国家和与之匹配的支付方式，所以注册在收银台就卡住了，你连密钥的影子都看不到。apiToken.sale 绕开这堵墙——你用银行卡或加密货币充值预付余额，然后用自己的密钥调用同一个 Anthropic Messages API。无需 Anthropic 账户，无需排队，无需企业核验。",
    sections: [
      { h2: "为什么用俄罗斯银行卡注册 Anthropic 会卡住", blocks: [
        { type: "p", text: "Anthropic 把 API 密钥锁在计费后面：开户要求受支持的账单国家和该国发行的支付方式，从俄罗斯出发，这项检查在密钥生成之前就会失败。模型本身只是普通的 HTTPS 端点——锁在付款和开户环节，不在 API 协议上。apiToken.sale 拆掉的正是这把锁：密钥和余额由平台自己签发，账单国家的问题根本不会出现。" },
        { type: "p", text: "这也解释了为什么常见的绕道方案都不靠谱。VPN 只能改变流量看起来的来源地，却变不出一张受支持账单国家的卡；借来的外国卡往往在第一次重新核验时就失效。持久的解决办法是彻底不依赖 Anthropic 的收银台。" },
      ] },
      { h2: "apiToken.sale 改变了什么，没有改变什么", blocks: [
        { type: "p", text: "这项服务扮演了 Anthropic 不愿在你的地区扮演的计费层角色。你注册一个免费账户、充值余额、生成一把形如 sk-pool-… 的密钥——不需要 Anthropic 账户，不需要受支持的账单国家，不排队，不做企业核验。激活是即时的：下一个请求密钥就能用。" },
        { type: "list", items: [
          "任何一步都不要求 Anthropic 账户或账单国家。",
          "结账时可选银行卡或加密货币，每次充值自由决定。",
          "密钥即时激活，没有人工审核。",
          "通过 Telegram 提供俄语和英语支持。",
        ] },
        { type: "p", text: "有一个诚实的限制。购买余额和生成密钥都没有地域锁定，但 API 端点的网络可达性取决于你自己的连接。如果你的网络能访问路由端点，整条链路就能跑通；如果连不上，那是你这边的路由问题，不是计费问题。" },
      ] },
      { h2: "预付余额，替代海外订阅", blocks: [
        { type: "p", text: "充值金额是任意整数美元——没有固定套餐要选，也没有月费。余额永不过期，只在请求实际运行时扣费：每个请求先折算成 Anthropic 官方 API 费用，再套用你的折扣。B2C 账户的每个请求都按官方费用固定优惠 50%，所以你充一次值，边开发边花。银行卡和加密货币充进同一个余额，每次充值可以自由切换——一种方式被拒付而另一种能用时，这很实用。" },
        { type: "table", headers: ["充值方式", "余额到账时间", "什么时候选它"], rows: [
          ["银行卡", "结账时", "支付顺利通过时最简单的路径"],
          ["加密货币（USDT、BTC 及其他主流币种）", "链上确认之后", "银行卡被拒付，或你不想用卡"],
        ] },
      ] },
      { h2: "一把密钥，所有 Claude 模型，一个余额", blocks: [
        { type: "p", text: "同一把 sk-pool-••• 密钥解锁全部受支持的 Claude 模型，同一个余额还覆盖受支持的 GPT、Gemini 和 Kimi 模型——一个项目混用多家提供商时很方便。下表中的模型 ID 就是你填进请求 model 字段的值。" },
        { type: "table", headers: ["模型", "模型 ID", "什么时候用它"], rows: [
          ["Claude Opus 4.8", "claude-opus-4-8", "最难的推理和长时程智能体编程任务"],
          ["Claude Opus 4.7", "claude-opus-4-7", "需要 Opus 级能力但锁定上一代"],
          ["Claude Sonnet 5", "claude-sonnet-5", "日常编程与对话，中等成本"],
          ["Claude Sonnet 4.6", "claude-sonnet-4-6", "已有提示词背后稳定的 Sonnet 行为"],
          ["Claude Haiku 4.5", "claude-haiku-4-5", "高并发、对延迟敏感的调用"],
        ] },
        { type: "link", text: "各模型价格、缓存费率和上下文窗口", href: "/models" },
      ] },
      { h2: "把 Claude Code、Cursor 和 SDK 指向路由端点", blocks: [
        { type: "p", text: "协议没有任何变化。路由端点讲 Anthropic Messages API——POST /v1/messages，带 x-api-key 和 anthropic-version 头——所以所有兼容 Anthropic 的工具只要覆盖 base URL 就能用。Claude Code 需要两个环境变量：" },
        { type: "code", code: `export ANTHROPIC_BASE_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••\n\n# Claude Code now talks to the router\nclaude` },
        { type: "p", text: "接入 IDE 之前，先用一个请求验证整条链路——计费、密钥和网络：" },
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-haiku-4-5",\n    "max_tokens": 64,\n    "messages": [{"role":"user","content":"Reply with: ok"}]\n  }'` },
        { type: "p", text: "Cursor 和 Cline 在设置面板里填同样的两个值（选 Anthropic 提供商，然后填 Base URL 和 API key），Python 和 TypeScript SDK 则在客户端构造器里接受 base URL。只会讲 OpenAI 协议的工具可以走 OpenAI 兼容通道 https://router.apitoken.sale/v1，用 Authorization: Bearer 加同一把密钥。" },
        { type: "p", text: "流式行为与上游完全一致：传 \"stream\": true，回答会以增量的服务器发送事件（SSE）到达，Claude Code 和聊天前端可以边生成边渲染 token，而不是傻等完整响应。" },
        { type: "note", text: "如果 curl 调用返回 JSON，说明你的网络路径没问题，剩下的都是客户端配置。如果超时，先测 https://router.apitoken.sale 本身的可达性，再动任何配置。" },
      ] },
      { h2: "一次坐下来，从注册到第一个请求", blocks: [
        { type: "steps", items: [
          "注册一个免费的 apiToken.sale 账户——用 Google 或 GitHub 注册可获得 $5 平台奖励金（邮箱/密码注册的账户没有这笔奖励）。",
          "用银行卡或加密货币充值任意整数美元；加密货币充值在网络确认交易后到账。",
          "在控制台生成 API 密钥——形如 sk-pool-…，可用于受支持的 Claude、GPT、Gemini 和 Kimi 模型。",
          "导出 ANTHROPIC_BASE_URL=https://router.apitoken.sale 和你的 ANTHROPIC_API_KEY，或把同样两个值粘进 Cursor 或 Cline。",
          "跑上一节的 curl 冒烟测试；返回 JSON 就说明计费、密钥和网络路径都已打通。",
        ] },
        { type: "note", text: "把 sk-pool-… 密钥当密码对待：任何拿到它的人都能花你的余额。放在环境变量或工具的设置里，绝不要提交进版本库。" },
      ] },
    ],
    faq: [
      { q: "没有外国银行卡，能从俄罗斯为 Claude API 付款吗？", a: "可以。结账通过支付服务商接受银行卡或加密货币，任何一步都不要求受支持的 Anthropic 账单国家。" },
      { q: "在俄罗斯使用 Claude API 需要 VPN 吗？", a: "购买和生成密钥不需要——这些环节没有地域锁定。路由端点的网络可达性取决于你自己的连接，配置工具之前先用一次 curl 调用测一下。" },
      { q: "这和 Anthropic 官方卖的 Claude API 是同一个吗？", a: "是——同一个 Anthropic Messages API，同样的模型 ID（如 claude-opus-4-8），同样的请求和响应格式。区别只在注册和付款方式：预付余额，B2C 账户按官方 API 费用固定优惠 50%。" },
      { q: "预付余额会过期吗？", a: "不会。余额永不过期，只被真实 API 用量消耗——没有月费，也没有固定套餐。" },
      { q: "有俄语支持吗？", a: "有——支持团队通过 Telegram 用俄语和英语回复。" },
    ],
  };
