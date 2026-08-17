import type { LocalizedContent } from "../learn";
import { BASE, KEY } from "../learn-shared";

export const content: LocalizedContent = {
    title: "Claude API 最佳实践",
    h1: "Claude API 最佳实践",
    description: "在 apiToken.sale 上使用 Claude API 的实用最佳实践：模型选择、提示词缓存、流式输出、密钥终身累计消费上限、到期日期与安全的密钥管理。",
    keywords: ["claude api 最佳实践", "claude api 生产环境清单", "anthropic api 最佳实践", "claude api 模型路由", "claude api 提示词缓存", "claude api 流式输出", "claude api 429 错误处理", "claude api 密钥管理", "降低 claude api 成本", "claude api 使用技巧"],
    dek: "Claude API 最佳实践归根结底是两个杠杆：你发出多少 token，以及哪个模型来消耗它们。本指南涵盖模型路由、提示词缓存、流式输出、重试纪律和按密钥配置的防护栏——这些习惯能让 apiToken.sale 上的生产集成保持快速、省钱、安全。",
    sections: [
      { h2: "让模型去匹配任务，而不是反过来", blocks: [
        { type: "p", text: "Claude API 最可靠的一条最佳实践，就是别再把每个请求都发给最强的模型。把每次调用路由到能胜任工作的最便宜模型，缓存重复发送的上下文，有人在等的响应一律流式输出，失败时用退避重试而不是紧密循环。下面就是这几步背后的实操细节。" },
        { type: "table", headers: ["工作负载", "起步模型", "原因"], rows: [
          ["大批量分类、信息抽取、快速改写", "claude-haiku-4-5", "单 token 最快、最便宜；处理窄任务的质量绰绰有余"],
          ["日常编码、对话、智能体循环", "claude-sonnet-5", "默认主力——中档价格，推理能力强"],
          ["高难度重构、架构设计、目标模糊的长时间会话", "claude-opus-4-8", "全系顶配；留给 Sonnet 明显吃力的任务"],
        ] },
        { type: "p", text: "在 apiToken.sale 上，所有支持的模型共用一把 API 密钥和一个预付费余额，路由只是改请求里的模型 ID 这一行——不需要额外账户，也不需要按模型配置计费。无论任务落到哪个模型，所有提供商都统一享受 50% B2C 折扣，所以降级到 Haiku 或 Sonnet 是纯赚。" },
        { type: "p", text: "升级要刻意，而不是默认顶配。智能体里的常见模式：主循环跑 claude-sonnet-5，检测到失败信号（工具反复报错、自我纠正原地打转）后，只把那一步重新发给 claude-opus-4-8。这样你只为真正需要的少数步骤付 Opus 的价格，而不是整个会话。" },
        { type: "link", text: "编程场景的模型选型深度对比", href: "/docs/learn/best-claude-model-for-coding" },
      ] },
      { h2: "缓存每次调用都重复发送的上下文", blocks: [
        { type: "p", text: "如果你的请求带有一大段稳定的前缀——长系统提示词、工具定义、代码库摘要、few-shot 示例——提示词缓存是模型选择之外最大的成本杠杆。用 cache_control 标记可复用的块，API 就会把它们存起来：缓存写入比新输入略贵一点，但后续缓存读取只需新输入 token 价格的一小部分。" },
        { type: "code", code: `curl ${BASE}/v1/messages \\
  -H "x-api-key: ${KEY}" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "content-type: application/json" \\
  -d '{
    "model": "claude-sonnet-5",
    "max_tokens": 1024,
    "system": [
      {"type": "text", "text": "...20k tokens of stable instructions...",
       "cache_control": {"type": "ephemeral"}}
    ],
    "messages": [{"role": "user", "content": "Summarize ticket #4821"}]
  }'` },
        { type: "p", text: "缓存命中率取决于两条规则。第一，缓存前缀必须在多次调用之间逐字节一致——哪怕只是在系统提示词开头注入一个时间戳，也会让它之后的所有内容失效，所以把易变内容放在末尾。第二，默认的 ephemeral 缓存只存活几分钟，每次命中会刷新，所以它奖励高频往返的工作负载：智能体、聊天会话、复用同一上下文的批处理任务。" },
        { type: "note", text: "缓存不是免费存储。对长文档的一次性请求会白付缓存写入的溢价，却没有读取来摊薄它。只缓存你确定会在 TTL 窗口内至少重发两次的内容。" },
      ] },
      { h2: "凡是有用户在等的响应，都用流式输出", blocks: [
        { type: "p", text: "设置 stream: true 后，API 会通过 server-sent events 边生成边返回 token，而不是给你一个阻塞式的完整响应。流式和缓冲调用消耗的 token 完全一样，但感知延迟从“等整个答案”降到“等第一个 token”——通常不到一秒。对聊天界面来说，这就是转圈等待和秒回的区别。" },
        { type: "p", text: "流式对智能体同样重要。事件一到达就读取，能让你在工具调用的 JSON 块闭合的那一刻就开始解析，向用户展示进度，并在输出明显跑偏时提前中止——掐断流意味着你不再为那些本来就要丢弃的输出 token 付费。" },
        { type: "note", text: "使用流式时，权威的 token 用量在最后一个 message_delta 事件里，而不是开头。记录成本或更新预算前，一定要读取终态 usage——绝不要按字符数估算。" },
      ] },
      { h2: "429 和 5xx 要退避重试，绝不紧密循环", blocks: [
        { type: "p", text: "apiToken.sale 不公布固定的每分钟请求数表：429 表示网关或上游容量触顶，正确的回应是耐心，而不是加压。有 Retry-After 头就按它来；否则用指数退避加随机抖动重试，并且先降低客户端并发，再考虑提高请求速率。" },
        { type: "steps", items: [
          "捕获错误并分类。只重试 429 和 5xx；400、401 或 403 会以同样的方式永远失败，应该修请求或密钥，而不是重试。",
          "有 Retry-After 头就等它指定的时长；否则大约等 1s、然后 2s、4s、8s——每次翻倍并加随机抖动，避免多个并行 worker 同步重试。",
          "限制重试次数（通常三到五次），然后让任务显式失败。静默的无限重试既烧余额又掩盖故障。",
          "如果在你的正常负载下 429 持续出现，先降低并发，再联系支持团队申请持续更高的吞吐，而不是在工程上绕着走。",
        ] },
        { type: "link", text: "apiToken.sale 的速率限制、Retry-After 与吞吐", href: "/docs/learn/claude-api-rate-limits" },
      ] },
      { h2: "每个环境一把密钥，防护栏全部开启", blocks: [
        { type: "p", text: "为每个环境或应用创建一把单独、命名清晰的密钥——prod-backend、staging-ci、local-dev——而不是到处共用一把。密钥泄露时，你只吊销那一把，其余照常运行；共用密钥则意味着一次泄露就要紧急轮换所有客户端。" },
        { type: "p", text: "控制台为每把密钥提供两个防护栏，都值得设置：可选的终身累计消费上限，限制一把密钥最多能从余额中支取的总额；以及到期日期，过了这天密钥直接失效。把终身上限设成该环境合理消耗的额度，给短周期项目配短生命周期的密钥。" },
        { type: "list", items: [
          "密钥放在密钥管理器或环境变量里——绝不进源码仓库、客户端代码或工单。",
          "任何接触过公开场所的密钥（一次提交、一行日志、一张截图）都按已泄露处理：先吊销，再排查。",
          "把每次请求的 max_tokens 限制在响应实际所需，避免失控的提示词把单次调用的成本吹大。",
        ] },
        { type: "link", text: "完整的密钥卫生手册", href: "/docs/learn/claude-api-key-security" },
      ] },
      { h2: "审计 token 明细，而不只是看余额", blocks: [
        { type: "p", text: "apiToken.sale 控制台里的每个请求都按模型、提供商和 token 类别逐项列出——输入、输出和缓存部分。每周看一次这份明细。成本回退几乎总是最先在这里露头：有人开始重发完整历史导致输入 token 攀升，max_tokens“以防万一”被调大导致输出 token 膨胀，提示词顺序调整后缓存读取暴跌。" },
        { type: "p", text: "经济上你也是占优的。请求按提供商的精确费率计量，再叠加统一的 50% B2C 折扣，净额从永不过期的预付费余额中扣除——所以你通过缓存、路由和收紧上下文省下的每个 token，同时也是你没为它付全价的 token。token 技巧减少数量，折扣降低单价，两者相乘。" },
        { type: "link", text: "动手前先用成本计算器估算工作负载", href: "/tools/claude-api-cost-calculator" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台奖励余额——适用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱/密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "最重要的 Claude API 最佳实践是什么？", a: "把每个任务路由到能胜任的最便宜模型，用 cache_control 缓存大而稳定的上下文，面向用户的响应一律流式输出，对 429/5xx 按 Retry-After 和指数退避重试，并为每个环境配一把密钥、设好终身累计消费上限和到期日期。" },
      { q: "默认应该用哪个 Claude 模型？", a: "日常编码和对话从 claude-sonnet-5 起步，大批量简单任务交给 claude-haiku-4-5，claude-opus-4-8 留给 Sonnet 明显吃力的任务。在 apiToken.sale 上三者共用一把密钥和余额，切换只是改一行模型 ID。" },
      { q: "如何在生产环境降低 Claude API 成本？", a: "缓存重复的上下文（缓存读取只需新输入 token 价格的一小部分），把简单任务降级到更便宜的模型，限制 max_tokens，并每周复查 token 级用量明细。在 apiToken.sale 上，这些手段还能与统一的 50% B2C 折扣叠加。" },
      { q: "Claude API 返回 429 时该怎么办？", a: "优先遵守 Retry-After 头，否则用指数退避加抖动重试，并降低并发。400、401 这类 4xx 错误绝不重试——去修请求或密钥。需要持续更高吞吐时，联系支持团队。" },
      { q: "流式输出会更费 token 吗？", a: "不会。stream: true 只是把同样的 token 通过 server-sent events 增量送达；终态 message_delta 事件携带权威用量。无论哪种方式你都只为生成的 token 付费——流式改变的只是你何时看到它们。" },
      { q: "Claude API 密钥应该怎么存放和管理？", a: "密钥放在密钥管理器或环境变量里，绝不进 git 或客户端代码。为每个环境创建命名密钥，在控制台设置其终身累计消费上限和到期日期，密钥一旦暴露立即吊销。" },
    ],
  };
