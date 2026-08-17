import type { LocalizedContent } from "../learn";
import { BASE, KEY } from "../learn-shared";

export const content: LocalizedContent = {
    title: "如何节省 Claude API token",
    h1: "如何节省 Claude API token",
    description: "用提示词缓存、按任务路由模型和精简上下文来节省 Claude API token。这些实战技巧都基于真实的请求结构，并可与 apiToken.sale 的折扣叠加。",
    keywords: ["节省 claude api token", "降低 claude api 成本", "claude 提示词缓存", "claude api 优化", "降低 claude api 账单", "claude api token 用量", "claude api 成本优化", "最便宜的 claude 模型", "claude max_tokens", "anthropic api cache control"],
    dek: "节省 Claude API token 归根结底是三个杠杆：少发输入 token、少生成输出 token，再通过选模型和提示词缓存降低每 token 的单价。每个杠杆都是对你已经在发的请求做一处具体改动——而且它们都能与 apiToken.sale 的折扣叠加，后者把等式中价格那一侧直接砍掉一半。",
    sections: [
      { h2: "Claude API 的 token 到底花在哪里", blocks: [
        { type: "p", text: "你的 Claude API 账单就是输入 token 加输出 token，按模型分别计量。价格表里的两个事实告诉你该往哪儿优化：在每一个现役 Claude 模型上，输出 token 的价格都是输入的五倍；而多轮对话每次调用都会把整段历史当作新输入重新发送。所以最大的节省来自三件事：少生成输出、少重发上下文，以及让更便宜的 token 类别——缓存读取和更小的模型——承担更多工作。" },
        { type: "table", headers: ["模型", "官方 输入 / 输出（每 1M token，美元）", "本站价格（−50%）"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "p", text: "这张表要当作路由表来读，而不只是价目表。无论输入还是输出，Haiku 每 token 都比 Opus 便宜五倍；统一的 50% B2C 折扣把每一行都减半，但并不改变排序——所以选对模型在这里省下的比例，和在官方价格下完全一样。" },
      ] },
      { h2: "缓存每次请求都要重发的上下文", blocks: [
        { type: "p", text: "对于任何带稳定重复前缀的场景——长系统提示词、工具定义、大参考文件——提示词缓存是单项最大的 token 节省手段。你在稳定块的末尾标一个 cache_control 断点：第一次调用写入缓存（单独计量），之后的调用以新输入价格的一小部分读回。一条缓存大约存活五分钟，每次读取都会续期，所以活跃会话的缓存可以一直保温。" },
        { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "system": [\n      {\n        "type": "text",\n        "text": "<your long, stable system prompt>",\n        "cache_control": {"type": "ephemeral"}\n      }\n    ],\n    "messages": [{"role":"user","content":"Refactor the parser."}]\n  }'` },
        { type: "note", text: "缓存匹配是前缀精确的：提示词靠前位置改一个字节，断点之后的一切都会按新输入重新计费。把易变的内容——时间戳、用户状态、当前问题——放在缓存断点之后，永远别放在它前面。" },
      ] },
      { h2: "把每个请求路由到能胜任的最便宜模型", blocks: [
        { type: "p", text: "把所有请求都发给 Opus 是 Claude API 使用中最贵的习惯。大部分生产流量——分类、抽取、格式化、自动补全式的编辑、解析工具返回——根本不需要前沿推理能力，为它们付 Opus 的价格纯属浪费。让模型匹配任务，每 token 的差价会自动替你省钱。" },
        { type: "steps", items: [
          "新负载默认用 claude-sonnet-5——日常编码和写作的均衡档。",
          "把高并发或机械性工作下放给 claude-haiku-4-5：打标签、总结短输入、格式转换、简单问答。",
          "用升级代替默认：先跑便宜的模型，只有结果通不过你的校验时才用 claude-opus-4-8 重试。",
          "在智能体循环里，规划留在强模型上，解析和格式化步骤交给 Haiku 跑。",
        ] },
        { type: "p", text: "经典模式是级联：Haiku 先尝试请求，一个廉价的校验判断结果是否可接受，只有失败的才升级到 Sonnet 或 Opus。你只为真正需要前沿能力的那一小部分流量付前沿价格。" },
      ] },
      { h2: "少发上下文，少要输出", blocks: [
        { type: "p", text: "请求里的每个文件、每条消息、每个工具定义，每次调用都要重新计费；回复里的每个 token 都按 5 倍的输出价格计费。两头都精简不光彩但见效快——不需要任何平台功能，只需要对放进请求的东西有点纪律。" },
        { type: "list", items: [
          "只发任务真正需要的文件和历史；精准的节选胜过整个仓库的倾倒。",
          "把长会话总结成一份滚动简报，而不是每轮重发完整记录。",
          "删掉当前步骤用不到的工具定义——它们每轮都按输入计费。",
          "把 max_tokens 限制在响应真正需要的范围内；失控的补全会一直计费到最后一个 token。",
          "让模型返回 diff 或补丁，而不是重写整个文件；反正要解析回复的话就要 JSON——多余的散文按输出价格计费。",
        ] },
        { type: "p", text: "因为输出价格是输入的五倍，砍掉 1,000 个输出 token 省下的钱等于砍掉 5,000 个输入 token。拿不准时，先缩短你要的回答，再缩短你发的上下文。" },
      ] },
      { h2: "动手调优前，先读 usage 对象", blocks: [
        { type: "p", text: "每个 Messages API 响应的末尾都有 usage 字段，给出精确账目。优化之前，按功能或接口把这些数字记下来——对 token 去向的猜测通常是错的，而 usage 对象能把优化变成算术题。" },
        { type: "code", code: `"usage": {\n  "input_tokens": 1520,\n  "output_tokens": 212,\n  "cache_creation_input_tokens": 8134,\n  "cache_read_input_tokens": 0\n}` },
        { type: "p", text: "缓存热起来之后，你的大部分输入应当挪进 cache_read_input_tokens，而普通的 input_tokens 应收缩到只剩新问题本身。apiToken.sale 控制台为每个请求展示同样的 token 级明细，所以每做一处改动你都能立刻看到变化，而不必等到月底账单。" },
        { type: "link", text: "用免费计算器估算你的月度开销", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "token 纪律与折扣是相乘的", blocks: [
        { type: "p", text: "apiToken.sale 的计费按固定顺序进行：每次调用先按其精确的用量构成——输入、输出、缓存写入、缓存读取——折算成 Anthropic 官方花费，然后减去统一的 50% B2C 折扣，净额从你的预付余额中扣除。缓存和路由压缩的是官方花费；折扣再把剩下的部分减半。两者相乘，所以一个把 token 量减半的负载，实际成本只有官方价格的四分之一。" },
        { type: "p", text: "余额本身永不过期，充值接受任意整数美元金额，所以没有订阅时钟逼着你烧掉本想省下的 token。各模型的当前价格（含缓存定价）见模型页面。" },
        { type: "link", text: "当前模型阵容与各模型价格", href: "/models" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额，可用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "节省 Claude API token 最有效的单一手段是什么？", a: "对大而重复的上下文——系统提示词、文件、工具定义——使用提示词缓存，再配合把每个任务路由到能胜任的最便宜模型。缓存读取的成本只是新输入 token 的一小部分，而 Haiku 每 token 比 Opus 便宜五倍。" },
      { q: "怎么查看一次 Claude API 请求用了多少 token？", a: "每个响应都带一个 usage 对象，包含 input_tokens、output_tokens、cache_creation_input_tokens 和 cache_read_input_tokens。apiToken.sale 控制台展示同样的逐请求 token 明细。" },
      { q: "想省钱该用哪个 Claude 模型？", a: "日常工作默认 claude-sonnet-5，高并发的机械性任务下放给 claude-haiku-4-5（官方每 1M token $1/$5，折后 $0.50/$2.50），把 claude-opus-4-8 留给真正高难度的推理。" },
      { q: "设置 max_tokens 能降低我的 Claude API 账单吗？", a: "你按实际生成的输出 token 付费，所以收紧 max_tokens 上限能防止失控的补全一直计费到上限。如果回复以 stop_reason: max_tokens 结束，说明上限截断了回答——要有意地调高它，而不是用同一个请求重试。" },
      { q: "这些省 token 技巧能与 apiToken.sale 的折扣叠加吗？", a: "能。缓存和模型路由先减少按官方价格计费的 token 数量，然后统一的 50% B2C 折扣再把剩下的部分减半——节省效果相乘。" },
    ],
  };
