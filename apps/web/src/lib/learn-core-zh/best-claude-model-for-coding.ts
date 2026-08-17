import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "最适合写代码的 Claude 模型",
    h1: "最适合写代码的 Claude 模型",
    description: "写代码该用哪个 Claude 模型？一份按任务在 Opus、Sonnet、Haiku 之间做选择的实用指南——全部可用同一把 apiToken.sale 密钥访问。",
    keywords: ["最适合写代码的 claude 模型", "claude 编程模型", "opus sonnet haiku 对比", "claude 写代码用哪个模型", "claude sonnet 编程", "claude opus 编程", "claude haiku 编程", "claude 模型对比", "claude api 模型路由", "claude api 折扣"],
    dek: "写代码最好的 Claude 模型并不是某一个模型——而是在 Sonnet、Opus、Haiku 之间按任务做的选择。本指南给出路由规则、五折后的真实 token 费率，以及在同一把 apiToken.sale 密钥上切换档位所需的确切请求改动。",
    sections: [
      { h2: "简短答案：默认 Sonnet，硬骨头交给 Opus", blocks: [
        { type: "p", text: "写代码最好的 Claude 模型是：大部分工作用 Claude Sonnet 5，答错一次就要赔上几个小时的会话用 Claude Opus 4.8，介于两者之间的机械性批量任务用 Claude Haiku 4.5。按任务选，而不是按项目选：同一个端点、同一把密钥、同一份预付余额覆盖全部三档，模型只是你在每个请求里设置的一个字符串。" },
        { type: "p", text: "这种分工来自三档模型各自的定位。Sonnet 牺牲一点峰值推理能力，换来速度和低得多的 token 费率——这正是「改代码、运行、看报错、再改」这种交互式循环最看重的东西。Opus 在每个 token 上投入更多算力，更擅长把冗长、含糊的上下文串起来。Haiku 为延迟和价格调优，而不是为深度——当任务本来就没有深度可言时，这反而是优点。" },
      ] },
      { h2: "三档模型一览", blocks: [
        { type: "p", text: "下面四个模型 ID 全部按 Anthropic 官方 token 价格统一五折供应，从同一份预付余额扣费：" },
        { type: "table", headers: ["模型", "官方输入 / 输出（$ / 1M token）", "本站（−50%）", "最适合"], rows: [
          ["Claude Sonnet 5 (claude-sonnet-5)", "$3 / $15", "$1.50 / $7.50", "日常编码、智能体、代码审查"],
          ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50", "同样定位，上一代"],
          ["Claude Opus 4.8 (claude-opus-4-8)", "$5 / $25", "$2.50 / $12.50", "高难度重构、架构设计、长会话"],
          ["Claude Haiku 4.5 (claude-haiku-4-5)", "$1 / $5", "$0.50 / $2.50", "Lint、信息抽取、大批量编辑"],
        ] },
        { type: "p", text: "Opus 4.8 和 Sonnet 5 都提供 1M token 的上下文窗口，所以两者之间的选择取决于推理深度和费率，而不是你能往提示词里塞多少代码。如果你的工具链还钉在上一代，Sonnet 4.6 在同一把密钥上仍然可用。" },
      ] },
      { h2: "什么时候 Opus 4.8 配得上更高的费率", blocks: [
        { type: "p", text: "当任务存在真正的模糊性时，就该上 Opus：跨模块重构，选对抽象本身就是问题所在；评审一套不是你写的系统；或者排查一个症状离根因隔着三层的 bug。在这类会话里，弱一点的模型不会大声报错——它会产出看似合理却有细微错误的代码，你得用审查时间把差价还回去。" },
        { type: "p", text: "Opus 在长时间智能体任务里同样值回票价。一个连续规划、编辑、验证二十分钟的智能体是在不断叠加小的判断，早期一个好决策能省下整棵分支的无效工具调用。但如果是边界清晰、需求明确的工单，Sonnet 能用更低的成本更快交出同样的 diff——在那里升级纯粹是浪费。" },
        { type: "note", text: "一个实用的升级信号：如果 Sonnet 已经跑了两轮完整循环还没有收敛——重复同一个失败的修复，或者在几种方案之间来回打转——停下来，把报错日志放进提示词，在 Opus 上重启会话，让它从头重新规划。" },
      ] },
      { h2: "不该按 Sonnet 费率付费的活，交给 Haiku 4.5", blocks: [
        { type: "p", text: "真实项目里很大一部分编码流量是机械性的：lint 修复、日志分类、从 diff 里抽取符号、生成 commit message、搭测试的初稿。Haiku 4.5 以 Sonnet 三分之一的输入费率就能胜任这些工作，而它的低延迟让它成为每次保存、每次 CI 任务都要触发的场景的正确引擎。" },
        { type: "list", items: [
          "pre-commit 和 CI 钩子：lint 报错解释、conventional-commit 信息、changelog 草稿。",
          "抽取与路由：在更大的模型介入推理之前，从日志、堆栈或代码中抽出结构化字段。",
          "高扇出的智能体步骤：在 Sonnet 阅读候选短名单之前，先给候选文件打分或给搜索结果排序。",
        ] },
        { type: "p", text: "实践中行之有效的模式是流水线：Haiku 负责过滤和压缩，Sonnet 负责干活，Opus 负责审查高风险的部分。每个环节只为自己真正需要的判断力付费，而便宜的环节让昂贵的环节始终面对一份短而干净的输入。" },
      ] },
      { h2: "按请求切换模型，而不是按账户", blocks: [
        { type: "p", text: "apiToken.sale 在 https://router.apitoken.sale 上暴露标准的 Anthropic Messages API，密钥放在 x-api-key 请求头里，所以切换档位只需要改 model 字段这一行——不需要新凭据，不需要换套餐，也不需要第二家供应商：" },
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "messages": [{"role":"user","content":"Refactor this function"}]\n  }'` },
        { type: "steps", items: [
          "把客户端一次性指向路由端点：base URL 为 https://router.apitoken.sale，API 密钥为 sk-pool-•••。Cursor、Claude Code、Continue 和 Anthropic 官方 SDK 都支持自定义端点。",
          "把工具的默认模型设为 claude-sonnet-5，让日常交互式工作落在主力档位上。",
          "重活按会话覆盖——在 Claude Code 里，ANTHROPIC_MODEL=claude-opus-4-8 让这一个会话跑在 Opus 上，其余一切仍走 Sonnet。",
          "在你自己控制的代码里显式路由：预处理调用用 claude-haiku-4-5，核心循环用 claude-sonnet-5，最终审查用 claude-opus-4-8。",
        ] },
      ] },
      { h2: "模型路由对预付余额意味着什么", blocks: [
        { type: "p", text: "50% 折扣对每一档都一视同仁，所以路由决策的效果是相乘而不是相加：一个走 Haiku 的 CI 钩子每百万输入 token 只要 $0.50，放在一次 Opus 审查会话旁边几乎等于免费——而这种差价正是混用模型的全部意义。余额是预付且共享的，重度使用 Opus 的一周只是让它消耗得更快——没有需要升级或降级的套餐，按请求切换除了 token 本身不花任何钱。" },
        { type: "p", text: "在敲定路由策略之前，先按档位盘点典型的一天：多少请求是机械性的，多少是真正的编码循环，多少是真的难。把每一类乘以上表中的费率，你得到的就是一个站得住脚的月度数字，而不是拍脑袋的猜测。" },
        { type: "link", text: "用免费成本计算器估算你的模型组合", href: "/tools/claude-api-cost-calculator" },
        { type: "link", text: "并排对比每个 Claude 模型和价格", href: "/models" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额，可用于所有支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "写代码最好的 Claude 模型是哪个？", a: "Claude Sonnet 5 是日常编码和智能体循环的正确默认选择。复杂重构、架构设计和漫长的高风险会话用 Claude Opus 4.8，lint、信息抽取这类快速大批量任务用 Claude Haiku 4.5。" },
      { q: "能按 API 请求切换 Claude 模型吗？", a: "能。一把密钥和一份预付余额覆盖所有模型，切换只需要在标准 Messages API 请求里改 model 字段这一行——不需要新凭据，也不需要换套餐。" },
      { q: "写代码用 Claude Opus 值吗？", a: "对边界清晰、需求明确的任务来说不值——Sonnet 能以大约 Opus 六成的 token 费率交出同样的 diff。Opus 在模糊的工作上值回票价：跨模块重构、设计评审和长时间智能体任务，早期一个好决策能省下大量无效的工具调用。" },
      { q: "在 Cursor 或 Claude Code 里该设哪个 Claude 模型？", a: "默认设 claude-sonnet-5。遇到重活按次覆盖——在 Claude Code 里，ANTHROPIC_MODEL=claude-opus-4-8 只让该会话跑在 Opus 上，其余工具仍走 Sonnet。" },
      { q: "一把 apiToken.sale 密钥能同时用 Opus、Sonnet 和 Haiku 吗？", a: "能。所有支持的 Claude 模型——Opus 4.8、Sonnet 5、Sonnet 4.6 和 Haiku 4.5——都跑在同一把密钥和同一份预付余额上，各自均按 Anthropic 官方 token 价格五折计费。" },
    ],
  };
