import type { LocalizedContent } from "../learn";
import { BASE, OPENAI_BASE, KEY } from "../learn-shared";

export const content: LocalizedContent = {
    title: "免费 Claude API 密钥：$5 赠送额度，无需绑卡",
    h1: "几分钟内拿到免费的 Claude API 密钥",
    description: "获取带 $5 平台赠送额度的免费 Claude API 密钥：用 Google 或 GitHub 注册，无需绑卡、无需 Anthropic 账户，即可调用所有受支持的 Claude 模型。",
    keywords: ["免费 claude api 密钥", "claude api 免费", "免费 claude api", "claude api 免费额度", "免费 anthropic api 密钥", "claude api 免绑卡", "claude api 不用信用卡", "claude api 免费点数", "免费试用 claude api", "如何获取免费 claude api 密钥"],
    dek: "免费的 Claude API 密钥只差一次 OAuth 注册：用 Google 或 GitHub 创建 apitoken.sale 账户，$5 平台赠送额度即刻到账——无需绑卡、无需 Anthropic 账户、无需排队。密钥从第一个请求起就说标准的 Anthropic Messages API，现有工具无需改动即可使用。用邮箱和密码注册的账户可以正常使用，但不会获得这笔赠送。",
    sections: [
      { h2: "简要结论：免费额度，不是免费套餐", blocks: [
        { type: "p", text: "从现在开始大约两分钟，你就能拿到一把可用的免费 Claude API 密钥。用 Google 或 GitHub 注册，打开控制台，生成密钥，$5 平台欢迎奖励已经在你的余额里——全程不要求填写任何支付信息。所有受支持的 Claude 模型都能立即调用，走的接口与付费余额完全相同。" },
        { type: "p", text: "先说清楚这份福利是什么、不是什么。它是一次性的额度赠送，用来让你用真实流量评估网关，而不是每月自动补充的长期免费套餐。没有沙箱模式，也没有功能阉割：流式、工具调用和长上下文的表现与付费账户完全一致，因为唯一的区别只在计费来源。" },
      ] },
      { h2: "通过 Google 或 GitHub 领取奖励", blocks: [
        { type: "steps", items: [
          "用 Google 或 GitHub OAuth 创建账户。欢迎奖励只绑定这条注册路径——没有审批队列、邀请制或人工审核。",
          "打开控制台生成一把 API 密钥。密钥形如 sk-pool-…，一把密钥覆盖全部受支持的 Claude 系列——Opus、Sonnet 和 Haiku——共用同一份余额。",
          `按你的工具选择线路协议：在 ${BASE} 使用 Anthropic Messages API 并携带 x-api-key 请求头，或在 ${OPENAI_BASE} 使用 OpenAI 兼容线路并携带 Authorization: Bearer。`,
        ] },
        { type: "note", text: "先用邮箱和密码注册了？这个账户可以正常使用，但永远拿不到欢迎奖励——赠送绑定的是 OAuth 注册方式，而不是新用户身份。如果控制台显示奖励为零，退出登录，改用 Google 或 GitHub 重新注册，而不是赌气直接充值。" },
      ] },
      { h2: "用一次低成本调用验证集成", blocks: [
        { type: "p", text: "别把奖励烧在 Opus 的长篇输出上。用最小的有效请求验证链路：Haiku、一条短提示词、一个硬性 max_tokens 上限。返回 200 且带 usage 字段，就说明鉴权、路由和计量端到端都通了。" },
        { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-haiku-4-5",\n    "max_tokens": 256,\n    "messages": [{"role":"user","content":"Reply with the word ok"}]\n  }'` },
        { type: "p", text: "排错是机械式的。401 说明密钥或 base URL 错了；400 通常是漏了 max_tokens 或模型 ID 拼错；全新账户报余额不足，几乎总是因为账户是用邮箱和密码注册的，奖励从未绑定。" },
      ] },
      { h2: "免费额度实际能用多久", blocks: [
        { type: "p", text: "每个请求都按 Anthropic 官方 token 费率计量，扣减余额前先减去 50% 的 B2C 统一折扣。也就是说，同样的钱在这里能买到大约是官方标价两倍的用量。" },
        { type: "table", headers: ["模型", "官方输入 / 输出（每百万 token，美元）", "本站（−50%）", "$5 约合输出 token 数"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50", "400K"],
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50", "670K"],
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50", "2M"],
        ] },
        { type: "p", text: "把最后一列当作评估预算，而不是生产预算。两百万 Haiku 输出 token 足够你接好编辑器、回放一份真实负载、并用自己的提示词比较模型质量。同样的奖励摊到 Opus 上，一个下午的 agentic 编码就烧光了——在你花真金白银之前，这本身就是有用的信息。" },
        { type: "link", text: "完整的分模型定价（含缓存费率）", href: "/models" },
        { type: "link", text: "在免费计算器中估算你的月度成本", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "同一把密钥也能调用 GPT、Gemini 和 Kimi", blocks: [
        { type: "p", text: `奖励对全部四家提供商的受支持模型都有效，你已经生成的密钥就是唯一需要的凭证。Claude 走 Anthropic Messages 线路；GPT 模型走 OpenAI 兼容线路；Gemini 在 ${BASE} 用 x-goog-api-key 请求头原生应答；Kimi 同时支持 Anthropic Messages 和通用的 OpenAI 兼容线路。` },
        { type: "p", text: "这正是免费额度被低估的地方：它是一份跨提供商的基准测试预算。用同一组提示词跑 Claude 和它的替代品，在你自己的任务上衡量质量，而不是看厂商排行榜，然后再决定付费余额花在哪里。" },
      ] },
      { h2: "额度用完之后", blocks: [
        { type: "p", text: "通过安全的收银渠道用银行卡或加密货币充值任意整数美元金额——没有固定套餐目录。统一折扣自动应用于之后的每个请求，不需要解锁，也不需要谈判。" },
        { type: "p", text: "没有订阅、没有月度最低消费，预付余额永不过期，闲置一个月的成本恰好是零。把首次充值当作评估的延续：充一小笔，把一个真实项目接到网关上，等数字证明值得时再扩大规模。" },
      ] },
    ],
    faq: [
      { q: "免费的 Claude API 密钥是沙箱还是真实 API？", a: "真实的。Google/GitHub 账户的 $5 奖励跑在与付费余额相同的受支持模型和接口上，包括流式和工具调用——只有计费来源不同。" },
      { q: "获取免费的 Claude API 密钥需要信用卡吗？", a: "任何环节都不需要银行卡。用 Google 或 GitHub 创建账户，无需任何支付信息即可获得 $5 平台奖励。" },
      { q: "为什么我没有收到 $5 欢迎奖励？", a: "只有通过 Google 或 GitHub OAuth 创建的账户才能获得。用邮箱和密码注册的账户功能完整，但不在赠送范围内。" },
      { q: "免费额度或余额会过期吗？", a: "奖励是面向新账户的一次性赠送，而不是会自动补充的免费套餐；预付余额永不过期——闲置期间没有月费侵蚀它。" },
      { q: "免费密钥能在 Cursor、Claude Code 或 Anthropic SDK 里用吗？", a: `可以。任何兼容 Anthropic 的客户端都行：把 base URL 设为 ${BASE}，密钥通过 x-api-key 发送，anthropic-version 请求头保持与官方 API 的要求完全一致。` },
      { q: "免费额度可以调用哪些模型？", a: "所有受支持的 Claude 模型——Opus 4.8 和 4.7、Sonnet 5 和 4.6、Haiku 4.5——外加同一把密钥、同一份余额下受支持的 GPT、Gemini 和 Kimi 系列。" },
    ],
  };
