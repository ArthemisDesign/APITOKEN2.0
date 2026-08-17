import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude Max 套餐 vs Claude API",
    h1: "Claude Max 订阅 vs API",
    description: "Claude Max 套餐 vs Claude API:$100/$200 订阅到底买了什么、边界在哪里,以及什么时候按量付费、统一五折的 API token 才是更划算的选择。",
    keywords: ["claude max 套餐", "claude max vs api", "claude 订阅 vs api", "claude max 值得买吗", "claude max 套餐价格", "claude max 用量限制", "claude api 按量付费", "claude code 不用 max 套餐", "claude 免订阅", "便宜的 claude api", "claude api token"],
    dek: "Claude Max 套餐是 Anthropic 的顶配订阅档,为整天泡在 claude.ai 和 Claude Code 里的人准备。Claude API 则是同一批模型的按量计费版本,为软件和用量起伏不定的人准备。本文把 Claude Max 套餐和按量付费的 API 计费放在一起对比,帮你选出真正匹配你工作方式的那个。",
    sections: [
      { h2: "Claude Max 套餐到底包含什么", blocks: [
        { type: "p", text: "Claude Max 是 Anthropic 自家产品的订阅,不是开发者产品。它位于 $20/月的 Pro 套餐之上,分两档——$100/月和 $200/月——买到的是 claude.ai、桌面和移动应用,以及用你账号登录的 Claude Code 里大得多的交互式用量额度。额度按会话制限制管理,以五小时滚动窗口重置,重度使用时还叠加每周上限。撞到天花板,就只能等窗口重置。" },
        { type: "p", text: "大多数对比都跳过的关键细节:Max 订阅不包含任何 API 用量。Anthropic 把订阅和 API 当作两套独立系统计费。$200 那一档也不附带哪怕一个 token 的 Messages API 额度。" },
      ] },
      { h2: "订阅做不了的事", blocks: [
        { type: "p", text: "订阅是在 Anthropic 的应用里把你验证为一个人,它无法验证你的软件。Max 席位不附带 API 密钥,所以任何需要密钥的东西都直接用不了它。" },
        { type: "list", items: [
          "从你自己的代码调用 Messages API 的生产后端和 SaaS 功能。",
          "CI 流水线、批处理任务、cron 驱动的智能体,以及一切无人值守运行的东西。",
          "要求 API 密钥的第三方工具:Cursor、VS Code 智能体、Continue、Aider、LangChain、LiteLLM。",
          "在规模化场景下对系统提示词、temperature、工具调用和结构化输出的程序化控制。",
          "团队或服务用量——Max 套餐是单人席位,不是基础设施。",
        ] },
        { type: "note", text: "如果你的目标就是 Claude Code,那同样不需要 Max——Claude Code 用 API 密钥跑得很好,按 token 计费,而不是消耗会话额度。" },
      ] },
      { h2: "API 如何计量同样的模型", blocks: [
        { type: "p", text: "API 没有月费,也没有会话窗口。每个请求按输入 token(你的提示词和上下文)和输出 token(模型的回复)计量;缓存读取的价格远低于新输入,缓存写入单独计量。流式响应与非流式计费完全相同。你拿到的是同样的前沿模型——Opus、Sonnet、Haiku——以及精确、可审计的单请求用量,而不是一块看不透的额度表。" },
        { type: "p", text: "对预算的影响很简单:闲置时间零成本。安静的一周只调了三次 API,就只花三次调用的钱,而不是摊一份 $200 订阅。" },
      ] },
      { h2: "算一笔盈亏平衡的账", blocks: [
        { type: "p", text: "诚实的对比看的是用量形态,不是标价。Max 适合持续、重度的交互式工作——每个工作日几小时的 Claude Code、长时间的 claude.ai 会话——这种场景按 token 的账单可能超过 $200。API 则赢下一切突发式、程序化或跨工具混用的场景,因为没有保底也没有封顶:只为烧掉的 token 付费,别的一分不花。" },
        { type: "p", text: "在 apiToken.sale 上,这笔账进一步倒向 API。每个请求先按 Anthropic 官方费率计量,再在扣余额之前统一减去 50% 的 B2C 折扣。也就是说,$200 预付余额覆盖 $400 的官方 API 消费——是 $200 Max 月费 token 预算的两倍,而且不过期、没有重置窗口。" },
        { type: "table", headers: ["你的用量形态", "更合适的选择"], rows: [
          ["每个工作日几小时的交互式 Claude Code", "Claude Max 可能说得通"],
          ["一周几次编码会话,时间不固定", "按量付费的 API"],
          ["智能体、CI、脚本或生产应用", "API——订阅做不到这一点"],
          ["Cursor、Continue、Aider 或其他基于密钥的工具", "必须有 API 密钥"],
          ["同一项目里同时用 Claude 和 GPT、Gemini 或 Kimi", "一份预付的多供应商余额"],
        ] },
        { type: "link", text: "动手前先估算你每月的 token 开销", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "用 API 余额跑 Claude Code,无需 Max", blocks: [
        { type: "p", text: "把 Claude Code 指向一把按量付费密钥只需两个环境变量。所有功能原样保留——唯一变化的是计费:从订阅额度变成按 token 扣你的预付余额。" },
        { type: "code", code: `export ANTHROPIC_BASE_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••\n\n# then just run\nclaude` },
        { type: "p", text: "同一把密钥也能在 https://router.apitoken.sale/v1 说 OpenAI 兼容协议,带 Authorization: Bearer 头,所以你工具链里那些期待 OpenAI 形态端点的工具,无需第二个账号就能用。" },
        { type: "link", text: "完整教程:无需订阅使用 Claude Code", href: "/docs/learn/claude-code-without-subscription" },
      ] },
      { h2: "token 价格减半,一把密钥打通四家供应商", blocks: [
        { type: "p", text: "apiToken.sale 以官方 token 费率统一五折,出售同一个 Anthropic Messages API 的预付、按量付费访问。余额永不过期,没有月费,每个请求都在控制台里给出 token 级明细——输入、输出和缓存各环节——让你永远知道钱花在了哪里。" },
        { type: "list", items: [
          "一把密钥覆盖受支持的 Claude、GPT、Gemini 和 Kimi 模型——不用给每家供应商单独开户。",
          "Anthropic Messages 协议用 x-api-key,OpenAI 兼容用 Bearer,Gemini 原生用 x-goog-api-key。",
          "需要时才充值;没用完的余额就静静等着。",
        ] },
        { type: "link", text: "含缓存费率的按模型定价", href: "/models" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户自带 $5 平台奖励余额——适用于受支持的 Claude、GPT、Gemini 和 Kimi 模型;邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "Claude Max 套餐包含 API 访问吗?", a: "不包含。Claude Max 是 Anthropic 自家应用和 Claude Code 的订阅;API 单独计费,没有任何订阅档位捆绑 API token。" },
      { q: "和 API 相比,Claude Max 值得买吗?", a: "对于每天在 Claude Code 和 claude.ai 里重度交互使用,$100 或 $200 档位可能跑赢按 token 计费。对于突发式、程序化或多工具用量,按量付费的 API——尤其是 apiToken.sale 统一五折的价格——几乎总是更便宜。" },
      { q: "没有 Max 或 Pro 订阅能用 Claude Code 吗?", a: "可以。把 ANTHROPIC_BASE_URL 和 ANTHROPIC_API_KEY 指向一把预付 API 密钥,Claude Code 的表现完全一致,按 token 从你的余额扣费。" },
      { q: "撞到 Claude Max 用量限制会怎样?", a: "你会被限流,直到会话窗口重置——限制按五小时滚动窗口运作,另叠加每周上限。API 计费没有会话额度;用量只受余额约束,不受计时器约束。" },
      { q: "$200 的 API 额度比 $200 的 Max 月费更值吗?", a: "在 apiToken.sale 上,是的:官方费率统一五折意味着 $200 预付余额覆盖 $400 的 Anthropic 官方消费,永不过期,且在任何接受 API 密钥的工具里都能用。" },
      { q: "一把 API 密钥能同时服务 Claude 和其他模型供应商吗?", a: "可以——一把 apiToken.sale 密钥覆盖受支持的 Claude、GPT、Gemini 和 Kimi 模型,统一从一份预付余额扣费。" },
    ],
  };
