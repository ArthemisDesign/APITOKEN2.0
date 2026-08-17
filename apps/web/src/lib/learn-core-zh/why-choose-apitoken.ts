import type { LocalizedContent } from "../learn";
import { BASE, OPENAI_BASE, KEY } from "../learn-shared";

export const content: LocalizedContent = {
    title: "为什么选择 apiToken.sale",
    h1: "为什么选择 apiToken.sale",
    description: "为什么开发者用一把 apiToken.sale 密钥同时访问 Claude、GPT、Gemini 和 Kimi：原生或兼容 API、官方 B2C 价格五折，支持银行卡或加密货币付款。",
    keywords: ["为什么选择 apitoken.sale", "多提供商 api", "claude api 折扣", "gpt api 折扣", "gemini api 折扣", "kimi api 密钥", "openai 兼容 api", "预付 api 余额", "一个密钥调用 claude gpt gemini", "llm api 网关"],
    dek: "apiToken.sale 存在的理由：同时使用 Claude、GPT、Gemini 和 Kimi 的开发者，往往要维护四个计费账户、四套 SDK 配置和四个定价页面。这项服务把它收敛成一把预付密钥，在官方 B2C 消费基础上统一五折——同时不会把四种协议压平成一种。下面讲清楚哪些保持原生、折扣覆盖什么、边界在哪里。",
    sections: [
      { h2: "一把密钥，四个模型系列", blocks: [
        { type: "p", text: "apiToken.sale 是一个独立的多提供商 API 网关：一把密钥、一份预付余额，即可访问受支持的 Claude、GPT、Gemini 和 Kimi 模型，无需分别开通 Anthropic、OpenAI、Google Cloud 或 Kimi 的计费账户。大多数评测文章漏掉的关键点是：这四个系列并没有被塞进同一个转译后的 API——每个提供商都保留其生态原本使用的协议。流式、工具调用和提示词缓存的语义都按各家自己的事件格式透传，所以能在官方端点上跑通的客户端代码，在这里无需改动即可运行。" },
        { type: "table", headers: ["提供商系列", "提供的协议", "鉴权请求头", "支持的模型示例"], rows: [
          ["Claude", "Anthropic Messages", "x-api-key", "claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5"],
          ["Kimi", "Anthropic Messages，另有 OpenAI 兼容通道", "x-api-key", "kimi/k3, kimi/kimi-for-coding"],
          ["GPT", "OpenAI 兼容", "Authorization: Bearer", "gpt-5.6-terra"],
          ["Gemini", "原生 generateContent", "x-goog-api-key", "gemini-3.6-flash"],
        ] },
      ] },
      { h2: "原生协议，而不是转译层", blocks: [
        { type: "p", text: "大多数多提供商路由器会把一切归一化成一套最小公分母的 schema，接缝很快就露出来：工具调用的 payload、流式事件类型和缓存控制在转译之后行为都会变。这里的网关按各协议原本的形态终止请求，所以指向网关的 Anthropic SDK 表现得就像直连 Anthropic，而 Google 形态的客户端也保留自己的 generateContent 路由。" },
        { type: "code", code: `# Claude and Kimi — Anthropic Messages\ncurl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"claude-sonnet-5","max_tokens":1024,"messages":[{"role":"user","content":"ping"}]}'\n\n# GPT — OpenAI-compatible\ncurl ${OPENAI_BASE}/chat/completions \\\n  -H "Authorization: Bearer ${KEY}" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"gpt-5.6-terra","messages":[{"role":"user","content":"ping"}]}'\n\n# Gemini — native generateContent\ncurl ${BASE}/v1beta/models/gemini-3.6-flash:generateContent \\\n  -H "x-goog-api-key: ${KEY}" \\\n  -H "content-type: application/json" \\\n  -d '{"contents":[{"parts":[{"text":"ping"}]}]}'` },
        { type: "note", text: "Kimi 是唯一一个同时走两条通道的系列：面向 Claude 形态工具链的 Anthropic Messages，以及面向只支持 OpenAI 协议的客户端的通用 OpenAI 兼容通道。按客户端选通道，而不是按账户——同一把密钥两条通道都能用。" },
      ] },
      { h2: "五折到底覆盖什么", blocks: [
        { type: "p", text: "定价模型一句话就能说完：每个请求按实际用量明细折算成官方提供商消费，然后统一减去 50% 的 B2C 折扣。同一费率覆盖全部四家提供商的支持模型——没有按提供商分档的比价，也没有加价售卖的 SKU 目录。" },
        { type: "list", items: [
          "计量按每次调用的真实用量明细进行：输入、输出、缓存，以及模型特有的长上下文或图片计费项。",
          "折扣在计量之后应用，所以官方消费打五折就是你真实流量结构的五折，而不是针对某个假想标价。",
          "费用从一份预付余额中扣除，按整数美元充值；余额永不过期，也没有客户订阅，闲置的星期不花一分钱。",
        ] },
        { type: "link", text: "查看全部四家提供商的分模型费率", href: "/models" },
        { type: "link", text: "充值前先估算你的工作负载成本", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "一份余额，代替四个计费账户", blocks: [
        { type: "p", text: "开通是即时、自助的：创建账户，生成一把形如 sk-pool-… 的密钥，下一个请求就能用。没有候补名单，没有人工审核，也不需要提供商一侧的批准——这同时也绕开了每家提供商各自要求的四次注册、绑卡验证和计费国家门槛。" },
        { type: "p", text: "你可以通过安全的收银台用银行卡或加密货币充值任意整数美元金额。这一点有双重意义：在受支持计费国家没有公司卡的团队也能付款，而在银行卡通道不稳定的地方，加密货币充值能让余额持续可用。如果某笔付款需要撤销，退款由原支付提供商处理——需要时可以通过 Telegram 联系到英文和俄文客服。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额，适用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "密钥上的护栏，控制台里的可见性", blocks: [
        { type: "p", text: "每把密钥都可以附加可选的终身消费上限和到期日期——足以放心把密钥交给外包人员、CI 任务或业余项目，而不必每天盯着。控制台按模型和提供商细分，展示每个请求的 token 级用量，让预付余额可审计，而不是一个黑盒。" },
        { type: "list", items: [
          "每密钥终身消费上限：对累计消费的硬性封顶，可选。",
          "每密钥到期日期：密钥在你选定的日期之后失效，可选。",
          "每请求 token 级明细：输入、输出和缓存各项，按模型和提供商细分。",
        ] },
      ] },
      { h2: "五分钟内发出第一个请求", blocks: [
        { type: "steps", items: [
          "创建免费账户并在控制台生成密钥。用 Google 或 GitHub 注册可获 $5 平台奖励余额；邮箱密码账户不享受此奖励。",
          `对于 Claude Code 和 Anthropic 形态的工具：export ANTHROPIC_BASE_URL=${BASE} 和 ANTHROPIC_API_KEY=${KEY}，然后照常运行工具。`,
          `对于 OpenAI 形态的客户端（Cursor、Continue、Aider、LangChain、LiteLLM）：把 base URL 设为 ${OPENAI_BASE}，并用同一把密钥作为 Bearer token。`,
          `对于 Gemini 客户端：保留 Google SDK 的调用形态，把它指向 ${BASE}，密钥放在 x-goog-api-key 请求头里。`,
          "先发一个低成本请求，在控制台的 token 级用量里确认到账后，再把密钥接入真实工作负载。",
        ] },
      ] },
      { h2: "什么时候 apiToken.sale 不是合适的选择", blocks: [
        { type: "p", text: "取舍值得直说。网关覆盖四个提供商系列——如果你的工作负载需要受支持的 Claude、GPT、Gemini 和 Kimi 产品线之外的模型，通用路由器是更合适的工具。如果你的组织已经与某家提供商直接签有企业协议，合同里的谈判条款可能比统一的 B2C 折扣更划算。" },
        { type: "p", text: "对其他所有人——独立开发者、小团队，以及任何想用一把密钥以官方 B2C 半价使用 Claude、GPT、Gemini 和 Kimi、用银行卡或加密货币付款而不开四个计费账户的人——这是从零到跑通多提供商环境的最短路径。" },
      ] },
    ],
    faq: [
      { q: "apiToken.sale 和其他 API 网关有什么不同？", a: "一把密钥和一份余额覆盖四个提供商系列，并统一享受 B2C 五折；同时每个客户端保留适合的原生或兼容协议——Anthropic Messages、OpenAI 兼容或 Gemini 原生 generateContent——而不是一套转译后的统一 schema。" },
      { q: "所有提供商都会被强制塞进同一个转译后的 API 吗？", a: "不会。Claude 和 Kimi 保留 Anthropic Messages，GPT 使用 OpenAI 兼容路由，Gemini 保留 Google 原生形态的 API。此外，只支持 OpenAI 协议的客户端也可以通过统一的 OpenAI 兼容通道调用 Kimi。" },
      { q: "apiToken.sale 是什么？", a: "一个独立的多提供商 API 网关，为受支持的 Claude、GPT、Gemini 和 Kimi 模型提供折扣预付访问，无需分别开通各提供商的计费账户。" },
      { q: "可以在付费前试用吗？", a: "可以。用 Google 或 GitHub 创建的账户自带 $5 平台奖励余额，可用于全部四家提供商的支持模型；邮箱密码账户不享受此奖励。" },
      { q: "预付余额会过期或自动续费吗？", a: "不会。余额永不过期，也没有客户订阅——按整数美元充值，只有在实际发起 API 请求时才会扣费。" },
      { q: "哪些工具可以使用 apiToken.sale 密钥？", a: "任何支持 Anthropic Messages、OpenAI API 形态或 Gemini generateContent 的工具：Claude Code、Cursor、Cline、Continue、Zed、Aider、LangChain、LiteLLM 以及各提供商官方 SDK，各自指向对应的端点即可。" },
    ],
  };
