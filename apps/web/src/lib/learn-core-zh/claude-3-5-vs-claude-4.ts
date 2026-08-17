import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude 3.5 对比 Claude 4——有哪些变化，如何迁移",
    h1: "Claude 3.5 对比 Claude 4：到底改了什么",
    description: "Claude 3.5 对比 Claude 4：实际提升在哪、从 claude-3-5-sonnet 到当前系列的模型 ID 对照、每 token 价格，以及在 apiToken.sale 上一行就能完成的迁移。",
    keywords: ["claude 3.5 对比 4", "claude 4 vs 3.5", "从 claude 3.5 迁移", "claude-3-5-sonnet-20241022 替代", "claude 模型迁移", "claude sonnet 5 对比 3.5 sonnet", "claude 4 模型 id", "claude 3.5 sonnet 停售", "claude api 价格", "升级 claude 模型"],
    dek: "Claude 3.5 对比 Claude 4 并不是一个难分伯仲的选择：当前系列在智能体编码、推理和长上下文一致性上更强，而它运行的 Messages API 没有任何变化。迁移就是换一个模型 ID——本文给出精确的对照表、价格影响，以及改这个字符串之前值得重测的几件事。",
    sections: [
      { h2: "3.5 到 4 系列到底改了什么", blocks: [
        { type: "p", text: "当前的 Claude 系列在 API 用户真正花钱的场景上完胜 3.5：智能体编码、多步推理，以及在长上下文中保持连贯。API 本身没变——同一个 Messages 端点、同样的请求和响应结构、同样的请求头——所以真正的问题不是“要不要换”，而是“换成哪个 ID”。答案在下面，改动本身只有一行。" },
        { type: "p", text: "有三项提升具体到了可以做规划的程度。第一，智能体编码：工具调用、多文件编辑和长时间自主运行的失败率相比 3.5 Sonnet 明显降低，这也是当前模型成为 Claude Code 和多数编码智能体默认选择的原因。第二，上下文：Opus 和 Sonnet 系列以标准价格提供 100 万 token 的上下文窗口，而 3.5 这一代的上限是 20 万——长文档和大型仓库的工作负载不再需要分块绕路。第三，推理控制：当前模型支持思考力度可调的自适应思考（adaptive thinking），你可以只在真正需要的请求上为更多“深思”付费。" },
        { type: "p", text: "输出风格也有变化。新模型写出的文字更密、更直接，对格式指令的执行更字面化。这通常是好事，但针对 3.5 的习惯调过的提示词值得重跑一遍——详见后面的重测一节。" },
      ] },
      { h2: "模型 ID 对照：每个 3.5 模型对应到哪个", blocks: [
        { type: "p", text: "Anthropic 会随时间退役旧的模型 ID，当前的目录——无论在这里还是上游——都已是新一代。如果你的配置里还写着 3.5 时代的 ID，对照关系如下：" },
        { type: "table", headers: ["配置里的 3.5 时代 ID", "当前替代", "上一代可选"], rows: [
          ["claude-3-5-sonnet-20241022", "claude-sonnet-5", "claude-sonnet-4-6"],
          ["claude-3-5-haiku-20241022", "claude-haiku-4-5", "—"],
          ["claude-3-opus-20240229", "claude-opus-4-8", "claude-opus-4-7"],
        ] },
        { type: "p", text: "默认选中间一列。右边一列只为一种情况存在：你的提示词或评测绑定在特定一代上，想在重新校准期间用一个经过验证的中间档。上一代选项的每 token 标价与当前型号相同，所以留在旧型号上没有省钱的理由——只有稳定性的理由。" },
      ] },
      { h2: "升级对你的 token 账单有什么影响", blocks: [
        { type: "p", text: "按官方标价算，这次迁移接近零成本变化。Sonnet 5 标价 $3/$15（每 100 万输入/输出 token）——与 3.5 Sonnet 当年的价格完全相同——而且 Anthropic 在 2026-08-31 之前一直提供 $2/$10 的上市优惠。Opus 档位降价明显：3 Opus 标价 $15/$75，所以 $5/$25 的 Opus 4.8 只有当年这个档位的三分之一。Haiku 4.5 的标价略高于旧的 3.5 Haiku，但在你架构中的同一位置上，它的能力强得多。" },
        { type: "table", headers: ["模型", "官方输入 / 输出（$/1M）", "本站（−50%）"], rows: [
          ["Claude Sonnet 5 / 4.6", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "p", text: "在 apiToken.sale 上，每个请求先折算成 Anthropic 官方花费，再从中减去固定的 50% B2C 折扣，最后才扣你的预付费余额。档位之间的排序不变；每一行都只是比你当年按官方价支付的 3.5 时代账单更便宜。" },
        { type: "link", text: "用成本计算器估算你的实际负载", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "迁移就是一行 diff", blocks: [
        { type: "p", text: "因为线上协议完全一致，迁移就是改一个 JSON 字段的值。其他一切——端点、请求头、max_tokens、messages 数组、响应的 content 块、stop_reason 和 usage——都按你现有代码的处理方式原样保留。" },
        { type: "code", code: `# Before — Claude 3.5\ncurl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"claude-3-5-sonnet-20241022","max_tokens":1024,"messages":[{"role":"user","content":"Hello"}]}'\n\n# After — only the model field changes\n  -d '{"model":"claude-sonnet-5","max_tokens":1024,"messages":[{"role":"user","content":"Hello"}]}'` },
        { type: "p", text: "在 apiToken.sale 上完全没有凭证方面的工作：同一把 sk-pool 密钥和同一个 base URL 已经服务所有受支持的 Claude 模型，所以模型 ID 真的就是全部改动。如果你通过环境变量或设置面板配置模型——ANTHROPIC_MODEL、Cursor 的模型输入框、Continue 的配置项——在那里改掉再重新部署即可。" },
      ] },
      { h2: "值得重测，而不只是改指向", blocks: [
        { type: "p", text: "协议兼容的替换不等于行为一致的替换。在新 ID 上生产之前，留出一轮评测的预算：" },
        { type: "list", items: [
          "针对 3.5 调过的系统提示词：新模型对指令的执行更字面化，你当年为 3.5 加的变通写法（“记得要……”、重复的约束）现在可能把输出限制过头。跑一遍你的提示词套件，删掉不再需要的脚手架。",
          "输出长度：当前模型的回答往往更详尽。如果你当年为了让 3.5 保持简洁而压低过 max_tokens，切换后检查有没有 stop_reason: max_tokens 造成的截断。",
          "思考是可选开启的：adaptive thinking 同时影响延迟和 token 花费。在测出数据之前，对延迟敏感的路径保持关闭，在重推理路径上再有意识地开启。",
          "智能体循环：工具调用的 schema 没变，但新模型调用工具更积极，从工具错误中恢复的方式也不同。先完整观察一次智能体运行，再信任你的循环保护逻辑。",
        ] },
        { type: "note", text: "如果某个提示词或评测套件确实绑死在旧行为上，可以先迁到上一代 ID（claude-sonnet-4-6 或 claude-opus-4-7）作为过渡，而不是一次跨两代——价格相同，行为跨度更小。" },
      ] },
      { h2: "灰度切换——每个请求只改一个字符串", blocks: [
        { type: "p", text: "没有需要排期的账户级迁移，所以你可以按任意适合自己的粒度推进：按环境、按功能开关、按请求。常见做法是固定把一定比例的流量发向新模型 ID，旧配置保持不动，对比输出结果和仪表盘里逐请求的 token 明细，然后再正式切换。因为一把密钥、一份预付费余额覆盖所有受支持的 Claude 模型——同一账户下还有 GPT、Gemini 和 Kimi——灰度发布不会让你多出任何额外的凭证、套餐或供应商账户。" },
        { type: "link", text: "当前 Claude 阵容及各模型价格", href: "/models" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台奖励余额——适用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "Claude 4 编码比 Claude 3.5 强吗？", a: "是的——当前系列相比 3.5 提升最明显的就是智能体编码、多步推理和长上下文一致性，而且运行在同一套 Messages API 上。做编码负载，没有任何理由再在 3.5 时代的模型 ID 上开新项目。" },
      { q: "claude-3-5-sonnet-20241022 的接替者是谁？", a: "claude-sonnet-5 是直接继任者，标价同为 $3/$15；如果你的提示词绑定了旧行为，claude-sonnet-4-6 是上一代选项。切换只是 model 字段的一行改动。" },
      { q: "从 Claude 3.5 迁移需要改代码吗？", a: "只改模型 ID。端点、请求头（x-api-key 和 anthropic-version）、max_tokens、消息结构和响应解析全部不变，现有的 Messages API 代码照常工作。" },
      { q: "Claude 4 比 Claude 3.5 贵吗？", a: "按官方标价，Sonnet 5 与 3.5 Sonnet 当年同价（每 100 万 token $3/$15），Opus 档位比 3 Opus 便宜得多。在 apiToken.sale 上，官方花费再享固定 50% 折扣，每个档位都低于当年的官方账单。" },
      { q: "迁移期间可以新旧模型并行跑吗？", a: "可以。一把 apiToken.sale 密钥和余额覆盖所有受支持的 Claude 模型，你可以把一部分流量路由到新模型 ID、旧配置保持在线，并在仪表盘对比逐请求的 token 用量。" },
      { q: "我现有的 Claude 3.5 提示词在新模型上还能用吗？", a: "几乎都能用，因为提示词格式完全一致——但输出会变：指令执行更字面化，回答更详尽。针对 3.5 行为深度调过的提示词，在新 ID 上生产前先重测。" },
    ],
  };
