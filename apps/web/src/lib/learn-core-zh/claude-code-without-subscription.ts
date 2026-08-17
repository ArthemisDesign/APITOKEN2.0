import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude Code 免订阅使用指南",
    h1: "不用 $200/月套餐，照样跑 Claude Code",
    description: "用按量付费的 API 余额跑 Claude Code，不用按月订阅。把 ANTHROPIC_BASE_URL 指向 router.apitoken.sale，只为实际用量付费。",
    keywords: ["claude code 免订阅", "claude code 不用订阅", "claude code api key", "claude code 按量付费", "claude code 不用 max 套餐", "claude code 无订阅", "claude code anthropic_base_url", "claude code 便宜用法", "claude code 预付 api", "claude code 计费替代方案"],
    dek: "Claude Code 不订阅也能用：把它指向任意一个兼容 Anthropic 的 API 密钥即可。在 apiToken.sale 上，就是预付余额加官方 token 价格固定 5 折——没有月费，没有席位费，闲置不计费。",
    sections: [
      { h2: "Claude Code 只需要一把 API 密钥，不需要套餐", blocks: [
        { type: "p", text: "是的，Claude Code 不需要任何 Anthropic 订阅就能跑。这个 CLI 只读两个环境变量：ANTHROPIC_BASE_URL 和 ANTHROPIC_API_KEY，然后向它们指向的 Anthropic Messages API 端点发请求。给它一把 apiToken.sale 的预付密钥，每个会话就按 token 从余额里扣费，而不是交固定月费。" },
        { type: "p", text: "工具本身没有任何变化：同样的 agent 循环，同样的文件编辑，同样的终端工作流——唯一不同的是请求落到了哪里、怎么计费。" },
      ] },
      { h2: "$200/月的套餐到底买到了什么", blocks: [
        { type: "p", text: "顶配 Claude 套餐是一个消费级订阅：固定费用换来 Anthropic 自家应用里的交互式使用，附带无法直接计量的用量上限。如果你每天都高强度对话、又完全不碰 API，它是划算的。" },
        { type: "p", text: "但如果你的用量忽高忽低，想要一把可编程的密钥给自己的脚本和工具用，或者宁可在一周没怎么写代码时付 $0，这套餐就不合适了。按量付费的 API 计费把模型倒了过来：不为\"存在\"付费，只有 token 真正流动时才产生费用。" },
      ] },
      { h2: "两个变量，把 Claude Code 切到按量付费", blocks: [
        { type: "steps", items: [
          "在 apiToken.sale 注册免费账户，充值任意整数美元金额——余额永不过期。",
          "在控制台生成一把 API 密钥（形如 sk-pool-…）。一把密钥覆盖所有受支持的 Claude 模型，同一份余额还能用 GPT、Gemini 和 Kimi。",
          "导出下面两个变量，重启 shell，然后运行 claude。在 Claude Code 里用 /status 验证——它会显示当前端点和认证来源。",
        ] },
        { type: "code", code: `# ~/.zshrc or ~/.bashrc\nexport ANTHROPIC_BASE_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••\n\n# new shell, then just run\nclaude` },
        { type: "note", text: "如果切换后 Claude Code 报认证错误，常见原因是旧的 shell：变量必须在 claude 进程启动之前导出。开一个新终端，或者 source 一下你的 rc 文件，再重试。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "一个会话按 token 算多少钱", blocks: [
        { type: "p", text: "每个请求都按官方 Anthropic token 价格计量，先扣除固定的 50% B2C 折扣，再从余额扣费。Agent 式编程是输入重型的——仓库上下文、工具结果和对话历史每一轮都会重发——所以你的钱主要花在输入那一列。" },
        { type: "table", headers: ["模型", "官方输入 / 输出（每 1M token，美元）", "本站（−50%）"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "p", text: "控制台会逐条展示每个请求的模型和 token 明细，所以一次长会话可以逐行审计，而不是订阅制里那种看不清的黑盒一天。" },
        { type: "link", text: "完整分模型定价，含缓存价格", href: "/models" },
        { type: "link", text: "用免费计算器估算一个月的 Claude Code 用量", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "在长 agent 会话里省余额", blocks: [
        { type: "p", text: "选模型是最大的杠杆。Claude Code 支持会话中途切换模型，所以日常修改交给 Sonnet，把 Opus 留给那些值回票价的多文件硬重构。" },
        { type: "list", items: [
          "日常编码和快速修复：claude-sonnet-5。",
          "深度重构和长推理链：claude-opus-4-8。",
          "分诊、重命名、样板代码：claude-haiku-4-5，输入成本只有 Opus 的五分之一。",
        ] },
        { type: "note", text: "提示词缓存对重复上下文有帮助：缓存命中的输入 token 按更低的 cache-read 价格计费，所以保持一个长会话，胜过每次重开对话让它重读整个仓库。" },
      ] },
      { h2: "一把密钥，不止 Claude Code", blocks: [
        { type: "p", text: "同一把密钥和同一份余额可以驱动所有兼容 Anthropic 的工具——Cursor、Cline、Continue、Zed、Aider，以及官方 SDK；通过路由器的 OpenAI 兼容通道和原生 Gemini 通道，这份预付余额还覆盖受支持的 GPT、Gemini 和 Kimi 模型。订阅则从来不会给你这样一把密钥。" },
        { type: "link", text: "把 Anthropic SDK 指向同一个端点", href: "/docs/learn/anthropic-sdk-base-url" },
        { type: "link", text: "在 Cursor 里使用同一把密钥", href: "/docs/learn/claude-api-key-for-cursor" },
      ] },
      { h2: "什么时候订阅仍然值得", blocks: [
        { type: "p", text: "诚实面对自己的使用模式。如果你每个工作日都满强度跑 Claude Code 八小时，固定套餐可能比纯按 token 计费更便宜——订阅正是为这种用量画像定价的。其他人——周末项目、脉冲式工作的自由职业者、在一个预算里混用多个 AI 工具的团队——都在为从没用过的闲置天数买单。预付余额没有闲置天数：随用随充；万一需要退款，可通过客服走原支付渠道办理（Telegram，支持英语和俄语，或发邮件到 apitokensale@gmail.com）。" },
      ] },
    ],
    faq: [
      { q: "不订阅 Claude 能用 Claude Code 吗？", a: "能。设置 ANTHROPIC_BASE_URL 和 ANTHROPIC_API_KEY，Claude Code 就能跑在任意兼容 Anthropic 的 API 密钥上——包括 apiToken.sale 的预付密钥——无需任何套餐。" },
      { q: "不订阅会丢失 Claude Code 的功能吗？", a: "不会。CLI 的行为完全一致；变的只有计费方式：从固定月费套餐变为按 token 从预付余额扣费。" },
      { q: "用预付余额跑一次 Claude Code 会话要多少钱？", a: "请求按官方 Anthropic 价格计量，再减去固定 50% 的 B2C 折扣。Sonnet 5 折算下来是每 1M 输入/输出 token $1.50 / $7.50；Opus 4.8 是 $2.50 / $12.50。" },
      { q: "Claude Code 支持自定义 ANTHROPIC_BASE_URL 吗？", a: "支持，这个变量正是 CLI 选择端点的方式。把它指向 https://router.apitoken.sale，就能得到同样的 Anthropic Messages API 和同样的模型 ID。" },
      { q: "有没有免费试用 Claude Code 的办法？", a: "通过 Google 或 GitHub 注册的 apiToken.sale 新账户可获得 $5 平台奖励余额，足够在充值前跑一次真实的 Claude Code 试用会话。" },
    ],
  };
