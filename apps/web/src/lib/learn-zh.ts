import type { LocalizedContent } from "./learn";

export const learnZh: Record<string, LocalizedContent> = {
  "how-to-buy-claude-api-key": {
    title: "如何购买 Claude API 密钥",
    h1: "如何购买 Claude API 密钥",
    description: "在 apitoken.sale 上几分钟内买到 Claude API 密钥——一把密钥通用所有 Claude 模型，预付余额，支持银行卡或加密货币支付，无需 Anthropic 账户。",
    keywords: ["购买 claude api 密钥", "如何购买 claude api", "claude api key", "获取 claude api 权限", "anthropic api 密钥"],
    dek: "无需 Anthropic 账户、无需邀请码、也不用公司信用卡即可开始使用 Claude。在 apitoken.sale 上你购买预付余额、生成一把密钥，就能以折扣价调用同一套 Anthropic Messages API。",
    sections: [
      { h2: "三步拿到你的密钥", blocks: [
        { type: "steps", items: [
          "创建一个免费账户并打开控制台——无需审批、无需排队。",
          "生成一把 API 密钥（形如 sk-pool-…）。同一把密钥可用于所有受支持的 Claude 模型。",
          "将任意兼容 Anthropic 的工具指向 https://api.apitoken.sale，并携带 x-api-key 请求头向 /v1/messages 发送请求。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "支付方式如何运作", blocks: [
        { type: "p", text: "想充多少就充多少（整数美元）——没有固定的产品套餐。你的余额为预付制，永不过期，仅在 API 请求实际运行时才会扣费。" },
        { type: "list", items: [
          "通过安全的收银服务商用银行卡或加密货币支付。",
          "每次请求都会先换算为官方 Anthropic API 消费，再套用你当前的折扣。",
          "B2C 账户起步即比官方消费低 60%，随着累计充值增加最高可达 70% 折扣。",
        ] },
      ] },
      { h2: "拿到密钥能做什么", blocks: [
        { type: "p", text: "一把密钥即可解锁全部受支持的 Claude 系列——Opus、Sonnet 和 Haiku——覆盖 Claude Code、Cursor、Cline、Continue、Zed 以及官方 Anthropic SDK。协议本身毫无变化，改变的只有价格。" },
      ] },
      { h2: "你能用到哪些 Claude 模型和工具", blocks: [
        { type: "p", text: "一把 Claude API 密钥即可在同一余额下解锁全部受支持的模型系列，并适用于所有兼容 Anthropic 的工具。" },
        { type: "list", items: [
          "模型：Claude Opus 4.8 与 4.7、Sonnet 5 与 4.6、Haiku 4.5。",
          "工具：Claude Code、Cursor、Cline、Continue、Zed 以及 Anthropic SDK。",
          "格式：支持流式输出与工具调用的 Anthropic Messages API。",
        ] },
      ] },
    ],
    faq: [
      { q: "购买 Claude API 密钥需要 Anthropic 账户吗？", a: "不需要。apitoken.sale 自行签发密钥和余额，因此你无需 Anthropic 账户、邀请码或审批即可开始。" },
      { q: "密钥多快能激活？", a: "即时激活。你在控制台生成密钥后，下一次请求即可使用——没有排队，也没有人工审核。" },
      { q: "起步要花多少钱？", a: "你可以充值任意整数美元金额。通过 Google 或 GitHub 创建的新账户还会免费获得价值 $10 的 Claude 官方价格用量。" },
      { q: "这是官方的 Claude API 吗？", a: "是的——它提供同一套 Anthropic Messages API 和同样的 Claude 模型。不同的只有价格以及注册和付款方式。" },
    ],
  },
  "cheapest-claude-api": {
    title: "最便宜的 Claude API——最高立省 70%",
    h1: "使用 Claude API 最省钱的方式",
    description: "把 Claude API 成本最高削减 70%。apitoken.sale 以预付折扣价出售一模一样的 Anthropic Messages API——同样的模型、同样的接口、更低的每 token 单价。",
    keywords: ["最便宜的 claude api", "claude api 折扣", "便宜的 claude api", "claude api 价格", "节省 anthropic api 费用", "比 anthropic 更便宜的 claude api"],
    dek: "Claude API 按 token 计费，而在漫长的编码会话中这些 token 累积得很快。apitoken.sale 通过汇集预付余额并套用递进折扣，让你以最高低 70% 的价格用上完全相同的 API。",
    sections: [
      { h2: "为什么更便宜", blocks: [
        { type: "p", text: "你向同一套 Anthropic Messages API 发送同样的请求，得到同样的响应。底层唯一不同的是计费：每次调用按官方费率计量，然后在扣减你的余额前先减去你的折扣。" },
        { type: "list", items: [
          "B2C 账户从比官方消费低 60% 起步。",
          "随着累计充值增加，折扣最高可达 70%。",
          "B2B 批量定价单独商议。",
        ] },
      ] },
      { h2: "省钱效果最明显的场景", blocks: [
        { type: "p", text: "智能体编码、漫长的多轮会话以及重度依赖提示缓存的工作流消耗的 token 最多——因此绝对节省额也最大。为每项任务选对模型还能进一步叠加节省。" },
        { type: "note", text: "小贴士：把快速、廉价的工作交给 Haiku，把 Opus 留给高难度推理，能让余额撑得更久。" },
      ] },
      { h2: "无订阅、无绑定", blocks: [
        { type: "p", text: "没有月费。你充值的是永不过期的预付余额，仅在请求运行时才消耗，因此闲置的日子不花一分钱。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "Claude API 折扣如何生效", blocks: [
        { type: "p", text: "没有加价，也没有单独的廉价模型——你得到的是对完全相同的 Claude API 的折扣访问。" },
        { type: "list", items: [
          "每次请求按官方 Anthropic token 费率计量。",
          "减去你的递进折扣（60% 直至 70%）。",
          "净额从你的预付余额中扣除。",
        ] },
        { type: "table", headers: ["模型", "官方 输入 / 输出（$ / 1M）", "本站起价（−60%）", "最优（−70%）"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2 / $10", "$1.50 / $7.50"],
          ["Claude Opus 4.7", "$5 / $25", "$2 / $10", "$1.50 / $7.50"],
          ["Claude Sonnet 5", "$3 / $15", "$1.20 / $6", "$0.90 / $4.50"],
          ["Claude Sonnet 4.6", "$3 / $15", "$1.20 / $6", "$0.90 / $4.50"],
          ["Claude Haiku 4.5", "$1 / $5", "$0.40 / $2", "$0.30 / $1.50"],
        ] },
        { type: "link", text: "每个模型的完整价格（含缓存费率）", href: "/models" },
        { type: "link", text: "用免费计算器估算你的月度成本", href: "/tools/claude-api-cost-calculator" },
      ] },
    ],
    faq: [
      { q: "这真的是同一套 Claude API 吗？", a: "是的——同一套 Anthropic Messages API、相同的模型 ID、相同的请求与响应格式。只有每次调用的价格更低。" },
      { q: "我能省多少？", a: "B2C 定价从比官方 API 消费低 60% 起步，随着累计充值提高最多可达 70% 折扣。" },
      { q: "有没有隐藏费用或订阅？", a: "没有。余额为预付制、永不过期，仅由真实 API 用量消耗——没有月费。" },
      { q: "有比直接从 Anthropic 购买更便宜的 Claude API 吗？", a: "有。apiToken.sale 以最高 70% 的递进折扣出售一模一样的 Anthropic API，且没有订阅。" },
    ],
  },
  "claude-api-for-russia": {
    title: "从俄罗斯及受限地区使用 Claude API",
    h1: "在俄罗斯使用 Claude API",
    description: "通过 apitoken.sale 从俄罗斯及其他受限地区访问 Claude API——无需 Anthropic 账户，支持银行卡或加密货币支付，一把密钥通用所有 Claude 模型。",
    keywords: ["俄罗斯 claude api", "从俄罗斯使用 claude api", "anthropic api 俄罗斯", "claude api 受限地区", "claude api 支付", "claude api 免翻墙"],
    dek: "Anthropic 并非在每个国家都直接销售，这让俄罗斯及其他地区的开发者缺乏明确的付款途径。apitoken.sale 消除了这道障碍：你购买预付余额即可拿到一把可用密钥，无论 Anthropic 在哪里开票。",
    sections: [
      { h2: "为什么直接访问很难", blocks: [
        { type: "p", text: "在 Anthropic 注册通常要求受支持的开票国家和支付方式。如果你无法完成这一步，就拿不到密钥——即便模型本身在网络上是可达的。" },
      ] },
      { h2: "apitoken.sale 如何解决", blocks: [
        { type: "list", items: [
          "无需 Anthropic 账户——密钥和余额由我们签发。",
          "用银行卡或加密货币支付，哪种方便用哪种。",
          "即时激活，无需排队，无需公司核验。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "与你现有的工具兼容", blocks: [
        { type: "p", text: "将 Claude Code、Cursor、Cline 或 Anthropic SDK 指向 https://api.apitoken.sale，即可像以前一样继续工作。支持提供俄语和英语服务，通过 Telegram 联系。" },
      ] },
      { h2: "在俄罗斯免 VPN 使用 Claude API", blocks: [
        { type: "p", text: "签发密钥和余额没有 Anthropic 开票国家的门槛，因此你不需要外国银行卡或公司即可开始。网络可达性取决于你自己的连接，但购买余额和生成密钥都没有地域限制。" },
      ] },
    ],
    faq: [
      { q: "我能从俄罗斯付款吗？", a: "可以。你可以通过收银服务商用银行卡或加密货币支付，因此不要求受支持的 Anthropic 开票国家。" },
      { q: "我需要 VPN 吗？", a: "你无需 Anthropic 账户或开票国家。网络可达性取决于你自己的连接，但签发密钥和余额没有地域限制。" },
      { q: "有俄语支持吗？", a: "有——支持提供俄语和英语服务，通过 Telegram 联系。" },
      { q: "我能从俄罗斯为 Claude API 付款吗？", a: "可以——用银行卡或加密货币支付，因此不要求受支持的 Anthropic 开票国家。" },
    ],
  },
  "claude-api-crypto-payment": {
    title: "用加密货币支付 Claude API",
    h1: "用加密货币支付 Claude API",
    description: "在 apitoken.sale 上用加密货币或银行卡购买 Claude API 余额。无需 Anthropic 账户，即时开通，预付余额永不过期。",
    keywords: ["claude api 加密货币支付", "用加密货币买 claude api", "claude api usdt", "加密货币支付 anthropic api", "claude api 比特币"],
    dek: "如果银行卡不是一个选项——或者你就是更偏好加密货币——你可以用加密货币为 Claude API 余额充值并立即开始。",
    sections: [
      { h2: "银行卡或加密货币，任你选择", blocks: [
        { type: "p", text: "结账时你可以通过安全的支付服务商用银行卡或加密货币支付。无论哪种方式，余额都会以预付形式进入你的账户，仅在请求运行时才扣费。" },
      ] },
      { h2: "加密货币为什么有帮助", blocks: [
        { type: "list", items: [
          "无需 Anthropic 支持的计费国家。",
          "在银行卡被拒或不可用的地方很实用。",
          "余额永不过期，因此你充值一次，边构建边扣减。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "结账时会遇到什么", blocks: [
        { type: "p", text: "在结账时选择加密货币，向显示的地址转入金额，网络确认后你的余额即入账。若你更愿意用银行卡进行某笔特定充值，银行卡依然可用。" },
        { type: "list", items: [
          "链上确认后余额入账。",
          "任意整数美元金额；余额永不过期。",
          "每次充值都可在银行卡和加密货币之间切换。",
        ] },
      ] },
      { h2: "可以用哪些加密货币支付", blocks: [
        { type: "p", text: "加密货币充值通过安全的支付服务商处理，因此常见币种都受支持。" },
        { type: "list", items: [
          "USDT 及其他稳定币。",
          "BTC 及主流加密货币。",
          "网络确认交易后余额即入账。",
        ] },
      ] },
    ],
    faq: [
      { q: "支持哪些支付方式？", a: "你可以通过收银服务商用银行卡或加密货币支付。" },
      { q: "余额会过期吗？", a: "不会。预付余额永不过期，仅在真实 API 使用时才消耗。" },
      { q: "我能用 USDT 购买 Claude API 吗？", a: "可以——结账时你可以用 USDT 或其他受支持的加密货币为 Claude API 余额充值。" },
    ],
  },
  "claude-api-without-waitlist": {
    title: "无需排队或审批的 Claude API",
    h1: "无需排队即可使用 Claude API",
    description: "跳过 Anthropic 的排队和审批。在 apitoken.sale 上创建账户、生成 Claude API 密钥，几分钟内即可发出第一个调用。",
    keywords: ["claude api 无排队", "claude api 即时开通", "claude api 无需审批", "快速获取 claude api 密钥", "claude api 无需 anthropic 账户"],
    dek: "等待审批会消磨积极性。apitoken.sale 让你即时、自助地用上所有受支持的 Claude 模型——无排队、无销售电话、无公司验证。",
    sections: [
      { h2: "即时、自助开通", blocks: [ { type: "steps", items: [
          "创建一个免费账户并打开控制台——无需审批、无需排队。",
          "生成一把 API 密钥（形如 sk-pool-…）。同一把密钥可用于所有受支持的 Claude 模型。",
          "将任意兼容 Anthropic 的工具指向 https://api.apitoken.sale，并携带 x-api-key 请求头向 /v1/messages 发送请求。",
        ] }, { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" } ] },
      { h2: "「即时」到底是什么意思", blocks: [
        { type: "p", text: "你一生成密钥它就是激活的。从注册到第一个成功请求之间没有任何人工审核步骤，因此你可以在同一次坐下就接通工具并交付。" },
      ] },
      { h2: "从零到第一个调用", blocks: [
        { type: "list", items: [
          "注册并打开控制台——没有审批步骤。",
          "生成密钥并把你的工具指向 api.apitoken.sale。",
          "发出请求，即可在用量中看到它被计量。",
        ] },
        { type: "p", text: "通过 Google 或 GitHub 创建的新账户还会附带价值 $10 的 Claude 官方价格用量，因此你可以在充值前验证整个流程。" },
      ] },
    ],
    faq: [
      { q: "真的没有排队吗？", a: "没错。开通是自助且即时的——你生成一把密钥，它在下一次请求就能用。" },
      { q: "我需要联系销售吗？", a: "不需要。B2C 开通完全自助。只有需要商议的 B2B 批量定价才涉及沟通。" },
    ],
  },
  "claude-api-quick-setup": {
    title: "两分钟完成 Claude API 配置",
    h1: "两分钟配置好 Claude API",
    description: "两分钟的 Claude API 快速上手：创建密钥、把 Base URL 设为 api.apitoken.sale，然后用 curl、Python 或你的 IDE 发出第一个 /v1/messages 请求。",
    keywords: ["claude api 快速上手", "claude api 配置", "claude api 第一个请求", "anthropic messages api", "claude api base url"],
    dek: "这是从零到跑通 Claude API 调用的最快路径。下面的一切都使用标准的 Anthropic Messages API，因此可以直接嵌入你现有的代码。",
    sections: [
      { h2: "1. 创建密钥", blocks: [ { type: "p", text: "注册、打开控制台并生成一把密钥。它形如 sk-pool-…，可用于所有受支持的模型。" } ] },
      { h2: "2. 设置端点", blocks: [
        { type: "p", text: "将任意兼容 Anthropic 的客户端指向网关：" },
        { type: "code", code: `Base URL:  https://api.apitoken.sale\nEndpoint:  POST /v1/messages\nHeaders:   x-api-key: sk-pool-•••\n           anthropic-version: 2023-06-01` },
      ] },
      { h2: "3. 发出第一个请求", blocks: [
        { type: "code", code: `curl https://api.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-opus-4-8",\n    "max_tokens": 1024,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "首次调用的常见错误", blocks: [
        { type: "list", items: [
          "401 Unauthorized——缺少或写错 x-api-key，或 Base URL 有误。",
          "400 Bad Request——检查模型 ID，并确认已设置 max_tokens。",
          "429 Too Many Requests——遵守 Retry-After 并降低并发。",
          "402 / 余额不足——充值任意整数美元金额即可。",
        ] },
      ] },
    ],
    faq: [
      { q: "我该用哪个 Base URL？", a: "在任意兼容 Anthropic 的工具中使用 https://api.apitoken.sale，并向 /v1/messages 发送请求。" },
      { q: "需要哪个鉴权请求头？", a: "发送 x-api-key（携带你的密钥）和 anthropic-version，与官方 Anthropic API 完全一致。" },
    ],
  },
  "free-claude-api-key": {
    title: "免费 Claude API 密钥助你上手",
    h1: "获取免费 Claude API 密钥开始使用",
    description: "通过 Google 或 GitHub 在 apitoken.sale 创建 Claude API 密钥，并获得价值 $10 的 Claude 官方价格用量——无需银行卡或 Anthropic 账户。",
    keywords: ["免费 claude api 密钥", "claude api 免费", "claude api 免费额度", "免费 anthropic api 密钥", "claude api 免银行卡"],
    dek: "通过 Google 或 GitHub 创建账户，即可获得价值 $10 的 Claude 官方价格用量并在充值前验证集成。邮箱密码账户不享受此奖励。",
    sections: [
      { h2: "“免费”包含什么", blocks: [
        { type: "list", items: [
          "一把可用于所有受支持 Claude 模型的 API 密钥。",
          "Google/GitHub 新账户可获一次性价值 $10 的 Claude 用量，无需银行卡。",
          "足够的额度让你接通工具并跑通真实请求。",
        ] },
        { type: "p", text: "当你准备用更多时，充值任意整数美元金额，你的折扣就会自动生效。" },
      ] },
      { h2: "如何领取", blocks: [
        { type: "steps", items: [
          "通过 Google 或 GitHub 创建账户并打开控制台——无需审批、无需排队。",
          "生成一把 API 密钥（形如 sk-pool-…）。同一把密钥可用于所有受支持的 Claude 模型。",
          "将任意兼容 Anthropic 的工具指向 https://api.apitoken.sale，并携带 x-api-key 请求头向 /v1/messages 发送请求。",
        ] },
      ] },
      { h2: "Claude API 是永久免费的吗？", blocks: [
        { type: "p", text: "包含的 $10 是免费起步额度，而不是无限的免费套餐。用完之后，你只为实际消耗的 token 付费——没有订阅、没有月度最低消费，预付余额也永不过期。" },
      ] },
    ],
    faq: [
      { q: "这些免费用量是真正的 API 访问吗？", a: "是的。Google/GitHub 账户的 $10 奖励可用于与付费余额相同的 Claude 模型和接口。" },
      { q: "开始使用需要银行卡吗？", a: "无需银行卡。通过 Google 或 GitHub 创建账户即可获得包含的 $10 用量。" },
      { q: "免费的 Claude API 密钥需要信用卡吗？", a: "不需要。通过 Google 或 GitHub 创建账户，即可在没有银行卡的情况下获得价值 $10 的 Claude 官方价格用量。" },
    ],
  },
  "claude-api-free-trial": {
    title: "Claude API 免费试用——几分钟即可开始",
    h1: "免费试用 Claude API",
    description: "几分钟内开始用 Claude 编码。通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量，无需银行卡。",
    keywords: ["claude api 免费试用", "试用 claude api", "claude api 测试", "claude api 沙盒", "claude api 演示"],
    dek: "无需单独申请试用——通过 Google 或 GitHub 创建账户，即可获得价值 $10 的官方价格用量，并对所有受支持的模型运行真实调用。",
    sections: [
      { h2: "先验证再付费", blocks: [
        { type: "p", text: "包含的用量正是为端到端检验网关而设计的：创建密钥、连接你的编辑器，确认流式输出、工具调用以及你喜欢的模型都表现如预期。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码注册不享受此奖励。" },
      ] },
      { h2: "随后按你的节奏扩展", blocks: [
        { type: "p", text: "当试用用量所剩不多时，充值任意金额即可。没有订阅、余额永不过期，因此你只为实际调用的部分付费。" },
      ] },
    ],
    faq: [
      { q: "我如何开始试用？", a: "通过 Google 或 GitHub 创建新账户，价值 $10 的官方价用量会自动添加；邮箱密码账户不参与。" },
      { q: "免费用量用完后会怎样？", a: "充值任意整数美元金额即可继续；你的递进折扣会立即生效。" },
    ],
  },
  "claude-code-without-subscription": {
    title: "无需订阅即可使用 Claude Code",
    h1: "无需 $200/月套餐使用 Claude Code",
    description: "用即用即付的 API 余额运行 Claude Code，而非按月订阅。将 ANTHROPIC_BASE_URL 设为 api.apitoken.sale，只为实际使用量付费。",
    keywords: ["claude code 无需订阅", "claude code api 密钥", "claude code 即用即付", "claude code 便宜", "claude code 免订阅"],
    dek: "使用 Claude Code 不一定意味着固定月费套餐。把它指向一把带预付余额的 API 密钥，你就按 token 付费——如果你的用量起伏不定或只是想试试，这非常理想。",
    sections: [
      { h2: "两个环境变量", blocks: [
        { type: "code", code: `export ANTHROPIC_BASE_URL=https://api.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••\n\n# then just run\nclaude` },
        { type: "p", text: "全部改动就这些。Claude Code 保留每一项功能——它只是以折扣价从你的预付余额扣费，而非走订阅。" },
      ] },
      { h2: "即用即付何时更划算", blocks: [
        { type: "list", items: [
          "偶尔或突发式的用量，此时固定月费很浪费。",
          "在决定订阅套餐前先试用 Claude Code。",
          "让多个工具共用一份余额和一把密钥。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "Claude Code 能用自定义 API 密钥吗？", a: "可以。设置 ANTHROPIC_BASE_URL 和 ANTHROPIC_API_KEY，Claude Code 就会直接使用你的密钥和余额。" },
      { q: "我会失去任何功能吗？", a: "不会。Claude Code 表现完全一致；只是计费从订阅变为按 token 预付使用。" },
    ],
  },
  "claude-opus-api": {
    title: "Claude Opus API 访问",
    h1: "通过 API 使用 Claude Opus 4.8",
    description: "通过一把 apitoken.sale 密钥以最高低于官方费率 70% 的价格访问 Claude Opus 4.8 和 4.7。最适合复杂推理、重构与长时间的智能体会话。",
    keywords: ["claude opus api", "claude opus 4.8 api", "opus api 密钥", "claude opus 价格", "claude opus 折扣"],
    dek: "Opus 是 Claude 能力最强的档位——面对高难度推理、架构设计和长时间智能体运行时应当选它。apitoken.sale 让你在与其他模型相同的密钥和余额上使用 Opus 4.8 和 4.7。",
    sections: [
      { h2: "何时使用 Opus", blocks: [
        { type: "list", items: [
          "复杂重构与跨文件改动。",
          "架构设计、规划与高风险推理。",
          "对一致性和缓存复用要求高的长时间会话。",
        ] },
      ] },
      { h2: "在你的余额上使用 Opus", blocks: [
        { type: "p", text: "Opus 4.8（模型 ID claude-opus-4-8）和 Opus 4.7 按官方 token 费率减去你的折扣计费，因此你能以标价的一小部分用上顶级档位。" },
        { type: "table", headers: ["模型", "官方 输入 / 输出（$ / 1M）", "本站起价（−60%）", "最优（−70%）"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2 / $10", "$1.50 / $7.50"],
          ["Claude Opus 4.7", "$5 / $25", "$2 / $10", "$1.50 / $7.50"],
        ] },
        { type: "link", text: "Claude Opus 4.8 详细价格（缓存、上下文、FAQ）", href: "/models/claude-opus-4-8" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "有哪些 Opus 模型可用？", a: "Claude Opus 4.8（claude-opus-4-8）和 Claude Opus 4.7，与 Sonnet、Haiku 共用同一把密钥和预付余额。" },
      { q: "Opus 值得多花那些 token 吗？", a: "对于复杂推理、重构和长时间智能体运行，值得。对于快速、廉价的任务，Haiku 或 Sonnet 通常更划算。" },
    ],
  },
  "claude-sonnet-api": {
    title: "Claude Sonnet API 访问",
    h1: "通过 API 使用 Claude Sonnet",
    description: "通过 apitoken.sale 使用 Claude Sonnet 5 和 Sonnet 4.6——日常编码与智能体的默认模型，最高享受官方 API 价格 70% 的折扣。",
    keywords: ["claude sonnet api", "claude sonnet 5 api", "sonnet api 密钥", "claude sonnet 价格", "最适合编码的 claude 模型"],
    dek: "Sonnet 是主力：足够快，适合交互式编码；又足够聪明，胜任真正的智能体工作流。apitoken.sale 在一份折扣余额上提供 Sonnet 5 和 Sonnet 4.6。",
    sections: [
      { h2: "日常主力模型", blocks: [
        { type: "p", text: "对于大多数编码和智能体任务，Sonnet 是合适的默认选择——在质量、速度和成本之间取得了很好的平衡。把 Opus 留给真正的难题。" },
      ] },
      { h2: "Sonnet 定价说明", blocks: [
        { type: "p", text: "Claude Sonnet 5（claude-sonnet-5）采用介绍期官方费率，引擎始终在套用你的折扣前应用当前有效费率。Sonnet 4.6 仍可在同一把密钥上使用。" },
        { type: "table", headers: ["模型", "官方 输入 / 输出（$ / 1M）", "本站起价（−60%）", "最优（−70%）"], rows: [
          ["Claude Sonnet 5", "$3 / $15", "$1.20 / $6", "$0.90 / $4.50"],
          ["Claude Sonnet 4.6", "$3 / $15", "$1.20 / $6", "$0.90 / $4.50"],
        ] },
        { type: "link", text: "Claude Sonnet 5 详细价格（缓存、上下文、FAQ）", href: "/models/claude-sonnet-5" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "我能用哪些 Sonnet 模型？", a: "Claude Sonnet 5（claude-sonnet-5）和 Claude Sonnet 4.6，与 Opus、Haiku 共用同一份余额。" },
      { q: "Sonnet 适合编码吗？", a: "适合——Sonnet 是日常编码和智能体工作流推荐的默认模型。" },
    ],
  },
  "claude-haiku-api": {
    title: "Claude Haiku API 访问",
    h1: "通过 API 使用 Claude Haiku 4.5",
    description: "通过 apitoken.sale 访问 Claude Haiku 4.5——最快、最经济的 Claude 模型，以预付折扣价理想应对高并发和低延迟任务。",
    keywords: ["claude haiku api", "claude haiku 4.5 api", "最快的 claude 模型", "便宜的 claude 模型", "haiku api 密钥"],
    dek: "Haiku 为速度和吞吐量而生：分类、抽取、路由以及任何延迟和成本比深度推理更重要的任务。",
    sections: [
      { h2: "何时该选 Haiku", blocks: [
        { type: "list", items: [
          "高并发、低延迟的请求。",
          "廉价的后台任务和预处理。",
          "在无需 Opus 的工作上让余额撑得更久。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "一把密钥混用多种模型", blocks: [
        { type: "p", text: "由于所有模型共用一把密钥和余额，你可以把廉价工作路由给 Haiku（claude-haiku-4-5），只把高难度请求升级到 Sonnet 或 Opus。" },
        { type: "table", headers: ["模型", "官方 输入 / 输出（$ / 1M）", "本站起价（−60%）", "最优（−70%）"], rows: [
          ["Claude Haiku 4.5", "$1 / $5", "$0.40 / $2", "$0.30 / $1.50"],
        ] },
        { type: "link", text: "Claude Haiku 4.5 详细价格（缓存、上下文、FAQ）", href: "/models/claude-haiku-4-5" },
      ] },
    ],
    faq: [
      { q: "Haiku 有多快、多便宜？", a: "Haiku 4.5 是速度最快、成本最低的 Claude 模型，非常适合高并发、对延迟敏感的工作。" },
      { q: "我能把 Haiku 与其他模型组合使用吗？", a: "可以。一把密钥和余额覆盖 Haiku、Sonnet 和 Opus，因此你能为每项任务路由到性价比最高的模型。" },
    ],
  },
  "claude-api-key-for-cursor": {
    title: "用于 Cursor 的 Claude API 密钥",
    h1: "在 Cursor 中使用 Claude API 密钥",
    description: "用 apitoken.sale 密钥把 Cursor 接入 Claude：将 Anthropic Base URL 设为 api.apitoken.sale，粘贴密钥，选择模型，即可以最高 70% 折扣编码。",
    keywords: ["用于 cursor 的 claude api 密钥", "cursor claude api", "cursor anthropic 密钥", "在 cursor 中用 claude", "不买 cursor pro 用 cursor"],
    dek: "Cursor 允许你自带 Anthropic 密钥，这意味着你可以用折扣预付余额在 Cursor 中运行 Claude，而不必依赖捆绑套餐。",
    sections: [
      { h2: "三步配置", blocks: [
        { type: "steps", items: [
          "打开 Cursor → Settings → Models → Anthropic API。",
          "把 Base URL 设为 https://api.apitoken.sale，并粘贴你的 sk-pool-••• 密钥。",
          "选择一个模型，例如 claude-opus-4-8，即可开始编码。",
        ] },
      ] },
      { h2: "配置", blocks: [
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : https://api.apitoken.sale\nAPI key  : sk-pool-•••\nModel    : claude-opus-4-8` },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "故障排查", blocks: [
        { type: "list", items: [
          "Cursor 忽略密钥：确认你编辑的是 Anthropic 提供方，而不是 OpenAI。",
          "找不到模型：设置一个当前的模型 ID，例如 claude-opus-4-8。",
          "401：重新检查 Base URL，并确认密钥完整粘贴。",
        ] },
        { type: "p", text: "连接成功后，所有受支持的 Claude 模型都可在同一把密钥和余额下使用。" },
      ] },
      { h2: "任何语言都能在 Cursor 中使用你的 Claude API 密钥", blocks: [
        { type: "p", text: "密钥与语言无关——无论是 Python、JavaScript、TypeScript、Go、Rust 还是其他项目，Cursor 都能在 Windows、macOS 和 Linux 上使用它。你配置的是模型提供方，而不是编程语言。" },
      ] },
    ],
    faq: [
      { q: "我能在 Cursor 里用自己的 Claude 密钥吗？", a: "可以。Cursor 的 Anthropic 提供方接受自定义 Base URL 和密钥，因此你可以把它指向 apitoken.sale。" },
      { q: "我还需要 Cursor Pro 吗？", a: "你可以用自己的 API 密钥和余额运行 Claude；而需要 Cursor 自身套餐的功能则与模型提供方无关，属于另一回事。" },
      { q: "Claude API 密钥能在 Windows 和 Mac 的 Cursor 里用吗？", a: "可以——Anthropic 提供方设置在 Windows、macOS 和 Linux 上完全相同。" },
    ],
  },
  "claude-api-for-vs-code": {
    title: "在 VS Code 中使用 Claude API（Cline、Continue）",
    h1: "在 VS Code 中使用 Claude API",
    description: "使用 apitoken.sale 密钥，通过 Cline 或 Continue 在 VS Code 中运行 Claude。把 Anthropic Base URL 设为 api.apitoken.sale，即可按 token 折扣付费。",
    keywords: ["claude api vs code", "cline claude api", "continue claude api", "vscode 用 claude", "vscode anthropic api 密钥"],
    dek: "Cline、Continue 等免费的 VS Code 智能体接受任何兼容 Anthropic 的端点，因此你可以用折扣余额在 VS Code 里用 Claude 编码。",
    sections: [
      { h2: "Cline", blocks: [
        { type: "code", code: `# Cline → Settings\nAPI Provider : Anthropic\nBase URL     : https://api.apitoken.sale\nAPI Key      : sk-pool-•••\nModel        : claude-opus-4-8` },
      ] },
      { h2: "Continue", blocks: [
        { type: "code", code: `// ~/.continue/config.json\n{\n  "models": [{\n    "title": "Claude via apiToken.sale",\n    "provider": "anthropic",\n    "apiBase": "https://api.apitoken.sale",\n    "apiKey": "sk-pool-•••",\n    "model": "claude-opus-4-8"\n  }]\n}` },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "选哪个扩展与故障排查", blocks: [
        { type: "p", text: "Cline 适合作为自主编辑的默认之选；Continue 更轻量，适合内联对话和补全。两者都免费，且都使用你的预付余额。" },
        { type: "list", items: [
          "401 Unauthorized：API 密钥或 Base URL 有误。",
          "找不到模型：使用当前的模型 ID，例如 claude-sonnet-5 或 claude-opus-4-8。",
          "缓慢或 429：降低并发并遵守 Retry-After。",
        ] },
      ] },
    ],
    faq: [
      { q: "哪些 VS Code 扩展可以用？", a: "任何支持兼容 Anthropic 端点的扩展都可以，包括 Cline 和 Continue，均可搭配 apitoken.sale 密钥使用。" },
      { q: "需要付费扩展吗？", a: "不需要。Cline 和 Continue 都是免费的；你只为消耗预付余额的 Claude API 用量付费。" },
    ],
  },
  "cursor-without-anthropic-account": {
    title: "无需 Anthropic 账户在 Cursor 中用 Claude",
    h1: "无需 Anthropic 账户在 Cursor 中运行 Claude",
    description: "没有 Anthropic 账户？改用 apitoken.sale 密钥在 Cursor 中使用 Claude。即时开通，支持银行卡或加密货币支付，官方费率最高立省 70%。",
    keywords: ["无 anthropic 账户用 cursor", "cursor claude 无 anthropic", "cursor claude api 密钥", "不用 anthropic 账户用 claude"],
    dek: "如果你无法或不愿创建 Anthropic 账户，apitoken.sale 会签发自己的密钥，Cursor 会把它当作 Anthropic 提供方来接受。",
    sections: [
      { h2: "为什么可行", blocks: [
        { type: "p", text: "Cursor 与 Anthropic Messages API 通信。apitoken.sale 对外暴露的正是这套 API，因此 Cursor 分辨不出差别——它只是使用你的密钥和 Base URL。" },
      ] },
      { h2: "配置方法", blocks: [
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : https://api.apitoken.sale\nAPI key  : sk-pool-•••\nModel    : claude-opus-4-8` },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "你能保留什么", blocks: [
        { type: "list", items: [
          "完整的 Claude 系列——Opus、Sonnet 和 Haiku——都在一把密钥下。",
          "标准的 Anthropic 行为：流式输出、工具调用、系统提示。",
          "每把密钥可选终身累计消费上限和到期日期，并在控制台查看 token 级用量。",
        ] },
        { type: "p", text: "你使用 Cursor 的方式毫无变化；只是把密钥来源从 Anthropic 换成了 apitoken.sale。" },
      ] },
    ],
    faq: [
      { q: "这样做需要 Anthropic 账户吗？", a: "不需要。apitoken.sale 提供密钥和余额，因此无需 Anthropic 账户。" },
      { q: "这个集成用的是官方 Anthropic API 吗？", a: "Cursor 使用标准的 Anthropic Messages API；apitoken.sale 以折扣价提供同一套 API。" },
    ],
  },
  "anthropic-sdk-base-url": {
    title: "在 Anthropic SDK 中使用自定义 Base URL",
    h1: "将 Anthropic SDK 指向 apitoken.sale",
    description: "只需把 base_url 设为 api.apitoken.sale，即可在官方 Anthropic Python 和 TypeScript SDK 中使用 apitoken.sale。同样的 SDK、同样的代码，每 token 成本更低。",
    keywords: ["anthropic sdk base url", "anthropic python sdk 自定义端点", "claude sdk base url", "anthropic typescript sdk", "claude api sdk 配置"],
    dek: "官方 Anthropic SDK 允许覆盖 Base URL，因此切换到 apitoken.sale 只是一行改动——你的模型 ID 和消息代码完全保持不变。",
    sections: [
      { h2: "Python", blocks: [
        { type: "code", code: `from anthropic import Anthropic\n\nclient = Anthropic(\n    base_url="https://api.apitoken.sale",\n    api_key="sk-pool-•••",\n)\nmsg = client.messages.create(\n    model="claude-opus-4-8",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Hello"}],\n)` },
      ] },
      { h2: "TypeScript", blocks: [
        { type: "code", code: `import Anthropic from "@anthropic-ai/sdk";\n\nconst client = new Anthropic({\n  baseURL: "https://api.apitoken.sale",\n  apiKey: "sk-pool-•••",\n});\nconst msg = await client.messages.create({\n  model: "claude-opus-4-8",\n  max_tokens: 1024,\n  messages: [{ role: "user", content: "Hello" }],\n});` },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "验证切换是否成功", blocks: [
        { type: "p", text: "改完 Base URL 后，发一次请求，确认你能收到正常的 Anthropic 响应。流式输出、工具调用和系统提示的表现都与 api.anthropic.com 完全一致——变的只有计费端点。" },
        { type: "list", items: [
          "返回 401 说明密钥或 Base URL 有误——两者都要重新检查。",
          "保持相同的模型 ID；消息相关的代码无需任何改动。",
          "在控制台按请求查看用量，确认消费和你的折扣。",
        ] },
      ] },
    ],
    faq: [
      { q: "我还能继续用官方 Anthropic SDK 吗？", a: "可以。把 base_url（Python）或 baseURL（TypeScript）设为 apitoken.sale，其余一切保持不变。" },
      { q: "模型 ID 会变吗？", a: "不会。继续使用相同的模型 ID，例如 claude-opus-4-8 和 claude-sonnet-5。" },
    ],
  },
  "apitoken-vs-anthropic-direct": {
    title: "apitoken.sale 对比 Anthropic 官方直购",
    h1: "apitoken.sale 对比直接向 Anthropic 购买",
    description: "对比 apitoken.sale 与 Anthropic 官方直购：完全相同的 Messages API 和模型，但最高立省 70%、无需账户、支持银行卡或加密货币支付。",
    keywords: ["claude api 对比 anthropic 官方", "apitoken 对比 anthropic", "anthropic api 替代", "比 anthropic api 更便宜", "claude api 转售"],
    dek: "apitoken.sale 并不是另一套 API——它就是同一套 Anthropic Messages API，从预付余额中以折扣价转售。下面说明真正改变了什么、又没有改变什么。",
    sections: [
      { h2: "保持不变的部分", blocks: [
        { type: "list", items: [
          "同一套 Anthropic Messages API、接口和流式输出。",
          "相同的模型 ID（claude-opus-4-8、claude-sonnet-5、claude-haiku-4-5）。",
          "与你代码已预期的相同的请求与响应格式。",
        ] },
      ] },
      { h2: "发生改变的部分", blocks: [
        { type: "list", items: [
          "价格：B2C 最高比官方消费低 70%。",
          "开通：无需 Anthropic 账户、排队或开票国家要求。",
          "支付：银行卡或加密货币。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "各自适合谁", blocks: [
        { type: "p", text: "如果你已经拥有顺畅的 Anthropic 开票和企业协议，直购或许适合你。如果你想用同样的模型但更便宜、更快上手，并且能用银行卡或加密货币付款，那么 apitoken.sale 是务实之选。" },
      ] },
    ],
    faq: [
      { q: "apitoken.sale 是真正的 Claude API 吗？", a: "是的——它提供同一套 Anthropic Messages API 和模型。只有定价和开通方式不同。" },
      { q: "为什么它比 Anthropic 官方直购更便宜？", a: "余额是预付且汇集的，并对官方消费套用最高 70% 的递进折扣。" },
    ],
  },
  "apitoken-vs-openrouter": {
    title: "Claude 场景下 apiToken.sale 与 OpenRouter 对比",
    h1: "Claude 场景下 apiToken.sale 与 OpenRouter 对比",
    description: "在选择 Claude 网关？对比 apiToken.sale 与 OpenRouter：原生 Anthropic 端点加预付折扣，对比多提供方路由器。",
    keywords: ["openrouter 替代品", "apitoken 对比 openrouter", "claude api 网关", "openrouter claude", "最佳 claude api 网关"],
    dek: "两者都能让你无需 Anthropic 账户就用上 Claude，但架构不同。如果 Claude 是你的主力模型，原生 Anthropic 端点会让一切更简单。",
    sections: [
      { h2: "原生 Anthropic 端点", blocks: [
        { type: "p", text: "apiToken.sale 在 https://api.apitoken.sale 上暴露标准的 Anthropic Messages API，因此 Claude Code、Cursor 和 Anthropic SDK 都无需任何适配器即可使用。你不必经过一层通用的多提供方抽象。" },
      ] },
      { h2: "是预付折扣，不是加价", blocks: [
        { type: "list", items: [
          "递进式 B2C 折扣，官方 Claude 消费最高立省 70%。",
          "一把密钥、一份余额，通用 Opus、Sonnet 和 Haiku。",
          "银行卡或加密货币充值，永不过期。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "分别适合什么时候用", blocks: [
        { type: "list", items: [
          "apiToken.sale——Claude 是你的主力模型，你想要一个带折扣的原生 Anthropic 端点。",
          "OpenRouter——你需要在一层抽象后路由到众多提供方。",
          "两者都能让你无需 Anthropic 账户即可开始；但只有 apiToken.sale 直接对 Claude 消费打折。",
        ] },
      ] },
    ],
    faq: [
      { q: "为什么要选 Claude 原生网关？", a: "如果 Claude 是你的主力模型，原生 Anthropic 端点意味着你现有的 Anthropic 工具和 SDK 无需改动即可使用。" },
      { q: "apiToken.sale 会加价吗？", a: "不会——它对官方 Claude 消费打折，而不是在标价之上加价。" },
    ],
  },
  "claude-opus-vs-sonnet": {
    title: "Claude Opus 对比 Sonnet——该用哪个",
    h1: "Claude Opus 对比 Sonnet：该用哪个模型",
    description: "Opus 还是 Sonnet？为编码和智能体挑选合适 Claude 模型的实用指南——并在一把 apitoken.sale 密钥和余额上同时使用两者。",
    keywords: ["claude opus 对比 sonnet", "该用哪个 claude 模型", "opus 还是 sonnet 编码", "最佳 claude 模型", "claude 模型对比"],
    dek: "Opus 和 Sonnet 解决不同的问题。选对模型是获得更好结果、少花 token 的最简单方式——而且你可以在一把密钥上同时保留两者。",
    sections: [
      { h2: "默认使用 Sonnet", blocks: [
        { type: "p", text: "Sonnet 5 和 Sonnet 4.6 能又快又省地处理绝大多数编码和智能体工作。从这里开始。" },
      ] },
      { h2: "遇到难题再升级到 Opus", blocks: [
        { type: "p", text: "在复杂重构、架构设计以及额外推理物有所值的长时间高风险会话中，就该选 Opus 4.8。" },
        { type: "note", text: "由于一把密钥同时覆盖两者，你可以为每项任务路由到合适的档位，而无需在多个提供方之间来回切换。" },
        { type: "table", headers: ["", "Claude Opus 4.8", "Claude Sonnet 5"], rows: [
          ["官方价格（输入 / 输出 / 1M）", "$5 / $25", "$3 / $15"],
          ["本站起价 −60%", "$2 / $10", "$1.20 / $6"],
          ["上下文窗口", "1M token", "1M token"],
          ["最适合", "高难推理、长程智能体运行", "日常编码与智能体"],
        ] },
        { type: "link", text: "比较所有 Claude 模型与价格", href: "/models" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "哪个更适合编码？", a: "Sonnet 是日常编码推荐的默认模型；复杂推理和长时间重构则使用 Opus。" },
      { q: "我能在一个账户上同时使用两者吗？", a: "可以。Opus、Sonnet 和 Haiku 都共用同一把密钥和预付余额。" },
    ],
  },
  "claude-api-pricing-explained": {
    title: "Claude API 定价详解",
    h1: "Claude API 定价如何运作",
    description: "了解 Claude API 定价：按 token 的输入与输出费率、提示缓存，以及 apitoken.sale 如何套用最高 70% 的递进折扣。",
    keywords: ["claude api 定价", "claude api 成本", "claude api 定价如何运作", "claude token 定价", "anthropic api 定价详解"],
    dek: "Claude 按 token 计费——输入和输出分别计价——对缓存内容有折扣。apitoken.sale 保持这些机制完全一致，并在其上叠加一层折扣。",
    sections: [
      { h2: "Token、输入与输出", blocks: [
        { type: "p", text: "每次请求都按输入 token（你的提示和上下文）和输出 token（模型的回复）计量。输出 token 通常比输入更贵，更大的模型每 token 成本更高。" },
      ] },
      { h2: "缓存与思考", blocks: [
        { type: "list", items: [
          "缓存写入和缓存读取分别计量，且缓存读取便宜得多。",
          "在重推理调用中，思考 token 计入输出。",
          "流式与非流式请求的计费方式相同。",
        ] },
      ] },
      { h2: "apitoken.sale 的折扣", blocks: [
        { type: "p", text: "每次调用先换算为官方 Anthropic 消费，再减去你的折扣：B2C 从 60% 折扣起步，随累计充值增长最高可达 70%。每次请求都在控制台中以 token 级别的明细可见。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "按模型划分的 Claude API token 价格", blocks: [
        { type: "p", text: "更大的模型每 token 更贵：Opus 是高端档，Sonnet 是均衡的默认选择，Haiku 最便宜。你的折扣适用于所有模型，因此排序不变，但每个价格都更低。" },
        { type: "table", headers: ["模型", "官方 输入 / 输出（$ / 1M）", "本站起价（−60%）", "最优（−70%）"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2 / $10", "$1.50 / $7.50"],
          ["Claude Opus 4.7", "$5 / $25", "$2 / $10", "$1.50 / $7.50"],
          ["Claude Sonnet 5", "$3 / $15", "$1.20 / $6", "$0.90 / $4.50"],
          ["Claude Sonnet 4.6", "$3 / $15", "$1.20 / $6", "$0.90 / $4.50"],
          ["Claude Haiku 4.5", "$1 / $5", "$0.40 / $2", "$0.30 / $1.50"],
        ] },
        { type: "link", text: "含缓存费率与上下文窗口的模型页面", href: "/models" },
      ] },
    ],
    faq: [
      { q: "Claude API 如何定价？", a: "按 token 计费，分为输入和输出，缓存读取另有更便宜的费率。更大的模型每 token 成本更高。" },
      { q: "折扣如何套用？", a: "先计算官方消费，再在扣减余额前减去你的 B2C 折扣（60% 直至 70%）。" },
      { q: "Claude API 的 token 如何计价？", a: "按 token 计费，分输入和输出，缓存读取更便宜。apiToken.sale 在官方 token 费率之上再套用你 60–70% 的折扣。" },
    ],
  },
  "save-tokens-on-claude-api": {
    title: "如何在 Claude API 上节省 token",
    h1: "如何在 Claude API 上节省 token",
    description: "通过提示缓存、为每项任务选对模型和精简上下文来削减 Claude API 成本。这些实用的省 token 技巧可与 apitoken.sale 折扣叠加。",
    keywords: ["节省 claude api token", "降低 claude api 成本", "claude 提示缓存", "claude api 优化", "降低 claude api 账单"],
    dek: "你的折扣降低了每 token 的单价；这些技巧降低了 token 的数量。二者叠加，会让账单大幅缩水。",
    sections: [
      { h2: "使用提示缓存", blocks: [
        { type: "p", text: "长而稳定的上下文——系统提示、大文件、工具定义——都应当缓存。缓存读取的成本仅为全新输入 token 的一小部分，因此重复的上下文变得廉价。" },
      ] },
      { h2: "选对模型", blocks: [
        { type: "p", text: "不要把每个请求都发给 Opus。把廉价或高并发的工作路由给 Haiku，让日常编码留在 Sonnet 上，把 Opus 留给真正高难度的推理。" },
      ] },
      { h2: "精简上下文", blocks: [
        { type: "list", items: [
          "只发送任务真正需要的文件和历史。",
          "对长会话做摘要，而非完整重发。",
          "把 max_tokens 限制在响应真正需要的范围内。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "最能省 token 的单项措施是什么？", a: "对大而重复的上下文使用提示缓存，再配合选择能胜任任务的最便宜模型。" },
      { q: "这些技巧能与折扣叠加吗？", a: "能。折扣降低每 token 单价；这些技巧降低 token 数量，因此节省会相乘放大。" },
    ],
  },
  "how-billing-works": {
    title: "apitoken.sale 的计费如何运作",
    h1: "计费如何运作",
    description: "了解 apitoken.sale 的计费：预付余额、按官方费率的逐次请求计量、你的递进折扣，以及控制台中的 token 级用量。",
    keywords: ["claude api 计费", "apitoken 计费如何运作", "预付 claude api", "claude api 用量追踪", "claude api 余额"],
    dek: "计费是预付且透明的。你充入一份余额，每次请求按官方消费减去你的折扣扣减，并提供可供你审计的完整明细。",
    sections: [
      { h2: "预付余额", blocks: [
        { type: "p", text: "你充值任意整数美元金额。余额永不过期，也没有订阅，因此闲置时间不花一分钱。" },
      ] },
      { h2: "逐次请求计量", blocks: [
        { type: "list", items: [
          "每次调用按 token 换算为官方 Anthropic 消费。",
          "减去你当前的折扣（B2C 为 60% 直至 70%）。",
          "净额从你的预付余额中扣除。",
        ] },
      ] },
      { h2: "完全可见", blocks: [
        { type: "p", text: "每次请求都在控制台中显示，含输入、输出、缓存和思考 token，因此你始终清楚余额去向。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "计费是预付还是后付？", a: "预付。你预先充入一份余额，请求从中扣减；没有月度账单。" },
      { q: "我能看到 token 级用量吗？", a: "可以。控制台会把每次请求按输入、输出、缓存读/写和思考 token 分解显示。" },
    ],
  },
  "claude-api-activation-time": {
    title: "Claude API 激活有多快？",
    h1: "你的 Claude API 密钥激活有多快",
    description: "apitoken.sale 的密钥即时激活。生成密钥、充值，几分钟内即可成功发出 Claude API 调用——无需人工审核或排队。",
    keywords: ["claude api 激活时间", "claude api 密钥多快", "即时 claude api 密钥", "claude api 就绪时间"],
    dek: "从创建密钥到使用它之间没有任何等待期。激活是即时的，速度唯一的限制就是你把密钥粘贴进工具有多快。",
    sections: [
      { h2: "为即时而设计", blocks: [
        { type: "p", text: "密钥一经生成即刻可用。充值在支付确认后立即入账，而银行卡支付几秒内即可确认。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "什么可能带来短暂延迟", blocks: [
        { type: "p", text: "唯一的等待是支付确认。银行卡充值几秒内到账；加密货币充值则在网络确认交易后入账，具体时间取决于你选择的币种和手续费。" },
        { type: "list", items: [
          "密钥生成：即时。",
          "银行卡充值：几秒。",
          "加密货币充值：网络确认之后。",
        ] },
      ] },
    ],
    faq: [
      { q: "我的密钥多久能用？", a: "立即可用。没有人工审核——刚生成的密钥在下一次请求即可使用。" },
      { q: "充值需要多久？", a: "银行卡支付几秒内到账；加密货币在网络确认交易后入账。" },
    ],
  },
  "claude-api-supported-countries": {
    title: "Claude API 支持的国家/地区",
    h1: "你可以在哪里使用 apitoken.sale",
    description: "apitoken.sale 全球可用，无 Anthropic 计费国家要求。用银行卡或加密货币支付，即可在 Anthropic 不直接服务的地区使用 Claude API。",
    keywords: ["claude api 支持的国家", "claude api 全球可用", "anthropic api 国家限制", "claude api 可用地区"],
    dek: "由于我们自行签发密钥和余额，因此没有 Anthropic 计费国家的门槛。这让身处直接注册困难地区的开发者也能用上 Claude API。",
    sections: [
      { h2: "无计费国家门槛", blocks: [
        { type: "list", items: [
          "无需 Anthropic 账户或受支持的计费国家。",
          "支持银行卡和加密货币支付。",
          "通过 Telegram 提供英语和俄语支持。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "跨地区的支付方式", blocks: [
        { type: "p", text: "由于我们签发密钥和余额，你不受 Anthropic 支持的计费国家约束。在可用的地区用银行卡支付，或在银行卡被拒的地区用加密货币支付。" },
        { type: "list", items: [
          "无需 Anthropic 计费国家。",
          "结账时可用银行卡或加密货币。",
          "通过 Telegram 提供英语和俄语支持。",
        ] },
      ] },
    ],
    faq: [
      { q: "我所在的国家能用 Claude API 吗？", a: "apitoken.sale 没有计费国家要求，因此你可以在 Anthropic 不直接计费的地区购买余额并使用密钥。" },
      { q: "支付限制怎么办？", a: "你可以用银行卡或加密货币支付，这在银行卡不可用的地区很有帮助。" },
    ],
  },
  "claude-api-refund-policy": {
    title: "Claude API 退款政策",
    h1: "退款与支持",
    description: "了解 apitoken.sale 如何处理余额、退款和支持。预付余额永不过期，并通过 Telegram 提供英语和俄语支持。",
    keywords: ["claude api 退款", "apitoken 退款政策", "claude api 支持", "claude api 退钱", "claude api 帮助"],
    dek: "预付余额的设计就是为了低风险：它永不过期，你只为实际调用的部分付费，而支持只需一条消息即可触达。",
    sections: [
      { h2: "余额与退款", blocks: [
        { type: "p", text: "由于余额为预付制且永不过期，未使用的资金会一直保留供日后使用。退款通过原支付渠道处理；请带上你的账户信息联系支持。" },
      ] },
      { h2: "获取帮助", blocks: [
        { type: "p", text: "支持通过 Telegram 提供英语和俄语服务，也可发邮件至 apitokensale@gmail.com。大多数集成问题都能很快得到解答。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "充值与余额如何运作", blocks: [
        { type: "p", text: "你以任意整数美元金额充值余额，且仅在请求运行时才扣减。由于它永不过期，没什么理由超额充值——用多少充多少即可。" },
        { type: "list", items: [
          "预付、永不过期的余额。",
          "退款通过原支付渠道处理。",
          "用你的账户邮箱联系支持以获取帮助。",
        ] },
      ] },
    ],
    faq: [
      { q: "我的余额会过期吗？", a: "不会。预付余额永不过期，仅在真实 API 使用时才消耗。" },
      { q: "我该如何联系支持？", a: "通过 Telegram 以英语或俄语联系支持，或发邮件至 apitokensale@gmail.com。" },
    ],
  },
  "apitoken-vs-proxyapi": {
    title: "Claude 场景下 apiToken.sale 与 ProxyAPI 对比",
    h1: "apiToken.sale 与 ProxyAPI 对比",
    description: "对比 Claude API 转售商：apiToken.sale 提供原生 Anthropic 端点、60–70% 的递进折扣、银行卡或加密货币支付，一把密钥通用所有模型。",
    keywords: ["proxyapi 替代品", "apitoken 对比 proxyapi", "claude api 转售", "proxyapi claude", "不用 proxyapi 用 claude api"],
    dek: "两者都能让你无需 Anthropic 账户就用上 Claude。差别在于付款方式、能省多少，以及端点是否真正 Anthropic 原生。",
    sections: [
      { h2: "原生 Anthropic 端点", blocks: [
        { type: "p", text: "apiToken.sale 在 https://api.apitoken.sale 上暴露标准的 Anthropic Messages API，因此 Claude Code、Cursor 和 Anthropic SDK 无需改动即可使用——你与 Claude 之间没有一层适配层。" },
      ] },
      { h2: "是折扣，不是加价", blocks: [
        { type: "list", items: [
          "递进式 B2C 折扣，官方 Claude 消费最高立省 70%。",
          "一把预付密钥、一份余额，通用 Opus、Sonnet 和 Haiku。",
          "银行卡或加密货币充值，永不过期。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "分别适合什么", blocks: [
        { type: "list", items: [
          "apiToken.sale——带递进折扣、密钥终身累计消费上限和可选到期日期的原生 Anthropic 端点。",
          "通用转售商——如果你已经在用它的其他提供方，可能适合你。",
          "两者都移除了 Anthropic 账户门槛；差别在于价格，以及 Claude 接入有多原生。",
        ] },
      ] },
    ],
    faq: [
      { q: "apiToken.sale 比普通转售商更便宜吗？", a: "它对官方 Claude 消费套用最高 70% 的递进折扣，而不是在标价之上再加价。" },
      { q: "我的 Anthropic 工具还能用吗？", a: "能——它是原生的 Anthropic Messages API，因此 Claude Code、Cursor 和 SDK 只需改一下 Base URL。" },
    ],
  },
  "apitoken-vs-portkey": {
    title: "apiToken.sale 与 Portkey 对比（Claude）",
    h1: "apiToken.sale 与 Portkey 对比",
    description: "Portkey 是一款使用你自有厂商密钥进行路由与可观测的 AI 网关。apiToken.sale 则直接提供折扣价的 Claude 密钥和余额。两者分别在什么时候用，看这篇。",
    keywords: ["portkey 替代方案", "ai 网关 claude", "claude api 网关", "portkey claude api", "claude 密钥折扣"],
    dek: "这两款工具解决的是不同的问题。Portkey 位于你已拥有的厂商密钥之前；而 apiToken.sale 正是折扣 Claude 密钥和余额的来源。",
    sections: [
      { h2: "各司其职", blocks: [
        { type: "p", text: "Portkey 在你自带的 API 密钥之上增加路由、缓存和可观测能力。它并不向你出售 Claude 权限或折扣——背后你仍需一个已充值的 Anthropic 账户。" },
        { type: "p", text: "apitoken.sale 才是密钥和余额的来源：一个位于 https://api.apitoken.sale 的原生 Anthropic 端点，最高立省 70%，且无需 Anthropic 账户。" },
      ] },
      { h2: "两者甚至可以组合", blocks: [
        { type: "p", text: "如果你喜欢 Portkey 的可观测能力，可以把 apiToken.sale 密钥设为它的 Anthropic 厂商，从而在底层享受折扣。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "Portkey 会给我 Claude 折扣吗？", a: "不会——Portkey 只是覆盖在你自有密钥之上的网关。折扣 Claude 密钥和余额由 apiToken.sale 提供。" },
      { q: "两者能一起用吗？", a: "能。把 apiToken.sale 密钥作为 Portkey 的 Anthropic 厂商，既保留可观测能力又能少花钱。" },
    ],
  },
  "apitoken-vs-litellm": {
    title: "Claude 场景下 apiToken.sale 与 LiteLLM 对比",
    h1: "apiToken.sale 与 LiteLLM 对比",
    description: "LiteLLM 是一个统一各模型 API 的自托管代理，但需要你自己充值的密钥。apiToken.sale 则是一个托管的折扣 Claude 端点，无需自行运维。",
    keywords: ["litellm 替代品", "apitoken 对比 litellm", "litellm claude", "自托管 claude 代理", "托管 claude api"],
    dek: "如果你想跨多个提供方自托管一个代理，LiteLLM 很棒。apiToken.sale 是相反的取舍：无需运维，而且 Claude 余额自带折扣。",
    sections: [
      { h2: "自托管 vs 托管", blocks: [
        { type: "list", items: [
          "LiteLLM：你自己运行和维护代理，并且仍要自行为每个提供方充值。",
          "apiToken.sale：完全托管的原生 Anthropic 端点，无需管理任何基础设施。",
          "apiToken.sale 对 Claude 消费提供 60–70% 的折扣，这是裸代理做不到的。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "分别适合什么时候用", blocks: [
        { type: "list", items: [
          "apiToken.sale——你想要一个托管、带折扣、无需运维的 Claude 端点。",
          "LiteLLM——你想自托管一个跨多个自付费提供方的统一代理。",
          "你甚至可以把 LiteLLM 放在 apiToken.sale 密钥前面，在底层保留折扣。",
        ] },
      ] },
    ],
    faq: [
      { q: "LiteLLM 会给 Claude 打折吗？", a: "不会。LiteLLM 路由到你自己充值的提供方；折扣来自 apiToken.sale 汇集的预付余额。" },
      { q: "用 apiToken.sale 需要自己托管东西吗？", a: "不需要——它是托管端点。你只需改一下 Base URL 和密钥。" },
    ],
  },
  "best-claude-model-for-coding": {
    title: "最适合编程的 Claude 模型",
    h1: "最适合编程的 Claude 模型",
    description: "编程该用哪个 Claude 模型？一份按任务挑选 Opus、Sonnet 或 Haiku 的实用指南——所有型号都在一把 apiToken.sale 密钥上。",
    keywords: ["最适合编程的 claude 模型", "claude 编程模型", "opus 和 sonnet 编程对比", "claude 写代码用哪个", "哪个 claude 适合写代码"],
    dek: "最佳模型取决于任务。让模型匹配任务，就能用更少的 token 得到更好的产出——而且每一档模型都在同一把密钥上。",
    sections: [
      { h2: "日常编程用 Sonnet", blocks: [
        { type: "p", text: "Claude Sonnet 5 和 Sonnet 4.6 是交互式编码和智能体循环的默认之选：快速、能干且高性价比。大多数工作从这里开始。" },
      ] },
      { h2: "高难度问题用 Opus", blocks: [
        { type: "p", text: "在复杂重构、架构设计以及需要额外推理才划算的漫长高风险会话中，使用 Claude Opus 4.8。" },
      ] },
      { h2: "大批量用 Haiku", blocks: [
        { type: "p", text: "Claude Haiku 4.5 擅长快速、廉价、大批量的任务——代码检查、信息抽取、快速编辑——帮你撑长余额。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "最适合编程的 Claude 模型是哪个？", a: "日常编码用 Sonnet，复杂推理和重构用 Opus，快速大批量任务用 Haiku——全部在一把 apiToken.sale 密钥上。" },
      { q: "能按请求切换模型吗？", a: "能。一把密钥和余额覆盖所有模型，你可以把每个请求路由到性价比最高的那一档。" },
    ],
  },
  "claude-max-plan-vs-api": {
    title: "Claude Max 订阅与 Claude API 对比",
    h1: "Claude Max 订阅与 API 对比",
    description: "何时该用 Claude 订阅、何时该用 Claude API。apiToken.sale 提供按量付费的全模型 API 权限，无月费，最高立省 70%。",
    keywords: ["claude max 订阅", "claude 订阅还是 api", "claude max 对比 api", "claude api 按量付费", "claude 免订阅"],
    dek: "固定的 Claude 订阅和按量付费的 API 计费适合不同的使用场景。对于程序化和突发式的使用，预付余额上的 API 通常更划算。",
    sections: [
      { h2: "订阅 vs 按 token 计费", blocks: [
        { type: "p", text: "对于单一应用内稳定、重度的交互式使用，固定月费套餐说得通。但对于突发式使用它就很浪费，而且它并不给你一把可编程、可接入自有工具的 API 密钥。" },
      ] },
      { h2: "为什么 API 往往更胜一筹", blocks: [
        { type: "list", items: [
          "只为实际用掉的 token 付费——没有月度保底。",
          "一把密钥驱动 Claude Code、Cursor、智能体和生产环境调用。",
          "apiToken.sale 在官方 token 费率上再享最高 70% 折扣。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "API 比 Claude 订阅更便宜吗？", a: "对于突发式或程序化的使用，按量付费的 API 计费能避免为闲置时间支付固定月费，而 apiToken.sale 还会进一步打折。" },
      { q: "能在编码工具里用 API 吗？", a: "能——API 密钥可用于 Claude Code、Cursor、VS Code 智能体和各 SDK，这些是订阅所不提供的。" },
    ],
  },
  "claude-3-5-vs-claude-4": {
    title: "Claude 3.5 与 Claude 4 对比——有何变化",
    h1: "Claude 3.5 与 Claude 4：有何变化",
    description: "从 Claude 3.5 迁移到当前的 Claude 4 系列？看看有哪些提升、更新后的模型 ID，以及如何在 apiToken.sale 上只改一处 base URL 就完成切换。",
    keywords: ["claude 3.5 对比 4", "claude 4 对比 3.5", "claude 模型迁移", "升级 claude 模型", "新版 claude 模型"],
    dek: "当前的 Claude 系列在推理和编码上相比 3.5 有明显提升。迁移基本上就是换一个模型 ID——其余一切照旧。",
    sections: [
      { h2: "有哪些提升", blocks: [
        { type: "p", text: "Opus、Sonnet 和 Haiku 4 系列模型在智能体编码、长上下文一致性和复杂推理方面相比 3.5 有所改进，同时沿用同一套 Messages API。" },
      ] },
      { h2: "如何迁移", blocks: [
        { type: "p", text: "把模型 ID 换成当前的某一个——例如 claude-opus-4-8、claude-sonnet-5 或 claude-haiku-4-5——并保留你现有的请求代码。在 apiToken.sale 上，密钥和端点都不变。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "Claude 4 比 3.5 强很多吗？", a: "是的，尤其在编码、智能体和长上下文任务上，同时使用相同的 API 格式。" },
      { q: "迁移难吗？", a: "不难——更新模型 ID（例如换成 claude-sonnet-5），你现有的 Messages API 代码即可继续工作。" },
    ],
  },
  "why-choose-apitoken": {
    title: "为什么选择 apiToken.sale",
    h1: "为什么选择 apiToken.sale",
    description: "开发者选择 apiToken.sale 使用 Claude 的理由：同一套 Anthropic API 最高便宜 70%，无需 Anthropic 账户即时开通，支持银行卡或加密货币支付。",
    keywords: ["为什么选 apitoken.sale", "最佳 claude api 服务商", "claude api 折扣服务商", "便宜的 claude api 网关", "claude api 无需 anthropic 账号"],
    dek: "apiToken.sale 只为一件事而生：同一套 Claude API，更便宜、更好上手。以下是它在实践中的意义。",
    sections: [
      { h2: "一句话版本", blocks: [
        { type: "list", items: [
          "一模一样的 Anthropic Messages API 和所有当前的 Claude 模型。",
          "在永不过期的预付余额上，官方消费最高立省 70%。",
          "即时、自助开通——无需 Anthropic 账户、无需排队、不限计费国家。",
          "支持银行卡或加密货币付款。",
          "每把密钥可选终身累计消费上限和到期日期，并在控制台查看 token 级用量明细。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "同一余额上的折扣 Claude API token", blocks: [
        { type: "p", text: "把它当作打折出售的 Claude API token：你一次性充值余额，享受官方 token 费率最高 70% 的折扣，并在所有模型和工具上使用。余额永不过期，也没有订阅。" },
      ] },
    ],
    faq: [
      { q: "apiToken.sale 有什么不同？", a: "它是同一套 Claude API，最高便宜 70%，即时开通、无需 Anthropic 账户，支持银行卡或加密货币付款。" },
      { q: "API 有任何改动吗？", a: "没有——协议、模型和响应都是标准的 Anthropic。只有价格和开通方式不同。" },
      { q: "apiToken.sale 是什么？", a: "一个独立的 Claude API 网关，以折扣价预付出售官方 Anthropic API 的访问权——同样的模型，最高便宜 70%，无需 Anthropic 账户。" },
    ],
  },
  "claude-api-gateway": {
    title: "什么是 Claude API 网关？",
    h1: "Claude API 网关是什么",
    description: "Claude API 网关位于你的工具和 Anthropic 之间，增加接入、计费和管控能力。apitoken.sale 是一个带 60–70% 折扣的原生网关。",
    keywords: ["claude api 网关", "什么是 api 网关", "anthropic 网关", "claude 代理", "claude api 接入层"],
    dek: "网关是介于你的代码和模型提供方之间的一层薄薄的中间层。好的 Claude 网关对你的工具是透明的，同时改善接入、价格和管控。",
    sections: [
      { h2: "网关做什么", blocks: [
        { type: "list", items: [
          "对外呈现标准的 Anthropic Messages API，让工具无需改动即可使用。",
          "处理接入和计费——在这里，就是折扣预付余额。",
          "增加按密钥的终身累计消费上限、可选到期日期和用量可见性。",
        ] },
      ] },
      { h2: "原生，而非转译层", blocks: [
        { type: "p", text: "apiToken.sale 是 Anthropic 原生的：把任意客户端指向 https://api.apitoken.sale/v1/messages，它的表现与 api.anthropic.com 完全一致——再加上你的折扣和控制台管控。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "挑选网关时看什么", blocks: [
        { type: "list", items: [
          "原生 Anthropic API，让工具和 SDK 无需改动即可使用。",
          "透明的按 token 计费，可在控制台审计。",
          "按密钥的管控：可选的终身累计消费上限和到期日期。",
          "无绑定——预付余额永不过期。",
        ] },
      ] },
    ],
    faq: [
      { q: "网关会改变 API 吗？", a: "不会。原生 Claude 网关讲的是标准的 Anthropic Messages API，因此你的工具和 SDK 无需改动。" },
      { q: "为什么用网关而不直接用 Anthropic？", a: "为了折扣、无需 Anthropic 账户即可即时开通，以及为单独密钥设置可选的终身累计消费上限和到期日期。" },
    ],
  },
  "claude-api-rate-limits": {
    title: "Claude API 速率限制",
    h1: "理解 Claude API 速率限制",
    description: "apiToken.sale 上的 429 意味着什么、如何通过 Retry-After 与退避处理，以及密钥消费护栏与吞吐限制有何不同。",
    keywords: ["claude api 速率限制", "claude api 429", "anthropic 限流", "claude api 吞吐", "claude api 重试"],
    dek: "速率限制让网关保持稳定、让你的余额更安全。妥善处理它意味着工具更顺滑、不浪费开销。",
    sections: [
      { h2: "流量限制与消费护栏", blocks: [
        { type: "p", text: "apiToken.sale 不公布固定的 RPM 表。429 可能表示网关或上游容量限制。控制台不能配置请求吞吐；可用的按密钥护栏是可选的终身累计消费上限和到期日期。" },
      ] },
      { h2: "处理 429", blocks: [
        { type: "list", items: [
          "遵守 Retry-After 响应头并采用指数退避。",
          "降低并发，而不是猛冲端点。",
          "若需持续更高的吞吐，请联系支持。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "Claude API 的速率限制是多少？", a: "apiToken.sale 不公布固定的 RPM 数字。遇到 429 时请遵守 Retry-After、进行退避并降低并发；如需持续更高的吞吐，请联系支持。" },
      { q: "遇到 429 该怎么办？", a: "遵守 Retry-After、进行退避并降低并发；如需持续更高的限额请联系支持。" },
    ],
  },
  "claude-api-streaming": {
    title: "使用 Claude API 进行流式输出",
    h1: "从 Claude API 流式获取响应",
    description: "如何在 apitoken.sale 上流式获取 Claude 响应，让编码智能体和界面更灵敏。与 Anthropic SSE 格式相同，计费方式与非流式一致。",
    keywords: ["claude api 流式", "claude sse", "流式获取 claude 响应", "anthropic 流式 api", "claude api 实时"],
    dek: "流式输出会在 token 生成时即刻发送，让智能体和聊天界面感觉即时响应。apitoken.sale 支持标准的 Anthropic 流式格式。",
    sections: [
      { h2: "如何流式输出", blocks: [
        { type: "p", text: "在请求中设置 \"stream\": true（或使用 SDK 的流式辅助方法）。网关会返回标准的 Anthropic 服务器发送事件（SSE）。" },
        { type: "code", code: `curl https://api.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "stream": true,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
      ] },
      { h2: "计费完全相同", blocks: [
        { type: "p", text: "流式与非流式请求的计费方式相同——都按输入和输出 token 计费——因此流式输出不会让你多花一分钱。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "什么时候值得用流式", blocks: [
        { type: "list", items: [
          "聊天和编码界面，用户会看着答案逐字出现。",
          "长生成任务，可以尽早渲染或处理部分输出。",
          "一旦发出工具调用就停止的智能体。",
        ] },
        { type: "p", text: "对于简短的批处理任务，非流式更简单；无论哪种方式，成本都一样。" },
      ] },
    ],
    faq: [
      { q: "apitoken.sale 支持流式输出吗？", a: "支持——标准的 Anthropic SSE 流式格式适用于编码智能体、IDE 和生产调用。" },
      { q: "流式输出会更贵吗？", a: "不会。流式与非流式请求按 token 计费的方式完全相同。" },
    ],
  },
  "claude-api-prompt-caching": {
    title: "Claude API 上的提示缓存",
    h1: "用 Claude 提示缓存削减成本",
    description: "提示缓存让 Claude API 上重复的上下文便宜得多。它在 apiToken.sale 上如何运作、何时使用，以及如何与你的折扣叠加。",
    keywords: ["claude 提示缓存", "claude api 缓存", "anthropic prompt cache", "缓存降低 claude 成本", "claude 缓存读取"],
    dek: "如果你反复发送同样的大段上下文——系统提示、文件、工具定义——缓存会把这些 token 从昂贵变成近乎免费。",
    sections: [
      { h2: "缓存如何省钱", blocks: [
        { type: "p", text: "缓存写入和缓存读取分别计量，而缓存读取只是全新输入 token 价格的一小部分。稳定、复用的上下文是理想的缓存对象。" },
      ] },
      { h2: "它可与你的折扣叠加", blocks: [
        { type: "p", text: "缓存降低 token 数量；你的 apiToken.sale 折扣降低每 token 单价。两者叠加，账单大幅缩水，而且每一条缓存行都会显示在你的用量明细中。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "提示缓存能省多少？", a: "缓存读取只是全新输入 token 价格的一小部分，因此重复的大段上下文会便宜得多。" },
      { q: "缓存能配合折扣一起用吗？", a: "能——缓存降低 token 数量、折扣降低每 token 单价，因此节省效果相乘。" },
    ],
  },
  "claude-api-best-practices": {
    title: "Claude API 最佳实践",
    h1: "Claude API 最佳实践",
    description: "在 apitoken.sale 上使用 Claude API 的实用最佳实践：模型选择、提示缓存、流式输出、密钥终身累计消费上限、到期日期，以及安全处理密钥。",
    keywords: ["claude api 最佳实践", "claude api 技巧", "claude api 生产环境", "claude api 使用规范", "anthropic api 最佳实践"],
    dek: "一份简短的清单，帮你在生产环境中从 Claude API 获得可靠又经济的结果。",
    sections: [
      { h2: "清单", blocks: [
        { type: "list", items: [
          "为每项任务挑选能胜任的最便宜模型；仅在需要时升级。",
          "缓存大而稳定的上下文，以大幅削减输入成本。",
          "为灵敏的智能体和界面使用流式响应。",
          "为每把密钥设置可选的终身累计消费上限和到期日期。",
          "用 Retry-After 和退避处理 429。",
          "关注 token 级用量明细，尽早发现浪费。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "把成本和可靠性控制住", blocks: [
        { type: "list", items: [
          "把 max_tokens 限制在每次响应实际所需的范围。",
          "对 429/5xx 采用指数退避重试，而非紧密循环。",
          "为不同环境使用名称清晰的单独密钥，泄露时无需更换所有客户端的密钥。",
          "每周复查 token 级用量，尽早发现回退。",
        ] },
      ] },
    ],
    faq: [
      { q: "最有效的最佳实践是什么？", a: "让模型与任务匹配，并缓存重复的上下文——两者结合最能削减成本。" },
      { q: "如何保护密钥安全？", a: "将密钥存入密钥管理器，设置合适的终身累计消费上限和到期日期，并立即吊销已暴露的密钥。" },
    ],
  },
  "claude-code-api-key": {
    title: "用 API 密钥配置 Claude Code",
    h1: "用 apitoken.sale 密钥运行 Claude Code",
    description: "只需两个环境变量即可为 Claude Code 配置 apitoken.sale 密钥，用预付余额以最高 70% 折扣运行所有 Claude 模型。",
    keywords: ["claude code api 密钥", "claude code 配置", "claude code anthropic base url", "claude code 自定义密钥", "低成本运行 claude code"],
    dek: "Claude Code 读取两个环境变量。把它们指向 apitoken.sale，即可保留全部功能，同时按折扣预付余额计费。",
    sections: [
      { h2: "两个变量", blocks: [
        { type: "code", code: `export ANTHROPIC_BASE_URL=https://api.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••\n\n# then just run\nclaude` },
        { type: "p", text: "整个配置就这么简单。高难度工作用 claude-opus-4-8，日常编码用 claude-sonnet-5。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "验证并选择模型", blocks: [
        { type: "p", text: "先跑一个简短的提示确认密钥可用，然后设置你的默认模型。如果 Claude Code 报鉴权错误，请重新检查两个环境变量，并重启 shell 以确保它们已导出。" },
        { type: "list", items: [
          "日常编码：claude-sonnet-5。",
          "高难度重构和漫长会话：claude-opus-4-8。",
          "在控制台按请求查看 token 用量，以追踪消费。",
        ] },
      ] },
    ],
    faq: [
      { q: "如何把 Claude Code 指向 apitoken.sale？", a: "将 ANTHROPIC_BASE_URL 和 ANTHROPIC_API_KEY 设为你的 apitoken.sale 端点和密钥，然后运行 claude。" },
      { q: "Claude Code 的所有功能都能保留吗？", a: "能——只有计费方式改变，从订阅制变为折扣预付用量。" },
    ],
  },
  "vscode-ai-agents-one-prompt": {
    title: "在 VS Code 中用 Claude 运行免费 AI 智能体",
    h1: "用 Claude 运行免费的 VS Code AI 智能体",
    description: "用 apitoken.sale 的 Claude 密钥配置 Cline、Roo Code 等免费 VS Code 智能体——无需 Cursor Pro。一个端点，通用所有 Claude 模型，还享折扣。",
    keywords: ["免费 vscode ai 智能体", "cline roo code claude", "vscode claude 智能体", "免费的 cursor 替代品", "不用 cursor 在 vscode 用 claude"],
    dek: "无需 Cursor Pro 也能拥有智能体编码。免费的 VS Code 智能体接受任何兼容 Anthropic 的密钥，因此 Claude 可以用折扣余额在 VS Code 中运行。",
    sections: [
      { h2: "把智能体指向 Claude", blocks: [
        { type: "steps", items: [
          "安装一个免费的智能体扩展，例如 Cline 或 Roo Code。",
          "选择 Anthropic 作为 API 提供方。",
          "把 Base URL 设为 https://api.apitoken.sale，粘贴你的 sk-pool-••• 密钥，并选择一个模型，例如 claude-sonnet-5。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "为每项任务选对模型", blocks: [
        { type: "list", items: [
          "claude-sonnet-5——日常编码和智能体循环的默认之选。",
          "claude-opus-4-8——复杂重构、架构设计和漫长会话。",
          "claude-haiku-4-5——快速、廉价的编辑和高吞吐步骤。",
        ] },
        { type: "p", text: "由于一把密钥通用所有模型，你可以在扩展里按任务随时切换，无需更换账户或计费方式。" },
      ] },
    ],
    faq: [
      { q: "做 AI 编码需要 Cursor Pro 吗？", a: "不需要。Cline、Roo Code 等免费 VS Code 智能体都可搭配 apitoken.sale 的 Claude 密钥使用。" },
      { q: "我该选哪个模型？", a: "日常编码用 claude-sonnet-5；复杂任务用 claude-opus-4-8。" },
    ],
  },
  "claude-api-key-security": {
    title: "保护你的 Claude API 密钥",
    h1: "让你的 Claude API 密钥保持安全",
    description: "如何在 apiToken.sale 上保护 Claude API 密钥：终身累计消费上限、可选到期日期、名称清晰的单独密钥、及时吊销和安全存储。",
    keywords: ["claude api 密钥安全", "保护 api 密钥", "轮换 claude api 密钥", "claude api 密钥管理", "anthropic 密钥安全"],
    dek: "你的密钥会花掉真实余额，所以要把它当作凭据对待。apiToken.sale 提供多种管控，在密钥万一泄露时限制影响范围。",
    sections: [
      { h2: "限制风险的管控", blocks: [
        { type: "list", items: [
          "为密钥设置终身累计消费上限。",
          "如果临时访问应自动结束，请选择到期日期。",
          "为每个工具或环境签发名称清晰的单独密钥。",
          "要更换密钥，请先创建新密钥、更新客户端，再吊销旧密钥。",
        ] },
      ] },
      { h2: "基本卫生习惯", blocks: [
        { type: "list", items: [
          "绝不把密钥提交到 git 或粘贴到聊天中。",
          "把密钥存放在环境变量或密钥管理器中。",
          "一旦密钥暴露，立即吊销并轮换。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "密钥泄露时如何把损失降到最低？", a: "使用终身累计消费上限和到期日期，为不同客户端保留名称清晰的单独密钥，并立即吊销已暴露的密钥。" },
      { q: "密钥应该存在哪里？", a: "存在环境变量或密钥管理器中——绝不提交到 git 或在聊天中分享。" },
    ],
  },
  "claude-api-for-ai-agents": {
    title: "面向 AI 智能体的 Claude API",
    h1: "将 Claude API 用于 AI 智能体",
    description: "用 apitoken.sale 在 Claude API 上构建 AI 智能体：一把密钥通用所有模型，配合流式输出、工具调用、提示缓存和密钥终身累计消费上限，控制长时间运行的成本。",
    keywords: ["claude api 智能体", "claude ai 智能体 api", "claude 工具调用", "claude 智能体框架", "claude api 自动化"],
    dek: "智能体工作负载既耗 token 又长时间运行，这让模型选择、缓存和成本控制变得最为关键。以下是 apitoken.sale 如何契合智能体。",
    sections: [
      { h2: "智能体需要什么", blocks: [
        { type: "list", items: [
          "流式输出和工具调用——两者都是 Anthropic Messages API 的标准能力。",
          "模型路由：Haiku 处理廉价步骤，Sonnet 负责推理，Opus 应对最难的任务。",
          "为重复的系统提示和工具定义使用提示缓存。",
          "密钥终身累计消费上限，让失控循环无法花费超过该密钥的上限。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获价值 $10 的 Claude 官方价格用量；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "一个成本感知的智能体循环", blocks: [
        { type: "p", text: "一个实用的模式：把规划和推理路由到 Sonnet，把廉价的子步骤和解析交给 Haiku，仅将最难的调用升级到 Opus。缓存系统提示和工具定义，让重复上下文几乎免费。" },
        { type: "list", items: [
          "设置密钥终身累计消费上限，让失控循环无法花费超过上限。",
          "使用流式输出，让智能体能够基于部分输出行动。",
          "关注 token 用量，以调优哪些步骤用哪个模型。",
        ] },
      ] },
    ],
    faq: [
      { q: "Claude API 适合做智能体吗？", a: "适合——具备流式输出、工具调用、模型路由和提示缓存，全都在一把 apitoken.sale 密钥下，并带消费管控。" },
      { q: "如何压低智能体成本？", a: "把廉价步骤路由到 Haiku，缓存重复上下文，并为智能体密钥设置终身累计消费上限。" },
    ],
  },
  "claude-api-langchain": {
    title: "在 LangChain 中使用 Claude API",
    h1: "在 LangChain 中使用 Claude API",
    description: "通过 apitoken.sale 将 LangChain 接入 Claude：把 ChatAnthropic 指向 api.apitoken.sale，模型 ID 保持不变，每 token 费用降低 60–70%。",
    keywords: ["claude api langchain", "langchain anthropic", "langchain claude", "chatanthropic base url", "langchain claude api 密钥"],
    dek: "LangChain 的 Anthropic 集成支持自定义 API URL，因此只改两行，你的链和智能体就能通过 apitoken.sale 运行 Claude——同样的模型，更低的 token 单价。",
    sections: [
      { h2: "把 ChatAnthropic 指向网关", blocks: [
        { type: "code", code: `from langchain_anthropic import ChatAnthropic\n\nllm = ChatAnthropic(\n    model="claude-opus-4-8",\n    anthropic_api_url="https://api.apitoken.sale",\n    anthropic_api_key="sk-pool-•••",\n)\nprint(llm.invoke("Hello").content)` },
        { type: "p", text: "整个集成就是这些：同一个 langchain-anthropic 包、同样的模型 ID、同样的流式输出与工具调用——变的只有端点和价格。" },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
      { h2: "或通过环境变量配置", blocks: [
        { type: "code", code: `export ANTHROPIC_API_URL=https://api.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••` },
        { type: "p", text: "设置好环境变量后，ChatAnthropic 会自动读取这两个值，共享代码库完全无需改代码。" },
      ] },
      { h2: "哪些功能可用", blocks: [
        { type: "list", items: [
          "链、智能体和 LangGraph 工作流——协议不变。",
          "通过标准集成使用流式输出、工具调用和结构化输出。",
          "所有受支持的 Claude 模型（Opus、Sonnet、Haiku）共用一把密钥和一个余额。",
        ] },
      ] },
    ],
    faq: [
      { q: "LangChain 支持自定义 Claude API 端点吗？", a: "支持。ChatAnthropic 接受 anthropic_api_url（或 ANTHROPIC_API_URL 环境变量），把它指向 https://api.apitoken.sale 即可，其余保持不变。" },
      { q: "LangChain 智能体和工具调用还能用吗？", a: "能——网关提供标准的 Anthropic Messages API，工具调用、流式输出和 LangGraph 智能体的行为与官方端点完全一致。" },
      { q: "从 LangChain 能用哪些模型？", a: "所有受支持的 Claude 模型——claude-opus-4-8、claude-sonnet-5、claude-haiku-4-5 等——共用一把密钥和预付余额。" },
    ],
  },
  "claude-api-litellm": {
    title: "在 LiteLLM 中使用 Claude API",
    h1: "在 LiteLLM 中使用 Claude API",
    description: "通过 apitoken.sale 将 LiteLLM 路由到 Claude：在 litellm_params 或代理配置中把 api_base 设为 api.apitoken.sale，每 token 费用降低 60–70%。",
    keywords: ["claude api litellm", "litellm anthropic", "litellm claude", "litellm api_base anthropic", "litellm 代理 claude"],
    dek: "LiteLLM 原生支持 Anthropic，并允许为每个模型覆盖端点——一行配置即可把你全部的 Claude 流量送经折扣网关。",
    sections: [
      { h2: "直接 SDK 调用", blocks: [
        { type: "code", code: `import litellm\n\nresponse = litellm.completion(\n    model="anthropic/claude-opus-4-8",\n    api_base="https://api.apitoken.sale",\n    api_key="sk-pool-•••",\n    messages=[{"role": "user", "content": "Hello"}],\n)` },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
      { h2: "LiteLLM 代理配置", blocks: [
        { type: "code", code: `# config.yaml\nmodel_list:\n  - model_name: claude-opus-4-8\n    litellm_params:\n      model: anthropic/claude-opus-4-8\n      api_base: https://api.apitoken.sale\n      api_key: sk-pool-•••` },
        { type: "p", text: "用这份配置运行代理，你的 LiteLLM 网关的每个客户端都会透明地使用折扣版 Claude 端点——当多个服务共享一个路由层时尤其方便。" },
      ] },
      { h2: "为什么在这里通过 LiteLLM 路由 Claude", blocks: [
        { type: "list", items: [
          "在一个地方把所有服务切到更便宜的端点。",
          "沿用你已有的 anthropic/ 模型前缀和参数。",
          "apitoken.sale 控制台按密钥追踪消费，精确到 token。",
        ] },
      ] },
    ],
    faq: [
      { q: "LiteLLM 支持自定义 Anthropic api_base 吗？", a: "支持——在 litellm.completion() 或代理配置的 litellm_params 中传入 api_base，LiteLLM 就会把 Anthropic 格式的请求发送到 https://api.apitoken.sale。" },
      { q: "模型还用 anthropic/ 前缀吗？", a: "是的。使用 anthropic/claude-opus-4-8（或任何受支持的模型），让 LiteLLM 应用 Anthropic 协议；变的只有端点和密钥。" },
      { q: "基于 LiteLLM 的工具也适用吗？", a: "适用——凡是经 LiteLLM 路由的东西（包括许多编码智能体）都会从同一份配置继承折扣端点。" },
    ],
  },
  "claude-api-aider": {
    title: "在 Aider 中使用 Claude API",
    h1: "在 Aider 中使用 Claude API",
    description: "通过 apitoken.sale 在 Claude 上运行 Aider：导出 ANTHROPIC_API_BASE 和密钥，选一个 Claude 模型，以 60–70% 的折扣在终端结对编程。",
    keywords: ["claude api aider", "aider anthropic", "aider claude", "aider anthropic api base", "aider claude api 密钥"],
    dek: "Aider 是终端里的结对程序员，长会话烧 token 很快。用两个环境变量把它指向折扣网关，工作流保持原样。",
    sections: [
      { h2: "两个环境变量", blocks: [
        { type: "code", code: `export ANTHROPIC_API_KEY=sk-pool-•••\nexport ANTHROPIC_API_BASE=https://api.apitoken.sale\n\naider --model anthropic/claude-opus-4-8` },
        { type: "p", text: "Aider 底层通过 LiteLLM 路由 Anthropic 流量，而 LiteLLM 会读取 ANTHROPIC_API_BASE——因此无需任何配置文件。" },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
      { h2: "为 Aider 选择模型", blocks: [
        { type: "list", items: [
          "anthropic/claude-opus-4-8——最难的重构和长程智能体编辑。",
          "anthropic/claude-sonnet-5——日常默认；编码质量接近 Opus。",
          "anthropic/claude-haiku-4-5——快速修改和低成本实验。",
        ] },
        { type: "p", text: "长 Aider 会话正是 token 折扣不断累积的地方：仓库地图、diff 和多文件编辑全部按输入和输出计费。" },
      ] },
    ],
    faq: [
      { q: "Aider 支持自定义 Claude 端点吗？", a: "支持。Aider 对 Anthropic 模型使用 LiteLLM，而 LiteLLM 读取 ANTHROPIC_API_BASE 环境变量——把它设为 https://api.apitoken.sale，然后正常启动 Aider 即可。" },
      { q: "在 Aider 里哪个 Claude 模型最好？", a: "claude-sonnet-5 是大多数编码工作的最佳默认；最难的多文件任务切到 claude-opus-4-8。两者共用同一把密钥。" },
      { q: "长 Aider 会话能便宜多少？", a: "每个请求按官方 token 费率计费再减去你 60–70% 的折扣，直连要花 $10 的会话在这里只需 $3–4。" },
    ],
  },
  "claude-api-roo-code": {
    title: "在 Roo Code 中使用 Claude API",
    h1: "在 Roo Code 中使用 Claude API",
    description: "通过 apitoken.sale 将 VS Code 中的 Roo Code 接入 Claude：选择 Anthropic 提供方，启用自定义 base URL，粘贴密钥，以 60–70% 的折扣编码。",
    keywords: ["claude api roo code", "roo code anthropic", "roo code claude", "roo code 自定义 base url", "roo code api 密钥"],
    dek: "Roo Code 是带原生 Anthropic 提供方和自定义 base URL 选项的智能体 VS Code 扩展——在折扣网关上两分钟即可完成设置。",
    sections: [
      { h2: "三步设置", blocks: [
        { type: "steps", items: [
          "打开 Roo Code 设置，选择 Anthropic 作为 API 提供方。",
          "启用自定义 base URL 选项并设为 https://api.apitoken.sale；粘贴你的 sk-pool-… 密钥。",
          "选择一个模型，例如 claude-opus-4-8 或 claude-sonnet-5，然后开始任务。",
        ] },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
      { h2: "Roo Code 为什么烧 token——以及如何少花钱", blocks: [
        { type: "p", text: "智能体扩展会循环地读文件、规划、编辑、复查，一个任务可能跑很多次模型调用。这正是按 token 折扣最有价值的负载：同样的会话便宜 60–70%，控制台里还有 token 级明细。" },
        { type: "list", items: [
          "日常任务走 claude-sonnet-5，难题交给 claude-opus-4-8。",
          "提示缓存按更便宜的官方缓存费率计费，再叠加你的折扣。",
          "一把密钥同时覆盖 Roo Code、Cline、Cursor 和各 SDK。",
        ] },
      ] },
    ],
    faq: [
      { q: "Roo Code 支持自定义 Anthropic base URL 吗？", a: "支持——Anthropic 提供方设置里有自定义 base URL 选项；设为 https://api.apitoken.sale 并使用你的 apitoken.sale 密钥即可。" },
      { q: "这把密钥能让 Roo Code 用哪些模型？", a: "所有受支持的 Claude 模型——Opus 4.8 和 4.7、Sonnet 5 和 4.6、Haiku 4.5——共用一把密钥和一个预付余额。" },
      { q: "和用 Cline 有什么区别？", a: "设置几乎一样：两者都是带 Anthropic 提供方、接受自定义 base URL 的 VS Code 智能体。用你喜欢的那个即可；密钥在两者中都能用。" },
    ],
  },
};
