import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API 支持的国家/地区",
    h1: "Claude API 在哪些地区可用——以及真正的门槛是什么",
    description: "哪些国家可以使用 Claude API？Anthropic 按计费国家限制直接开户。apiToken.sale 没有这道门槛：用银行卡或加密货币支付，即可在任何地区调用 Claude。",
    keywords: ["claude api 支持的国家", "claude api 我的国家能用吗", "claude api 可用地区", "anthropic api 国家限制", "claude api not available in your country", "anthropic 支持的计费国家", "claude api 全球可用", "不支持的地区使用 claude api", "没有 anthropic 账户使用 claude api", "任意地区购买 claude api"],
    dek: "搜索 Claude API 支持的国家，通常只说明一件事：Anthropic 在你所在地区收不了款。门槛是 Anthropic 的计费国家名单，而不是模型本身。apiToken.sale 直接为你签发密钥和余额，没有计费国家要求——你可以在全球任何地区使用 Claude API，用银行卡或加密货币支付。",
    sections: [
      { h2: "你所在的国家能用 Claude API 吗？", blocks: [
        { type: "p", text: "Claude API 本身没有技术上的区域封锁——门槛在 Anthropic 的计费环节。如果你所在的国家不在 Anthropic 直接计费的支持名单里，即使模型在网络上完全可达，你也无法完成注册或绑定支付方式。apiToken.sale 拆掉了这道门槛：密钥和余额由我们直接签发，因此身处 Anthropic 不直接服务的地区也能正常使用 Claude API，而且完全不需要 Anthropic 账户。" },
        { type: "p", text: "Anthropic 公布了直接销售 API 服务的国家名单，这份名单会随时间变动。如果你的国家不在名单上，直接注册通常会卡在支付环节——不是 API 拒绝你的流量，而是没有办法给账户充值。从技术上看，你原本要发的每个请求都完全可行，被堵住的只是计费关系。" },
        { type: "p", text: "由于账户、密钥和余额都在 apiToken.sale 上，整个流程不会问你在哪里。注册、充值、调用 API，都随你的笔记本或服务器所在的地区进行；其他地区的团队成员也可以用各自的密钥共享同一个余额。" },
        { type: "table", headers: ["要求", "Anthropic 官方直开", "apiToken.sale"], rows: [
          ["受支持的计费国家", "开通付费账户的必需条件", "不要求——无需 Anthropic 账户"],
          ["支付方式", "受支持地区的银行卡", "银行卡，或 USDT、BTC 等加密货币"],
          ["等待名单与审核", "可能遇到等待名单和审批", "没有——密钥即时激活，无需企业认证"],
          ["可以从哪里调用", "计费绑定受支持国家", "全球任意地区，通过 https://router.apitoken.sale"],
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "在 Anthropic 不服务的地区完成支付并调用", blocks: [
        { type: "p", text: "结账流程会适应你的地区，而不是拒绝它。银行卡可用的地区用银行卡支付；银行卡被拒或不可用的地区，可以通过安全的支付服务商选择加密货币——USDT 等稳定币、BTC 及其他主流币种——网络确认交易后余额即可到账。你可以充值任意整数美元金额，余额为预付制、永不过期，每个请求按 Anthropic 官方费率计量，并统一叠加 50% 的 B2C 折扣。" },
        { type: "steps", items: [
          "在 apiToken.sale 创建账户——没有审批环节，也没有计费国家表单。",
          "用银行卡充值任意整数美元金额，或选择加密货币并支付页面显示的确切金额；链上确认后余额到账。",
          "生成 API 密钥（形如 sk-pool-…）。一个密钥即可在同一个余额下调用受支持的 Claude、GPT、Gemini 和 Kimi 模型。",
          "把客户端指向 https://router.apitoken.sale，使用 Anthropic Messages 协议并带上 x-api-key 请求头，然后发送请求。",
        ] },
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"claude-sonnet-5","max_tokens":256,"messages":[{"role":"user","content":"ping"}]}'` },
        { type: "p", text: "协议不因你的地理位置而有任何变化。你拿到的是同一套 Anthropic Messages API——流式输出、工具调用和系统提示全部包含——覆盖整条受支持的 Claude 产品线：Opus 4.8 和 4.7、Sonnet 5 和 4.6、Haiku 4.5。" },
        { type: "list", items: [
          "可在 Claude Code、Cursor、Cline、Continue、Zed 以及官方 Anthropic SDK 中使用——只需设置 base URL，其余代码原封不动。",
          "每个请求都会带着模型、服务商和 token 明细出现在控制台里，在任何地区都能审计花费。",
          "退款通过原支付服务商处理；如需退款，请用你的账户邮箱联系支持团队。",
        ] },
        { type: "note", text: "一句实话：网络可达性取决于你自己的连接——购买余额和生成密钥没有任何地理门槛，但没有任何服务能绕过本地的网络封锁。支持团队通过 Telegram 提供英语和俄语服务，也可发送邮件至 apitokensale@gmail.com。" },
        { type: "link", text: "在模型页面浏览所有受支持模型及每 token 定价。", href: "/models" },
        { type: "link", text: "充值前用 Claude API 成本计算器估算你的工作量。", href: "/tools/claude-api-cost-calculator" },
      ] },
    ],
    faq: [
      { q: "我所在的国家能用 Claude API 吗？", a: "用 apiToken.sale，实际上可以。密钥和余额由我们直接签发，没有计费国家要求，因此你可以在 Anthropic 不直接计费的地区购买余额并使用 Claude API。" },
      { q: "Anthropic 不接受我的银行卡或所在国家怎么办？", a: "通过安全的结账服务商，用银行卡或 USDT、BTC 等加密货币向 apiToken.sale 付款。预付余额永不过期，每个请求按 Anthropic 官方费率计费，并统一减去 50% 的 B2C 折扣。" },
    ],
  };
