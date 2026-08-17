import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "用加密货币支付 Claude API",
    h1: "用加密货币支付 Claude API",
    description: "在 apiToken.sale 上用 USDT、BTC 或银行卡购买 Claude API 余额。无需 Anthropic 账户和受支持的账单国家，即时开通，预付余额永不过期。",
    keywords: ["claude api 加密货币支付", "用加密货币买 claude api", "claude api usdt 充值", "加密货币支付 anthropic api", "claude api 比特币支付", "claude api 无银行卡", "claude api 加密货币充值", "claude api 预付余额", "购买 claude api", "claude api 开通", "claude api 代币"],
    dek: "没有 Anthropic 账户、不在受支持的账单国家、甚至没有银行卡，也能用加密货币支付 Claude API。在 apiToken.sale 上，你可以通过安全的支付服务商用 USDT、BTC 或其他主流币种为预付余额充值，同一份余额也随时接受银行卡充值。余额永不过期，只在 API 请求运行时才消耗。",
    sections: [
      { h2: "加密货币充值的完整流程", blocks: [
        { type: "p", text: "整个流程都在结账页内完成，只需一笔交易。我们这边不需要你通过交易所账户中转，没有发票，也没有人工审核环节——支付服务商确认你的转账后，平台就会把余额计入你的账户。" },
        { type: "steps", items: [
          "打开控制台，选择充值，在结账时选择加密货币——以后的任何一笔充值仍可改用银行卡。",
          "输入任意整数美元金额。没有固定的商品目录；你计划花多少就充多少。",
          "从任意钱包或交易所，向安全支付服务商展示的地址转入该金额。",
          "等待链上确认。网络一旦确认交易，余额就会自动计入你的账户。",
        ] },
        { type: "note", text: "确认时间取决于你转出的币种和网络，与平台无关——快速网络上的稳定币比拥堵的 BTC 内存池确认得更快。无论哪种情况，入账规则都一样：网络确认后余额才会到账。" },
        { type: "p", text: "三个好习惯能让加密货币充值省心到底。务必在结账页为你的币种显示的那条网络上转账——USDT 转错链是最经典的不可找回错误。如果你希望钱包转出的金额等于到账金额，优先用稳定币，因为波动较大的币在转账到确认之间可能产生价差。最后保留交易哈希：万一需要客服追踪某笔付款，交易哈希加上你的账户邮箱是最快的解决方式。" },
      ] },
      { h2: "银行卡还是加密货币：每笔充值的实际差别", blocks: [
        { type: "p", text: "两种方式进入的是同一份预付余额，只在请求运行时扣减，且永不过期。选择是按笔充值算的，而不是按账户算的，所以这个月用 USDT 充、下个月用银行卡充，其他什么都不用变。" },
        { type: "table", headers: ["", "银行卡", "加密货币（USDT、BTC 等）"], rows: [
          ["在哪里支付", "安全支付服务商", "安全支付服务商"],
          ["余额何时入账", "结账确认付款时", "链上确认之后"],
          ["金额", "任意整数美元", "任意整数美元"],
          ["需要 Anthropic 账户", "不需要", "不需要"],
          ["更适合", "你的卡可用且能通过 3-D Secure 验证", "银行卡被拒、不受支持，或你的资金本就以加密货币持有"],
        ] },
      ] },
      { h2: "什么时候加密货币是接入 Claude API 的实用途径", blocks: [
        { type: "p", text: "Anthropic 官方计费要求受支持的国家和可用的支付方式，这恰好是许多开发者卡住的地方。加密货币充值完全绕开了这道门槛：你根本不用开通 Anthropic 账户，因此也不需要受支持的账单国家。" },
        { type: "list", items: [
          "你所在地区没有受支持的 Anthropic 账单国家，无法直接注册。",
          "你的银行拒绝跨境或与 API 相关的刷卡，或者你的卡就是不被结账页接受。",
          "你的流动资金以稳定币持有，宁愿直接花 USDT，也不想先经银行兑换。",
          "你希望 API 支出不出现在共享卡或公司卡的对账单上。",
        ] },
        { type: "p", text: "由于余额是预付且永不过期的，加密货币充值并不是订阅承诺。充一次，随请求运行逐步扣减，需要时再充——两种方式都可以。未用完的资金会一直可用，所以没必要一次充太多：小额、定期充值和一次大额充值的效果完全一样。" },
      ] },
      { h2: "花余额：一把密钥通用 Claude、GPT、Gemini 和 Kimi", blocks: [
        { type: "p", text: "余额到账后，在控制台生成密钥——形如 sk-pool-…，即时激活，无需排队。这一把密钥可在 https://router.apitoken.sale 上调用标准的 Anthropic Messages API，因此 Claude Code、Cursor、Cline 和官方 Anthropic SDK 都能原样使用；同一份余额还覆盖受支持的 GPT、Gemini 和 Kimi 模型。每个请求都按提供商官方费率计量，然后在从余额扣费前统一应用 50% 的 B2C 折扣。" },
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-opus-4-8",\n    "max_tokens": 1024,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
        { type: "p", text: "如果支付或充值出了任何问题，可以通过 Telegram 联系英文或俄文客服，或发邮件至 apitokensale@gmail.com，退款会通过原支付服务商处理。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台奖励余额——可用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "能用 USDT 购买 Claude API 访问权限吗？", a: "可以。在结账时选择加密货币，把 USDT 或其他受支持的稳定币转到显示的地址；网络确认交易后，你的 Claude API 余额即入账。务必在结账页显示的那条网络上转账，并在余额到账前保留交易哈希。" },
      { q: "支持哪些支付方式？可以混用吗？", a: "通过安全支付服务商支持银行卡和加密货币（USDT 及其他稳定币、BTC 及主流币种）。选择是按笔充值算的，所以你可以在银行卡和加密货币之间自由切换；无论哪种方式，都可以充任意整数美元金额。" },
      { q: "用加密货币支付需要 Anthropic 账户或受支持的账单国家吗？", a: "不需要。apiToken.sale 自行签发密钥和预付余额，因此没有 Anthropic 注册、没有账单国家门槛，也没有排队——密钥生成后的下一个请求即可使用。余额永不过期，只被真实的 API 调用消耗。" },
    ],
  };
