import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
  title: "Claude Opus 对比 Sonnet：哪个模型该在什么时候用",
  h1: "Claude Opus 对比 Sonnet：该用哪个模型？",
  description: "Claude Opus 对比 Sonnet，按任务定夺：Sonnet 5 是编码与智能体的默认选择，Opus 4.8 负责高难度推理——在同一把 apiToken.sale 密钥上分别为每 1M token $1.50/$7.50 和 $2.50/$12.50。",
  keywords: ["claude opus 对比 sonnet", "编码用 opus 还是 sonnet", "该用哪个 claude 模型", "claude opus 4.8 对比 sonnet 5", "claude 模型对比", "claude opus sonnet 价格", "最适合编码的 claude 模型", "什么时候用 claude opus", "claude api 模型路由", "anthropic opus sonnet 价格", "claude sonnet 5 对比 opus"],
  dek: "Claude Opus 还是 Sonnet，本质是一个路由决策，而不是站队问题。Sonnet 5 以 Opus 四成的 token 价格承担日常编码和智能体工作；Opus 4.8 是高难度推理和长时间自主运行的升级档位。两者共用同一把 apiToken.sale 密钥和同一份预付余额，可以按请求切换。",
  sections: [
    { h2: "简短结论：默认 Sonnet，按需升级 Opus", blocks: [
      { type: "p", text: "几乎所有任务都先用 Claude Sonnet 5，只有当任务确实需要更深的推理时再升级到 Claude Opus 4.8。Sonnet 的编码质量接近 Opus，而 token 价格只有 Opus 的 40%，因此它是交互式编码、智能体循环和生产流量的正确默认选择。Opus 的溢价只在一小类任务上物有所值：跨多文件重构、架构决策，以及答错的代价高于 token 成本的长时间自主会话。" },
      { type: "p", text: "实践中最常见的错误是所有任务只用同一个模型。全部流量走 Opus 的团队在为日常工作多付钱；死守 Sonnet 的团队则在反复重试 Sonnet 本就完不成的任务上白白消耗。把两个档位当作一个系统：Sonnet 起草，Opus 处理例外。" },
    ] },
    { h2: "Opus 和 Sonnet 的真正差别", blocks: [
      { type: "p", text: "两个档位并不是不同的产品。它们共用 Anthropic Messages API、相同的请求结构、相同的 1M token 上下文窗口和 128K token 输出上限。你为 Opus 多付的钱买的是推理深度和长程一致性——在大型代码库或多步计划中不跑偏的能力。Sonnet 给你的是速度，以及在 95% 用不到那些能力的请求上低得多的计价。" },
      { type: "table", headers: ["", "Claude Opus 4.8", "Claude Sonnet 5"], rows: [
        ["Model ID", "claude-opus-4-8", "claude-sonnet-5"],
        ["官方价格（输入 / 输出 / 1M）", "$5 / $25", "$3 / $15"],
        ["本站（−50%）", "$2.50 / $12.50", "$1.50 / $7.50"],
        ["缓存读取（每 1M）", "$0.50", "$0.30"],
        ["上下文窗口", "1M token", "1M token"],
        ["最大输出", "128K token", "128K token"],
        ["最适合", "高难推理、长程智能体运行", "日常编码与智能体"],
      ] },
      { type: "note", text: "Anthropic 为 Sonnet 5 提供了截至 2026-08-31 的介绍期价格，每 1M token $2/$10；标准费率为 $3/$15。上一代——Opus 4.7 和 Sonnet 4.6——仍以与各自后继者相同的价格提供，因此没有理由为了省钱把新工作钉在旧模型上。" },
    ] },
    { h2: "适合用 Sonnet 的任务", blocks: [
      { type: "p", text: "凡是快速、迭代、以量取胜的工作，都是 Sonnet 的主场。输出 token 是每次请求中昂贵的一半——在两个档位上都是输入价格的五倍——所以能以更低输出费率一次通过的模型，几乎总是胜过被随意使用的更强模型。" },
      { type: "list", items: [
        "交互式编辑：单文件修改、生成测试、一段话就能描述清楚的重构。",
        "包含大量工具调用的智能体循环，账单主要由原始 token 量决定。",
        "高并发生产流量——分类、抽取、起草、摘要。",
        "任何对延迟敏感的场景：首 token 更快比最后几个质量百分点更重要。",
      ] },
    ] },
    { h2: "Opus 能值回票价的任务", blocks: [
      { type: "list", items: [
        "跨多文件的大型重构，漏掉一个边界情况代价高昂。",
        "架构与设计权衡分析——一次错误决策的成本远超 token 成本。",
        "需要在数小时累积的上下文中保持连贯的长时间自主会话。",
        "在 Sonnet 生成的 diff 合并前做最后一轮审查。",
      ] },
      { type: "p", text: "升级触发条件应该基于证据而非直觉：一次失败的 Sonnet 尝试、一个改动文件多到记不住的 diff、或一个承受不起反悔的决策。如果这些都不适用，你多半是在花 Opus 的钱做 Sonnet 的活。" },
      { type: "note", text: "两个档位都支持自适应思考（adaptive thinking）——在 Sonnet 5 上，省略 thinking 参数时默认开启；在 Opus 4.8 上则是推荐模式。思考 token 按输出 token 计费，因此在 Opus 上你要为刻意推理支付每 1M $25。在推理本身就是产出的地方开启它；机械性任务保持关闭。" },
    ] },
    { h2: "一把密钥，按请求切换模型", blocks: [
      { type: "p", text: "在档位间路由只是改一个字段的事。一把 apiToken.sale 密钥（形如 sk-pool-•••）覆盖 Opus、Sonnet 和 Haiku——以及受支持的 GPT、Gemini 和 Kimi 模型——共用同一份预付余额。没有按模型的套餐、没有单独注册、也不用换端点：只需在同一条 Anthropic Messages 请求里换掉模型 ID。" },
      { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 2048,\n    "messages": [{"role":"user","content":"Review this diff for regressions."}]\n  }'` },
      { type: "p", text: "把 \"claude-sonnet-5\" 改成 \"claude-opus-4-8\"，同一个调用就会跑在最高档位。50% 的统一 B2C 折扣对两者一视同仁，所以相对价格排序永远不变——Sonnet 始终是更便宜的那一档。每个请求都会在控制台显示 token 级用量，你的路由策略实际花多少钱一目了然。" },
    ] },
    { h2: "让支出可预测的路由模式", blocks: [
      { type: "steps", items: [
        "所有工作负载默认走 claude-sonnet-5——交互式会话、CI 智能体和生产流量都一样。",
        "预先定义升级触发条件：失败的 Sonnet 尝试、跨多文件重构、或不可逆的设计决策，交给 claude-opus-4-8。",
        "把 Opus 当审查者而不是起草者：Sonnet 写代码，Opus 审 diff，这样 Opus 费率只作用在一小部分 token 上。",
        "用提示词缓存复用长提示词——缓存读取在 Sonnet 5 上按每 1M $0.30、Opus 4.8 上按每 1M $0.50 计费，远低于输入费率，在长智能体循环中收益会持续累积。",
      ] },
      { type: "p", text: "在定下策略之前，先用自己的流量算一笔账：两个档位的差距足够大，升级率哪怕只变动一点，月度账单就会明显不同。" },
      { type: "link", text: "用 Claude API 成本计算器测算档位拆分", href: "/tools/claude-api-cost-calculator" },
      { type: "link", text: "比较所有 Claude 模型与价格", href: "/models" },
      { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台奖励余额——可用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
    ] },
  ],
  faq: [
    { q: "Claude Opus 编码比 Sonnet 更好吗？", a: "默认并不是。Sonnet 5 在日常编码和编辑上的质量接近 Opus，而 token 价格只有 40%，对大多数工作来说性价比更高。Opus 4.8 在复杂重构、架构设计和长时间自主运行上才真正领先。" },
    { q: "Opus 比 Sonnet 贵多少？", a: "官方价格为每 1M 输入/输出 token $5/$25，Sonnet 为 $3/$15。在 apiToken.sale，50% 统一折扣对两者都适用：Opus 4.8 为 $2.50/$12.50，Sonnet 5 为 $1.50/$7.50。" },
    { q: "Opus 和 Sonnet 能用同一把 API 密钥吗？", a: "可以。一把密钥和一份预付余额同时覆盖 Opus、Sonnet 和 Haiku。切换只需改请求中的模型 ID——无需单独的套餐、注册或端点。" },
    { q: "Opus 和 Sonnet 的上下文窗口一样吗？", a: "一样。Opus 4.8 和 Sonnet 5 都按标准价格提供 1M token 上下文窗口，没有长上下文溢价，单次响应最多输出 128K token。" },
    { q: "还应该用 Opus 4.7 或 Sonnet 4.6 吗？", a: "除非你有固定在它们上面的提示词或评测。Opus 4.7 与 Opus 4.8 同价，Sonnet 4.6 与 Sonnet 5 同价，因此新工作应面向当前一代。" },
  ],
};
