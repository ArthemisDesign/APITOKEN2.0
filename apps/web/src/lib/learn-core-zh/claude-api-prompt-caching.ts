import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API 提示词缓存（Prompt Caching）",
    h1: "Claude API 提示词缓存：工作原理与实际节省",
    description: "Claude 提示词缓存把复用的上下文存起来，让重复请求按缓存读取价而不是完整输入价计费。本文讲清断点、TTL、价格计算，以及如何与 apiToken.sale 的折扣叠加。",
    keywords: ["claude 提示词缓存", "claude api 缓存", "anthropic prompt cache", "cache_control claude api", "claude 缓存读取价格", "claude api 缓存断点", "anthropic messages api 缓存", "提示词缓存降低 claude api 成本", "claude 缓存 ttl", "claude api base url", "claude api key"],
    dek: "Claude 提示词缓存让你把稳定的上下文——系统提示词、工具定义、参考文件——标记出来，重复请求直接从缓存读取，只花输入价零头的钱，而不是每次全价重算。本文涵盖断点设置、缓存 TTL、写入与读取的价格计算，以及缓存用量在 apiToken.sale 账单上的呈现方式。",
    sections: [
      { h2: "提示词缓存对 Claude API 账单的影响", blocks: [
        { type: "p", text: "提示词缓存让 Anthropic 存储请求中可复用的前缀——也就是你设置的断点之前的所有内容——之后携带相同前缀的请求直接从缓存读取，不再重新处理。缓存读取的价格只有全新输入 token 的一个零头，而缓存写入比输入价略高一点。如果你的应用反复发送同一段大上下文（系统提示词、代码库快照、文档集），缓存能把每次调用中最贵的部分变成最便宜的部分。" },
        { type: "p", text: "缓存写入和缓存读取在 API 响应和账单中作为独立的 token 桶分别计量，所以缓存到底省了多少一目了然。响应本身没有任何变化——同样的模型、同样的质量、同样的流式行为。" },
      ] },
      { h2: "在请求中放置 cache_control 断点", blocks: [
        { type: "p", text: "按请求开启：在某个内容块上加一个 cache_control 标记即可。标记之前的所有内容——系统提示词、工具定义、更早的消息——都会成为可缓存的前缀。下面是一个发往 apiToken.sale 的真实请求，缓存了系统提示词和工具：" },
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "system": [\n      {\n        "type": "text",\n        "text": "You are a senior reviewer... (long stable instructions)",\n        "cache_control": {"type": "ephemeral"}\n      }\n    ],\n    "messages": [{"role": "user", "content": "Review this diff: ..."}]\n  }'` },
        { type: "list", items: [
          "每个请求最多可以设置四个 cache_control 断点——常见的布局是工具之后一个、系统提示词之后一个、大参考文档之后一个。",
          "缓存从第一个 token 开始做精确的前缀匹配。系统提示词里改动一个字符，其后的内容就全部 miss。",
          "只有达到最小可缓存尺寸的块才会被存储——Sonnet 和 Opus 模型约为 1,024 token，Haiku 更高。太短的提示词会静默跳过缓存。",
          "易变内容（时间戳、用户特定数据）要放在最后一个断点之后，绝不要放进前缀里。",
        ] },
      ] },
      { h2: "缓存写入与缓存读取的定价", blocks: [
        { type: "p", text: "Anthropic 按模型输入 token 单价的倍数为缓存操作定价。写入是每个缓存块一次性的小幅溢价；读取才是真正把钱省回来的环节。在 apiToken.sale 上，固定的 50% B2C 折扣会按官方消费金额计算后作用于每一条用量，缓存用量也不例外。" },
        { type: "table", headers: ["用量项", "官方费率（× 输入价）", "本站实际（−50%）"], rows: [
          ["全新输入 token", "1×", "0.5×"],
          ["缓存写入，5 分钟 TTL", "1.25×", "0.625×"],
          ["缓存写入，1 小时 TTL", "2×", "1×"],
          ["缓存读取", "0.1×", "0.05×"],
        ] },
        { type: "p", text: "缓存条目默认存活五分钟，每次命中都会重置计时器，所以活跃会话可以无限期保持缓存热度。对于突发型负载，可以以更高的写入成本选择一小时 TTL。缓存读取的价格是全新输入的十分之一——叠加折扣后是二十分之一——所以一个前缀在 TTL 内被读取三次，就已经比全新发送两次更便宜。" },
        { type: "note", text: "缓存条目不跨账户共享，也绝不会在 apiToken.sale 的客户之间泄漏。你的缓存前缀只能被同一上游账户上下文下鉴权的请求复用。" },
      ] },
      { h2: "哪些负载能命中缓存——哪些永远命中不了", blocks: [
        { type: "list", items: [
          "每轮都重发相同代码库上下文、CLAUDE.md 和工具 schema 的编程代理和 IDE 助手。",
          "针对固定文档集做查询的 RAG 流水线——缓存整个语料库，只变化问题。",
          "带长而稳定的系统提示词和 few-shot 示例库的聊天机器人。",
          "针对一个大指令块对大量短条目做分类或抽取的批处理任务。",
        ] },
        { type: "p", text: "一次性问题、每次都变的提示词、低于最小尺寸的前缀，缓存都帮不上忙。如果每个请求确实都是独一无二的，你只会白付写入溢价而永远收不到读取——全量开启之前先实测。" },
      ] },
      { h2: "在 usage 对象中确认缓存命中", blocks: [
        { type: "p", text: "每个 Messages API 响应都会在 usage 块里直接报告缓存用量。缓存命中时看起来是这样的：" },
        { type: "code", code: `"usage": {\n  "input_tokens": 38,\n  "cache_creation_input_tokens": 0,\n  "cache_read_input_tokens": 14802,\n  "output_tokens": 412\n}` },
        { type: "p", text: "持续观察各请求中的 cache_read_input_tokens：健康的集成在首次调用之后，大部分上下文都会落在这个字段里，而 cache_creation_input_tokens 在 TTL 到期前都接近零。在 apiToken.sale 上，同样的用量明细会出现在你的控制台中——每个请求都列出模型、提供商和 token 级拆分，每一条缓存行都能在用量详情里看到，节省是可审计的，而不是靠推测。" },
      ] },
      { h2: "缓存与预付费折扣的叠加", blocks: [
        { type: "p", text: "缓存降低你按全价支付的 token 数量；apiToken.sale 的折扣降低每个 token 的单价。两者是相乘关系。以 Claude Sonnet 5 为例（官方输入价 $3 / 1M token）：全新重发 100,000 token 的上下文，每次调用花 $0.30。走缓存读取只要 $0.03，再叠加固定的 50% B2C 折扣，这次调用的上下文部分只需 $0.015——相比过去占账单大头的部分，降了 20 倍。" },
        { type: "p", text: "计费保持预付费、简单透明：一个余额覆盖支持的 Claude、GPT、Gemini 和 Kimi 模型，每个模型先按官方价格表计量，再应用折扣。充值一次，用好缓存的负载能让同样的余额跑得远比无缓存流量更久。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额——适用于支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
        { type: "link", text: "各模型的输入、输出与缓存费率", href: "/models" },
        { type: "link", text: "在 Claude API 成本计算器中模拟缓存负载", href: "/tools/claude-api-cost-calculator" },
      ] },
    ],
    faq: [
      { q: "Claude 缓存读取便宜多少？", a: "缓存读取按模型输入 token 价格的 0.1× 计费，缓存写入为 1.25×（5 分钟 TTL）或 2×（1 小时 TTL）。在 apiToken.sale 上还会叠加固定的 50% B2C 折扣，缓存读取最终只有官方输入价的 0.05×。" },
      { q: "Claude 提示词缓存能存多久？", a: "缓存条目默认存活五分钟，每次命中都会重置计时器，所以活跃会话可以无限期保持热度。突发型流量可以按更高的写入费率选择一小时 TTL。" },
      { q: "为什么我的 Claude 提示词缓存没有命中？", a: "常见原因：前缀变了（缓存从第一个 token 开始匹配，任何改动都会使其后内容失效）、块低于最小可缓存尺寸（Sonnet 和 Opus 模型约 1,024 token）、两次调用之间超过了五分钟 TTL，或 cache_control 加在了每个请求都会变化的块上。" },
      { q: "提示词缓存在 apiToken.sale 上能用吗？", a: "可以。把带 cache_control 的标准 Messages API 请求发到 https://router.apitoken.sale/v1/messages，在 x-api-key 头里带上你的 sk-pool-… 密钥即可。缓存创建和读取按 Anthropic 官方费率计量，然后再应用你的折扣。" },
      { q: "缓存 token 还会扣我的预付余额吗？", a: "会，但按缓存费率扣：缓存写入为输入价的 1.25–2×，读取为 0.1×，先折算成 Anthropic 官方消费金额，再减去固定的 50% B2C 折扣。每个请求的缓存用量都能在控制台的用量拆分中看到。" },
    ],
  };
