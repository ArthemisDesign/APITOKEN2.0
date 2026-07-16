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
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
      { h2: "支付方式如何运作", blocks: [
        { type: "p", text: "想充多少就充多少（整数美元）——没有固定的产品套餐。你的余额为预付制，永不过期，仅在 API 请求实际运行时才会扣费。" },
        { type: "list", items: [
          "通过安全的收银服务商用银行卡或加密货币支付。",
          "每次请求都会先换算为官方 Anthropic API 消费，再套用你当前的折扣。",
          "B2C 账户起步即比官方消费低 60%，随着累计充值增加最高可达 80% 折扣。",
        ] },
      ] },
      { h2: "拿到密钥能做什么", blocks: [
        { type: "p", text: "一把密钥即可解锁全部受支持的 Claude 系列——Opus、Sonnet 和 Haiku——覆盖 Claude Code、Cursor、Cline、Continue、Zed 以及官方 Anthropic SDK。协议本身毫无变化，改变的只有价格。" },
      ] },
    ],
    faq: [
      { q: "购买 Claude API 密钥需要 Anthropic 账户吗？", a: "不需要。apitoken.sale 自行签发密钥和余额，因此你无需 Anthropic 账户、邀请码或审批即可开始。" },
      { q: "密钥多快能激活？", a: "即时激活。你在控制台生成密钥后，下一次请求即可使用——没有排队，也没有人工审核。" },
      { q: "起步要花多少钱？", a: "你可以充值任意整数美元金额，而且每个新账户还会免费获得价值 $10 的 Claude 用量（按官方 API 价格计）。" },
    ],
  },
  "cheapest-claude-api": {
    title: "最便宜的 Claude API——最高立省 80%",
    h1: "使用 Claude API 最省钱的方式",
    description: "把 Claude API 成本最高削减 80%。apitoken.sale 以预付折扣价出售一模一样的 Anthropic Messages API——同样的模型、同样的接口、更低的每 token 单价。",
    keywords: ["最便宜的 claude api", "claude api 折扣", "便宜的 claude api", "claude api 价格", "节省 anthropic api 费用", "比 anthropic 更便宜的 claude api"],
    dek: "Claude API 按 token 计费，而在漫长的编码会话中这些 token 累积得很快。apitoken.sale 通过汇集预付余额并套用递进折扣，让你以最高低 80% 的价格用上完全相同的 API。",
    sections: [
      { h2: "为什么更便宜", blocks: [
        { type: "p", text: "你向同一套 Anthropic Messages API 发送同样的请求，得到同样的响应。底层唯一不同的是计费：每次调用按官方费率计量，然后在扣减你的余额前先减去你的折扣。" },
        { type: "list", items: [
          "B2C 账户从比官方消费低 60% 起步。",
          "随着累计充值增加，折扣最高可达 80%。",
          "B2B 批量定价单独商议。",
        ] },
      ] },
      { h2: "省钱效果最明显的场景", blocks: [
        { type: "p", text: "智能体编码、漫长的多轮会话以及重度依赖提示缓存的工作流消耗的 token 最多——因此绝对节省额也最大。为每项任务选对模型还能进一步叠加节省。" },
        { type: "note", text: "小贴士：把快速、廉价的工作交给 Haiku，把 Opus 留给高难度推理，能让余额撑得更久。" },
      ] },
      { h2: "无订阅、无绑定", blocks: [
        { type: "p", text: "没有月费。你充值的是永不过期的预付余额，仅在请求运行时才消耗，因此闲置的日子不花一分钱。" },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
    ],
    faq: [
      { q: "这真的是同一套 Claude API 吗？", a: "是的——同一套 Anthropic Messages API、相同的模型 ID、相同的请求与响应格式。只有每次调用的价格更低。" },
      { q: "我能省多少？", a: "B2C 定价从比官方 API 消费低 60% 起步，随着累计充值提高最多可达 80% 折扣。" },
      { q: "有没有隐藏费用或订阅？", a: "没有。余额为预付制、永不过期，仅由真实 API 用量消耗——没有月费。" },
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
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
      { h2: "与你现有的工具兼容", blocks: [
        { type: "p", text: "将 Claude Code、Cursor、Cline 或 Anthropic SDK 指向 https://api.apitoken.sale，即可像以前一样继续工作。支持提供俄语和英语服务，通过 Telegram 联系。" },
      ] },
    ],
    faq: [
      { q: "我能从俄罗斯付款吗？", a: "可以。你可以通过收银服务商用银行卡或加密货币支付，因此不要求受支持的 Anthropic 开票国家。" },
      { q: "我需要 VPN 吗？", a: "你无需 Anthropic 账户或开票国家。网络可达性取决于你自己的连接，但签发密钥和余额没有地域限制。" },
      { q: "有俄语支持吗？", a: "有——支持提供俄语和英语服务，通过 Telegram 联系。" },
    ],
  },
  "claude-api-crypto-payment": {
    title: "用加密货币支付 Claude API",
    h1: "用加密货币支付 Claude API",
    description: "在 apitoken.sale 上用加密货币或银行卡购买 Claude API 余额。无需 Anthropic 账户，即时激活，预付余额永不过期。",
    keywords: ["claude api 加密货币支付", "用加密货币购买 claude api", "claude api usdt", "加密货币支付 anthropic api", "claude api 比特币"],
    dek: "如果无法使用银行卡——或者你就是更偏好加密货币——你可以用加密货币为 Claude API 余额充值并立即开始。",
    sections: [
      { h2: "银行卡或加密货币，任你选", blocks: [
        { type: "p", text: "在结账时你可以通过安全的支付服务商用银行卡或加密货币支付。无论哪种方式，余额都会以预付形式进入你的账户，仅在请求运行时消耗。" },
      ] },
      { h2: "加密货币为何有帮助", blocks: [
        { type: "list", items: [
          "无需受支持的 Anthropic 开票国家。",
          "在银行卡被拒或无法使用的场景下尤其有用。",
          "余额永不过期，一次充值即可边开发边扣减。",
        ] },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
    ],
    faq: [
      { q: "支持哪些支付方式？", a: "你可以通过收银服务商用银行卡或加密货币支付。" },
      { q: "余额会过期吗？", a: "不会。预付余额永不过期，仅由真实 API 用量消耗。" },
    ],
  },
  "claude-api-without-waitlist": {
    title: "无需排队或审批的 Claude API",
    h1: "无需排队即可访问 Claude API",
    description: "跳过 Anthropic 的排队和审批。在 apitoken.sale 上创建账户、生成 Claude API 密钥，几分钟内完成你的第一次调用。",
    keywords: ["claude api 免排队", "claude api 即时开通", "claude api 无需审批", "快速获取 claude api 密钥", "claude api 无需 anthropic 账户"],
    dek: "等待审批会消磨积极性。apitoken.sale 让你即时自助访问所有受支持的 Claude 模型——不排队、不用销售通话、无需公司核验。",
    sections: [
      { h2: "即时、自助的访问", blocks: [
        { type: "steps", items: [
          "创建一个免费账户并打开控制台——无需审批、无需排队。",
          "生成一把 API 密钥（形如 sk-pool-…）。同一把密钥可用于所有受支持的 Claude 模型。",
          "将任意兼容 Anthropic 的工具指向 https://api.apitoken.sale，并携带 x-api-key 请求头向 /v1/messages 发送请求。",
        ] },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
      { h2: "所谓“即时”到底意味着什么", blocks: [
        { type: "p", text: "你生成密钥的那一刻它就已生效。注册与第一次成功请求之间没有人工审核环节，因此你可以在同一次会话中接通工具并上线。" },
      ] },
    ],
    faq: [
      { q: "真的完全不用排队吗？", a: "没错。访问是自助且即时的——你生成密钥后，下一次请求即可使用。" },
      { q: "我需要联系销售吗？", a: "不需要。B2C 访问完全自助。只有需要商议的 B2B 批量定价才涉及沟通。" },
    ],
  },
  "claude-api-quick-setup": {
    title: "两分钟搞定 Claude API 配置",
    h1: "两分钟配置好 Claude API",
    description: "两分钟 Claude API 快速上手：创建密钥、将 base URL 设为 api.apitoken.sale，用 curl、Python 或你的 IDE 发送第一个 /v1/messages 请求。",
    keywords: ["claude api 快速上手", "claude api 配置", "claude api 第一个请求", "anthropic messages api", "claude api base url"],
    dek: "这是从零到跑通一次 Claude API 调用最快的路径。下面的一切都使用标准 Anthropic Messages API，因此可以直接嵌入你现有的代码。",
    sections: [
      { h2: "1. 创建密钥", blocks: [ { type: "p", text: "注册、打开控制台并生成一把密钥。它形如 sk-pool-…，可用于所有受支持的模型。" } ] },
      { h2: "2. 设置你的接口地址", blocks: [
        { type: "p", text: "将任意兼容 Anthropic 的客户端指向网关：" },
        { type: "code", code: `Base URL:  https://api.apitoken.sale\nEndpoint:  POST /v1/messages\nHeaders:   x-api-key: sk-pool-•••\n           anthropic-version: 2023-06-01` },
      ] },
      { h2: "3. 发送你的第一个请求", blocks: [
        { type: "code", code: `curl https://api.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-opus-4-8",\n    "max_tokens": 1024,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
    ],
    faq: [
      { q: "我应该用哪个 base URL？", a: "用任意兼容 Anthropic 的工具时使用 https://api.apitoken.sale，并向 /v1/messages 发送请求。" },
      { q: "需要哪个认证请求头？", a: "发送携带你密钥的 x-api-key 以及 anthropic-version，与官方 Anthropic API 完全一致。" },
    ],
  },
  "free-claude-api-key": {
    title: "免费 Claude API 密钥助你上手",
    h1: "获取免费 Claude API 密钥开始使用",
    description: "在 apitoken.sale 上免费创建 Claude API 密钥，并获得价值 $10 的 Claude 用量（按官方 API 价格计）——无需银行卡、无需 Anthropic 账户、即时访问。",
    keywords: ["免费 claude api 密钥", "claude api 免费", "claude api 免费额度", "免费 anthropic api 密钥", "claude api 免银行卡"],
    dek: "你可以在花一分钱之前就创建密钥并进行真实的 Claude 调用。每个新的 B2C 账户都会获得价值 $10 的用量（按官方 API 价格计），让你先验证集成是否可行。",
    sections: [
      { h2: "“免费”包含什么", blocks: [
        { type: "list", items: [
          "一把可用于所有受支持 Claude 模型的 API 密钥。",
          "价值 $10 的 Claude 用量（按官方 API 价格计），无需银行卡。",
          "足够的额度让你接通工具并跑通真实请求。",
        ] },
        { type: "p", text: "当你准备用更多时，充值任意整数美元金额，你的折扣就会自动生效。" },
      ] },
      { h2: "如何领取", blocks: [
        { type: "steps", items: [
          "创建一个免费账户并打开控制台——无需审批、无需排队。",
          "生成一把 API 密钥（形如 sk-pool-…）。同一把密钥可用于所有受支持的 Claude 模型。",
          "将任意兼容 Anthropic 的工具指向 https://api.apitoken.sale，并携带 x-api-key 请求头向 /v1/messages 发送请求。",
        ] },
      ] },
    ],
    faq: [
      { q: "这些免费用量是真正的 API 访问吗？", a: "是的。包含的 $10 用量运行在与付费余额相同的 Claude 模型和接口上。" },
      { q: "开始使用需要银行卡吗？", a: "创建账户并使用包含的 $10 用量无需银行卡。" },
    ],
  },
  "claude-api-free-trial": {
    title: "Claude API 免费试用——几分钟即可开始",
    h1: "免费试用 Claude API",
    description: "几分钟内开始用 Claude 编码。apitoken.sale 为每个新账户提供价值 $10 的 Claude 用量（按官方价格计），无需银行卡、无需 Anthropic 审批。",
    keywords: ["claude api 免费试用", "试用 claude api", "claude api 测试", "claude api 沙盒", "claude api 演示"],
    dek: "无需单独申请试用——你只需注册，即可获得价值 $10 的用量（按官方 API 价格计），并对所有受支持的模型运行真实调用。",
    sections: [
      { h2: "先验证再付费", blocks: [
        { type: "p", text: "包含的用量正是为端到端检验网关而设计的：创建密钥、连接你的编辑器，确认流式输出、工具调用以及你喜欢的模型都表现如预期。" },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
      { h2: "随后按你的节奏扩展", blocks: [
        { type: "p", text: "当试用用量所剩不多时，充值任意金额即可。没有订阅、余额永不过期，因此你只为实际调用的部分付费。" },
      ] },
    ],
    faq: [
      { q: "我如何开始试用？", a: "只需创建账户——价值 $10 的官方价用量会自动添加，无需申请步骤。" },
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
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
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
    description: "通过一把 apitoken.sale 密钥以最高低于官方费率 80% 的价格访问 Claude Opus 4.8 和 4.7。最适合复杂推理、重构与长时间的智能体会话。",
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
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
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
    description: "通过 apitoken.sale 使用 Claude Sonnet 5 和 Sonnet 4.6——日常编码与智能体的默认模型，最高享受官方 API 价格 80% 的折扣。",
    keywords: ["claude sonnet api", "claude sonnet 5 api", "sonnet api 密钥", "claude sonnet 价格", "最适合编码的 claude 模型"],
    dek: "Sonnet 是主力：足够快，适合交互式编码；又足够聪明，胜任真正的智能体工作流。apitoken.sale 在一份折扣余额上提供 Sonnet 5 和 Sonnet 4.6。",
    sections: [
      { h2: "日常主力模型", blocks: [
        { type: "p", text: "对于大多数编码和智能体任务，Sonnet 是合适的默认选择——在质量、速度和成本之间取得了很好的平衡。把 Opus 留给真正的难题。" },
      ] },
      { h2: "Sonnet 定价说明", blocks: [
        { type: "p", text: "Claude Sonnet 5（claude-sonnet-5）采用介绍期官方费率，引擎始终在套用你的折扣前应用当前有效费率。Sonnet 4.6 仍可在同一把密钥上使用。" },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
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
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
      { h2: "一把密钥混用多种模型", blocks: [
        { type: "p", text: "由于所有模型共用一把密钥和余额，你可以把廉价工作路由给 Haiku（claude-haiku-4-5），只把高难度请求升级到 Sonnet 或 Opus。" },
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
    description: "用 apitoken.sale 密钥把 Cursor 接入 Claude：将 Anthropic base URL 设为 api.apitoken.sale，粘贴密钥，选择模型，以最高 80% 的折扣编码。",
    keywords: ["cursor claude api 密钥", "cursor claude api", "cursor anthropic 密钥", "在 cursor 中使用 claude", "无需 cursor pro"],
    dek: "Cursor 允许你自带 Anthropic 密钥，这意味着你可以用折扣预付余额在 Cursor 中运行 Claude，而非捆绑套餐。",
    sections: [
      { h2: "三步配置", blocks: [
        { type: "steps", items: [
          "打开 Cursor → Settings → Models → Anthropic API。",
          "将 base URL 设为 https://api.apitoken.sale 并粘贴你的 sk-pool-••• 密钥。",
          "选择一个模型（如 claude-opus-4-8）即可开始编码。",
        ] },
      ] },
      { h2: "配置", blocks: [
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : https://api.apitoken.sale\nAPI key  : sk-pool-•••\nModel    : claude-opus-4-8` },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
    ],
    faq: [
      { q: "我能在 Cursor 里用自己的 Claude 密钥吗？", a: "可以。Cursor 的 Anthropic 提供方接受自定义 base URL 和密钥，因此你可以把它指向 apitoken.sale。" },
      { q: "我还需要 Cursor Pro 吗？", a: "你可以通过自己的 API 密钥和余额运行 Claude；需要 Cursor 自身套餐的功能与模型提供方是相互独立的。" },
    ],
  },
  "claude-api-for-vs-code": {
    title: "在 VS Code 中使用 Claude API（Cline、Continue）",
    h1: "在 VS Code 中使用 Claude API",
    description: "用 apitoken.sale 密钥通过 Cline 或 Continue 在 VS Code 中运行 Claude。将 Anthropic base URL 设为 api.apitoken.sale，以折扣价按 token 付费。",
    keywords: ["claude api vs code", "cline claude api", "continue claude api", "在 vscode 中使用 claude", "vscode anthropic api 密钥"],
    dek: "Cline、Continue 等免费 VS Code 智能体接受任意兼容 Anthropic 的接口，因此你可以在 VS Code 中用折扣余额与 Claude 一起编码。",
    sections: [
      { h2: "Cline", blocks: [
        { type: "code", code: `# Cline → Settings\nAPI Provider : Anthropic\nBase URL     : https://api.apitoken.sale\nAPI Key      : sk-pool-•••\nModel        : claude-opus-4-8` },
      ] },
      { h2: "Continue", blocks: [
        { type: "code", code: `// ~/.continue/config.json\n{\n  "models": [{\n    "title": "Claude via apiToken.sale",\n    "provider": "anthropic",\n    "apiBase": "https://api.apitoken.sale",\n    "apiKey": "sk-pool-•••",\n    "model": "claude-opus-4-8"\n  }]\n}` },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
    ],
    faq: [
      { q: "哪些 VS Code 扩展可以用？", a: "任何支持兼容 Anthropic 接口的扩展都能配合 apitoken.sale 密钥使用，包括 Cline 和 Continue。" },
      { q: "我需要付费扩展吗？", a: "不需要。Cline 和 Continue 都是免费的；你只需为对应预付余额的 Claude API 用量付费。" },
    ],
  },
  "cursor-without-anthropic-account": {
    title: "无需 Anthropic 账户在 Cursor 中使用 Claude",
    h1: "无需 Anthropic 账户在 Cursor 中运行 Claude",
    description: "没有 Anthropic 账户？改用 apitoken.sale 密钥在 Cursor 中使用 Claude。即时访问、银行卡或加密货币支付，享受最高 80% 的官方 API 费率折扣。",
    keywords: ["无需 anthropic 账户使用 cursor", "cursor claude 无 anthropic", "cursor claude api 密钥", "不用 anthropic 账户使用 claude"],
    dek: "如果你无法或不愿创建 Anthropic 账户，apitoken.sale 会签发自己的密钥，Cursor 会把它当作 Anthropic 提供方接受。",
    sections: [
      { h2: "为什么可行", blocks: [
        { type: "p", text: "Cursor 与 Anthropic Messages API 通信。apitoken.sale 暴露的正是这套 API，因此 Cursor 分辨不出差别——它只是使用你的密钥和 base URL。" },
      ] },
      { h2: "配置方法", blocks: [
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : https://api.apitoken.sale\nAPI key  : sk-pool-•••\nModel    : claude-opus-4-8` },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
    ],
    faq: [
      { q: "这样做需要 Anthropic 账户吗？", a: "不需要。apitoken.sale 提供密钥和余额，因此无需 Anthropic 账户。" },
      { q: "这个集成是官方 Anthropic API 吗？", a: "Cursor 使用标准的 Anthropic Messages API；apitoken.sale 以折扣价提供同一套 API。" },
    ],
  },
  "anthropic-sdk-base-url": {
    title: "使用自定义 Base URL 调用 Anthropic SDK",
    h1: "将 Anthropic SDK 指向 apitoken.sale",
    description: "通过将 base_url 设为 api.apitoken.sale，用官方 Anthropic Python 和 TypeScript SDK 调用 apitoken.sale。相同的 SDK、相同的代码、更低的每 token 成本。",
    keywords: ["anthropic sdk base url", "anthropic python sdk 自定义接口", "claude sdk base url", "anthropic typescript sdk", "claude api sdk"],
    dek: "官方 Anthropic SDK 允许你覆盖 base URL，因此切换到 apitoken.sale 只是一行改动——你的模型 ID 和消息代码保持完全不变。",
    sections: [
      { h2: "Python", blocks: [
        { type: "code", code: `from anthropic import Anthropic\n\nclient = Anthropic(\n    base_url="https://api.apitoken.sale",\n    api_key="sk-pool-•••",\n)\nmsg = client.messages.create(\n    model="claude-opus-4-8",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Hello"}],\n)` },
      ] },
      { h2: "TypeScript", blocks: [
        { type: "code", code: `import Anthropic from "@anthropic-ai/sdk";\n\nconst client = new Anthropic({\n  baseURL: "https://api.apitoken.sale",\n  apiKey: "sk-pool-•••",\n});\nconst msg = await client.messages.create({\n  model: "claude-opus-4-8",\n  max_tokens: 1024,\n  messages: [{ role: "user", content: "Hello" }],\n});` },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
    ],
    faq: [
      { q: "我能继续使用官方 Anthropic SDK 吗？", a: "可以。把 base_url（Python）或 baseURL（TypeScript）设为 apitoken.sale，其余一切保持不变。" },
      { q: "模型 ID 会变吗？", a: "不会。使用相同的模型 ID，例如 claude-opus-4-8 和 claude-sonnet-5。" },
    ],
  },
  "apitoken-vs-anthropic-direct": {
    title: "apitoken.sale 对比 Anthropic 官方直购",
    h1: "apitoken.sale 对比直接向 Anthropic 购买",
    description: "对比 apitoken.sale 与 Anthropic 官方直购：完全相同的 Messages API 和模型，但最高立省 80%、无需账户、支持银行卡或加密货币支付。",
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
          "价格：B2C 最高比官方消费低 80%。",
          "开通：无需 Anthropic 账户、排队或开票国家要求。",
          "支付：银行卡或加密货币。",
        ] },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
      { h2: "各自适合谁", blocks: [
        { type: "p", text: "如果你已经拥有顺畅的 Anthropic 开票和企业协议，直购或许适合你。如果你想用同样的模型但更便宜、更快上手，并且能用银行卡或加密货币付款，那么 apitoken.sale 是务实之选。" },
      ] },
    ],
    faq: [
      { q: "apitoken.sale 是真正的 Claude API 吗？", a: "是的——它提供同一套 Anthropic Messages API 和模型。只有定价和开通方式不同。" },
      { q: "为什么它比 Anthropic 官方直购更便宜？", a: "余额是预付且汇集的，并对官方消费套用最高 80% 的递进折扣。" },
    ],
  },
  "apitoken-vs-openrouter": {
    title: "面向 Claude 的 apitoken.sale 对比 OpenRouter",
    h1: "面向 Claude 的 apitoken.sale 对比 OpenRouter",
    description: "在挑选 Claude 网关？对比 apitoken.sale 与 OpenRouter：原生 Anthropic 接口加预付折扣，还是一个多提供方路由器。",
    keywords: ["openrouter 替代", "apitoken 对比 openrouter", "claude api 网关", "openrouter claude", "最佳 claude api 网关"],
    dek: "两者都能让你在没有 Anthropic 账户的情况下用上 Claude，但构建方式不同。如果 Claude 是你的主力模型，原生 Anthropic 接口能让一切保持简单。",
    sections: [
      { h2: "原生 Anthropic 接口", blocks: [
        { type: "p", text: "apitoken.sale 在 https://api.apitoken.sale 暴露标准的 Anthropic Messages API，因此 Claude Code、Cursor 和 Anthropic SDK 无需任何适配器即可工作。你无需经过通用的多提供方抽象层路由。" },
      ] },
      { h2: "预付折扣，而非加价", blocks: [
        { type: "list", items: [
          "面向 B2C 的递进折扣，最高比官方 Claude 消费低 80%。",
          "一把密钥和一份余额覆盖 Opus、Sonnet 和 Haiku。",
          "银行卡或加密货币充值，永不过期。",
        ] },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
    ],
    faq: [
      { q: "为什么选择 Claude 原生网关？", a: "如果 Claude 是你的主力模型，原生 Anthropic 接口意味着你现有的 Anthropic 工具和 SDK 无需改动即可工作。" },
      { q: "apitoken.sale 会加价吗？", a: "不会——它对官方 Claude 消费套用折扣，而非加价。" },
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
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
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
    description: "了解 Claude API 定价：按 token 的输入与输出费率、提示缓存，以及 apitoken.sale 如何套用最高 80% 的递进折扣。",
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
        { type: "p", text: "每次调用先换算为官方 Anthropic 消费，再减去你的折扣：B2C 从 60% 折扣起步，随累计充值增长最高可达 80%。每次请求都在控制台中以 token 级别的明细可见。" },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
    ],
    faq: [
      { q: "Claude API 如何定价？", a: "按 token 计费，分为输入和输出，缓存读取另有更便宜的费率。更大的模型每 token 成本更高。" },
      { q: "折扣如何套用？", a: "先计算官方消费，再在扣减余额前减去你的 B2C 折扣（60% 直至 80%）。" },
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
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
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
          "减去你当前的折扣（B2C 为 60% 直至 80%）。",
          "净额从你的预付余额中扣除。",
        ] },
      ] },
      { h2: "完全可见", blocks: [
        { type: "p", text: "每次请求都在控制台中显示，含输入、输出、缓存和思考 token，因此你始终清楚余额去向。" },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
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
    description: "apitoken.sale 的密钥即时激活。生成密钥、充值，几分钟内即可完成一次成功的 Claude API 调用——无需人工审核或排队。",
    keywords: ["claude api 激活时间", "claude api 密钥多快", "即时 claude api 密钥", "claude api 就绪时间"],
    dek: "从创建密钥到使用它之间没有等待期。激活是即时的，因此唯一的速度限制就是你把密钥粘贴到工具里的速度。",
    sections: [
      { h2: "生来即时", blocks: [
        { type: "p", text: "密钥在你生成的那一刻即生效。充值在支付确认后立即计入余额，而银行卡支付几秒内即可确认。" },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
    ],
    faq: [
      { q: "我的密钥多久能用？", a: "立即可用。没有人工审核——刚生成的密钥在下一次请求即可使用。" },
      { q: "充值需要多久？", a: "银行卡支付几秒内计入；加密货币在网络确认交易后计入。" },
    ],
  },
  "claude-api-supported-countries": {
    title: "Claude API 支持的国家",
    h1: "你可以在哪里使用 apitoken.sale",
    description: "apitoken.sale 全球可用，无 Anthropic 开票国家要求。用银行卡或加密货币支付，从 Anthropic 不直接服务的地区使用 Claude API。",
    keywords: ["claude api 支持的国家", "claude api 全球可用", "anthropic api 国家限制", "claude api 可用地区"],
    dek: "由于密钥和余额由我们签发，因此没有 Anthropic 开票国家的门槛。这让身处直接注册困难地区的开发者也能用上 Claude API。",
    sections: [
      { h2: "没有开票国家门槛", blocks: [
        { type: "list", items: [
          "无需 Anthropic 账户或受支持的开票国家。",
          "支持银行卡和加密货币支付选项。",
          "通过 Telegram 提供英语和俄语支持。",
        ] },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
    ],
    faq: [
      { q: "Claude API 在我的国家可用吗？", a: "apitoken.sale 没有开票国家要求，因此你可以从 Anthropic 不直接开票的地区购买余额并使用密钥。" },
      { q: "支付限制方面如何？", a: "你可以用银行卡或加密货币支付，这在银行卡无法使用的地方很有帮助。" },
    ],
  },
  "claude-api-refund-policy": {
    title: "Claude API 退款政策",
    h1: "退款与支持",
    description: "了解 apitoken.sale 如何处理余额、退款和支持。预付余额永不过期，并通过 Telegram 提供英语和俄语帮助。",
    keywords: ["claude api 退款", "apitoken 退款政策", "claude api 支持", "claude api 退钱", "claude api 帮助"],
    dek: "预付余额被设计为低风险：永不过期，你只花实际调用的部分，而支持只需一条消息即可触达。",
    sections: [
      { h2: "余额与退款", blocks: [
        { type: "p", text: "由于余额为预付且永不过期，未使用的资金会保留供未来使用。退款处理通过原支付服务商进行；请携带你的账户信息联系支持。" },
      ] },
      { h2: "获取帮助", blocks: [
        { type: "p", text: "支持通过 Telegram 提供英语和俄语服务，也可通过邮箱 apitokensale@gmail.com 联系。大多数集成问题都能得到快速解答。" },
        { type: "note", text: "新账户开通即获得价值 $10 的 Claude 用量（按官方 API 价格计），足够你接通工具并在充值前跑通真实调用。" },
      ] },
    ],
    faq: [
      { q: "我的余额会过期吗？", a: "不会。预付余额永不过期，仅由真实 API 用量消耗。" },
      { q: "我如何联系支持？", a: "通过 Telegram 用英语或俄语联系支持，或发邮件至 apitokensale@gmail.com。" },
    ],
  },
};
