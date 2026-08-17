import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "如何购买 Claude API 密钥",
    h1: "如何购买 Claude API 密钥",
    description: "几分钟内买到 Claude API 密钥：预付余额、银行卡或加密货币结账，一把密钥通用所有 Claude 模型，统一按官方消费打 5 折——无需 Anthropic 账户。",
    keywords: ["购买 claude api 密钥", "claude api key 购买", "如何购买 claude api", "claude api 密钥", "anthropic api key", "claude api 加密货币支付", "claude api 无需 anthropic 账户", "claude api 预付余额", "claude api 充值", "claude api 折扣", "buy claude api key"],
    dek: "想在没有 Anthropic 账户、邀请码和公司信用卡的情况下买到 Claude API 密钥，整个流程大约五分钟：创建账户、用银行卡或加密货币给预付余额充值、生成密钥。这把密钥调用的是与 Anthropic 官方签发的密钥相同的 Anthropic Messages API——Opus、Sonnet、Haiku 全部包含——且统一按官方消费打 5 折。下面是完整的购买流程、计费逻辑，以及你的工具应该指向的端点。",
    sections: [
      { h2: "五分钟完成购买：账户、余额、密钥", blocks: [
        { type: "p", text: "在 apitoken.sale 上购买 Claude API 密钥只需三步：注册、给预付余额充值、点击生成。密钥在下一次请求时即刻生效——没有排队、没有人工审核，整个流程的任何环节都不需要 Anthropic 账户或审批。" },
        { type: "steps", items: [
          "用 Google、GitHub 或邮箱加密码创建账户。通过 Google 或 GitHub 创建的账户自带 $5 平台奖励余额，可用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受该奖励。",
          "充值任意整数美元金额——没有固定的产品目录，也没有最低方案限制。用银行卡支付，或通过安全的收银服务商用加密货币支付。",
          "打开控制台生成 API 密钥。密钥形如 sk-pool-•••，即刻可用；同一把密钥还覆盖平台上受支持的 GPT、Gemini 和 Kimi 模型。",
          "用一次请求验证密钥（见本指南末尾的 curl）。返回 200 且有真实输出就说明完成了；返回 401 则说明密钥或请求头名称写错了。",
        ] },
      ] },
      { h2: "无需 Anthropic 账户、邀请码或公司信用卡", blocks: [
        { type: "p", text: "直接从 Anthropic 购买意味着要先有一个 Anthropic 账户，而对很多买家来说这正是拦路虎：注册、审批、绑卡要求。apitoken.sale 替换了这整个环节——它自行签发密钥、自行管理预付余额，所以你只需要一个邮箱地址（或 Google/GitHub 登录）和一种付款方式。" },
        { type: "p", text: "作为交换，你拿到的不是克隆品，也不是转手的第三方模型。网关提供的是同一套 Anthropic Messages API 和同样的 Claude 模型，请求行为完全一致。与直接购买相比只有三点不同：每次调用的价格、注册方式和付款方式。" },
      ] },
      { h2: "一把密钥，全部 Claude 模型——以及三种协议", blocks: [
        { type: "p", text: "密钥不绑定某个模型或某个工具。一份余额即可覆盖全部受支持的 Claude 系列，各自使用标准模型 ID：" },
        { type: "list", items: [
          "Claude Opus 4.8（claude-opus-4-8）和 Opus 4.7",
          "Claude Sonnet 5（claude-sonnet-5）和 Sonnet 4.6",
          "Claude Haiku 4.5（claude-haiku-4-5）",
        ] },
        { type: "p", text: "Anthropic 通道原样提供 Messages API：SSE 流式输出、工具调用和 system 提示词的行为与 Anthropic 官方端点完全一致。不同客户端之间唯一变化的只是你指向的协议通道——三条通道共享同一把密钥和同一份余额：" },
        { type: "table", headers: ["协议通道", "端点", "认证请求头"], rows: [
          ["Anthropic Messages（Claude、Kimi）", "https://router.apitoken.sale/v1/messages", "x-api-key"],
          ["OpenAI 兼容（GPT 及 OpenAI 形态的客户端）", "https://router.apitoken.sale/v1", "Authorization: Bearer"],
          ["Gemini 原生", "https://router.apitoken.sale", "x-goog-api-key"],
        ] },
        { type: "p", text: "由于线上协议就是原生 Anthropic 格式，这把密钥无需插件或代理即可直接接入所有兼容 Anthropic 的工具：Claude Code、Cursor、Cline、Continue、Zed 以及官方 Anthropic SDK。协议没有任何变化，变化的只有价格。" },
      ] },
      { h2: "预付余额的账怎么算：50% 折扣如何生效", blocks: [
        { type: "p", text: "没有订阅，也没有月费。余额为预付制、永不过期，只在 API 请求实际运行时扣费——闲置几周一分钱不花。每次调用的计费分三步：" },
        { type: "list", items: [
          "请求先按 Anthropic 官方 token 费率计量。",
          "再减去你当前的折扣：B2C 账户每次请求统一享受官方消费 50% 的折扣。",
          "净额从预付余额中扣除——所以 $50 余额可以覆盖 $100 的官方价用量。",
        ] },
        { type: "note", text: "余额用尽后，请求会以余额不足的错误失败，直到你再次充值——没有透支，也不会从你的银行卡产生意外扣款。" },
        { type: "link", text: "各模型定价（含缓存费率）", href: "/models" },
        { type: "link", text: "用免费成本计算器估算一个月的用量", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "把 Claude Code、Cursor 和 SDK 指向你的密钥", blocks: [
        { type: "p", text: "每个兼容 Anthropic 的客户端都只需要改两个值：base URL 和凭证。提示词、流式代码和工具定义原样保留。对 Claude Code 及其他由 shell 驱动的 agent，导出两个环境变量：" },
        { type: "code", code: `export ANTHROPIC_BASE_URL=https://router.apitoken.sale\nexport ANTHROPIC_AUTH_TOKEN=sk-pool-•••` },
        { type: "p", text: "官方 SDK 接收同样两个参数：" },
        { type: "code", code: `import anthropic\n\nclient = anthropic.Anthropic(\n    base_url="https://router.apitoken.sale",\n    api_key="sk-pool-•••",\n)` },
        { type: "p", text: "在 Cursor、Cline、Continue 和 Zed 中，同样两个字段位于服务商设置里——例如 Cursor → Settings → Models → Anthropic API。粘贴密钥、把 base URL 设为 https://router.apitoken.sale、选一个模型（如 claude-opus-4-8），请求就会走你的预付余额并套用折扣。" },
        { type: "note", text: "如果某个客户端只提供“OpenAI 兼容”类型的服务商，改用 https://router.apitoken.sale/v1 并携带 Authorization: Bearer 请求头——x-api-key 请求头属于 Anthropic Messages 通道。" },
      ] },
      { h2: "用一次请求验证购买", blocks: [
        { type: "p", text: "在把密钥接入大项目之前，先发一个最小调用。成本只有零点几美分，却能端到端验证整条链路——密钥、余额、端点：" },
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-haiku-4-5",\n    "max_tokens": 64,\n    "messages": [{"role":"user","content":"ping"}]\n  }'` },
        { type: "list", items: [
          "401 Unauthorized——密钥缺失或拼错、请求头不是 x-api-key，或 base URL 写错。",
          "400 Bad Request——检查模型 ID（例如 claude-haiku-4-5）以及是否设置了 max_tokens。",
          "402 / 余额不足——余额已空，充值任意整数美元金额即可。",
          "429 Too Many Requests——遵守 Retry-After 请求头并降低并发。",
        ] },
      ] },
    ],
    faq: [
      { q: "购买 Claude API 密钥需要 Anthropic 账户吗？", a: "不需要。apitoken.sale 自行签发密钥和预付余额，因此无需 Anthropic 账户、邀请码或审批即可开始，也不强制要求公司信用卡。" },
      { q: "购买后密钥多快生效？", a: "即刻生效。你在控制台生成密钥后，下一次请求就能用——没有排队，也没有人工审核。" },
      { q: "起步最少要花多少钱？", a: "充值支持任意整数美元金额，几美元就能起步。通过 Google 或 GitHub 创建的新账户还会获得 $5 平台奖励余额。" },
      { q: "可以用加密货币购买 Claude API 密钥吗？", a: "可以。结账通过安全的支付服务商受理银行卡和加密货币，充入的余额永不过期。" },
      { q: "这是官方的 Claude API 吗？", a: "是的——它提供同一套 Anthropic Messages API 和同样的 Claude 模型，包括流式输出和工具调用。不同的只有价格，以及注册和付款方式。" },
    ],
  };
