import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Kimi K3 对比 Kimi for Coding：上下文、推理与价格",
    h1: "Kimi K3 对比 Kimi for Coding：什么负载选哪个模型",
    description: "Kimi K3 对比 Kimi for Coding：公开别名、256K 与 1M 上下文、可调推理强度与常开思考、High Speed 的双倍费率，以及一套两级路由策略。",
    keywords: ["kimi k3 对比 kimi for coding", "kimi k3 api", "kimi for coding 价格", "kimi 编程模型怎么选", "kimi k3 256k 与 1m 区别", "kimi highspeed 值得买吗", "kimi 模型对比", "kimi k3 reasoning effort", "kimi k2.7 code", "编程代理选哪个 kimi 模型"],
    dek: "Kimi K3 是推理与长上下文家族，Kimi for Coding 是常开思考的低成本编程家族。这篇 Kimi K3 对比 Kimi for Coding 的文章逐一映射每个公开别名——上下文窗口、推理控制和每 token 费率——最后给出一套路由策略：日常编辑走便宜模型，难题或超大任务升级到 K3。",
    sections: [
      { h2: "先说结论：按次编辑成本 vs 推理余量", blocks: [
        { type: "p", text: "把 Kimi for Coding 当作默认编程模型，任务超出它的能力时再升级到 K3。Kimi for Coding 按每百万 token 计费：缓存命中 $0.19、缓存未命中 $0.95、输出 $4——是已公开 Kimi 系列里最低的通用编程费率；K3 则是 $0.30 / $3 / $15，换来的是 1M 上下文模式和 low、high、max 三档显式推理强度控制。两个家族用同一把 apiToken.sale 密钥都能调用，所以这是按请求的路由决策，不是账户决策。" },
        { type: "p", text: "大多数团队最终会落在这样的分工上：接近自动补全的编辑、测试生成、小型重构和高频代理循环交给 Kimi for Coding；整仓分析、长文档处理和需要可见推敲过程的难题交给 K3。High Speed 买的是延迟，不是能力——它服务的是同一个编程模型，token 费率恰好翻倍。" },
      ] },
      { h2: "别名映射：上下文窗口与思考模式", blocks: [
        { type: "table", headers: ["公开别名", "上下文", "推理控制", "最适合"], rows: [
          ["kimi/kimi-for-coding", "256K", "始终开启 thinking", "日常编程与经济型代理循环"],
          ["kimi/kimi-for-coding-highspeed", "256K", "始终开启 thinking", "对延迟敏感、速度值回票价的编程场景"],
          ["kimi/k3-256k", "256K", "low / high / max 强度，默认 high", "需要 K3 推理但不进完整上下文模式"],
          ["kimi/k3 · kimi/k3[1m]", "1M", "low / high / max 强度，默认 high", "长代码库、长文档与高难推理"],
        ] },
        { type: "p", text: "k3[1m] 是 K3 1M 模式的兼容写法，不是单独定价的模型。路由器会把它规范化为提供商真实的 k3 线上模型，所以 kimi/k3 和 kimi/k3[1m] 产生的是同样的流量、同样的账单。" },
        { type: "p", text: "256K 形态比看上去更重要。如果任务装得进 256K token，k3-256k 让你用上 K3 的推理控制，又不必把请求押进 1M 上下文模式——对于「难但小」的问题，比如一个别扭的算法或一个棘手的并发 bug，这才是正确的默认值。" },
      ] },
      { h2: "每个请求实际花多少钱", blocks: [
        { type: "p", text: "Kimi 公布的是三段价格——缓存命中、缓存未命中和输出——而不是单一的输入价，而且缓存是自动的。重复前缀按命中价计费；新写入缓存的 token 按未命中计费，既不是免费，也不是隐藏的第四段。apiToken.sale 按实际服务的模型定价，并对每一段统一打五折：" },
        { type: "table", headers: ["别名", "官方 命中 / 未命中 / 输出（每 1M）", "固定五折之后"], rows: [
          ["kimi/k3 · k3-256k · k3[1m]", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
          ["kimi/kimi-for-coding", "$0.19 / $0.95 / $4", "$0.095 / $0.475 / $2"],
          ["kimi/kimi-for-coding-highspeed", "$0.38 / $1.90 / $8", "$0.19 / $0.95 / $4"],
        ] },
        { type: "note", text: "推理 token 是输出的子集，按输出费率结算，不会作为独立的 token 类别再收一次。所以同一个提示词，K3 用 max 强度跑可能比 low 强度明显更贵——差价体现在输出量上，而不是附加费。" },
        { type: "p", text: "这张表有个好用的读法：五折之后，High Speed 的价格正好等于基础版 Kimi for Coding 的官方价。如果你本来就打算按 Moonshot 的标价买基础模型，这里的低延迟版本花的是同样的钱。" },
      ] },
      { h2: "推理控制：可调强度 vs 常开思考", blocks: [
        { type: "list", items: [
          "K3 提供 low、high、max 三档推理强度，默认 high。便宜的探索性轮次调低强度，只在真正需要推敲的步骤调高。",
          "Kimi for Coding 和 High Speed 始终开启 thinking，不提供强度选项——每次调用都是这个家族固定的思考行为。",
          "在 Anthropic 通道上，none/off 的思考设置只应理解为关闭 K3 推理，而不是模型切换器：实测覆盖中这些轮次仍按 K3 费率计费。",
          "kimi-k2.6 不是可寻址的公开模型。不要试图通过调整推理参数去够到更老的一代。",
        ] },
        { type: "p", text: "这种不对称决定了成本账。Kimi for Coding 的常开思考摊在 $4 的输出费率里；K3 的可调强度摊在 $15 的输出费率里。为一个根本不需要 max 强度的任务付 K3 的输出价，是团队在这对模型上超支最常见的方式。" },
      ] },
      { h2: "High Speed 什么时候值回双倍费率", blocks: [
        { type: "p", text: "High Speed 的缓存命中、缓存未命中和输出费率恰好是基础版 Kimi for Coding 的两倍，底层模型完全相同。你买的就是延迟，仅此而已。这笔交易只在一种情况下合理：有人在等响应，而这个人的时间比 token 更贵。" },
        { type: "list", items: [
          "值得：交互式结对编程、编辑器内嵌的补全循环、现场演示。",
          "不值得：CI 测试生成、过夜重构批处理、评估扫描，以及任何排队或会重试的负载。",
          "永远不值得：你本来就打算交给 K3 的任务——High Speed 属于编程家族，不是更快的 K3。",
        ] },
      ] },
      { h2: "面向真实代理循环的两级路由策略", blocks: [
        { type: "p", text: "两个家族共用一把密钥、一个余额，路由器可以按调用拆分工作。实践中站得住的策略是：先估计请求的上下文规模和难度，便宜且小的发给 Kimi for Coding，大或难的带上明确强度升级到 K3：" },
        sourceBlock("kimi-k3-vs-kimi-for-coding", 5, 1),
        { type: "p", text: "终态 usage 对象是你的反馈回路。如果你路由到 K3 的负载持续返回很小的输出、输入又重度命中缓存，那它属于更便宜的别名；如果 Kimi for Coding 持续搞砸某一类任务，这一类就是落到了实处的升级规则。" },
      ] },
      { h2: "从实时目录固定别名，不要凭记忆", blocks: [
        { type: "steps", items: [
          "用你的密钥拉取按密钥过滤的目录：curl https://router.apitoken.sale/v1/models -H \"Authorization: Bearer sk-pool-•••\"。模型访问由目录驱动，所以这份响应——而不是任何博客文章，包括本篇——才是你的密钥能调用什么的唯一事实来源。",
          "在客户端配置里固定精确的别名字符串（kimi/k3-256k、kimi/kimi-for-coding……）。不带 kimi/ 命名空间的裸写法属于 Anthropic 通道；硬编码任何一种形式之前先查目录。",
          "给每个固定下来的别名发一个极小的探测请求，检查终态 usage。在放代理循环无人值守运行之前，确认计费模型和缓存分段与你的预期一致。",
          "把别名写进长期环境变量或 CI 变量之前，重新检查 /v1/models；定义可用性的是目录，而不是别名字符串。",
        ] },
        { type: "note", text: "不要请求 Kimi 的内部官方模型 ID。公开路由器流量使用目录里的订阅别名；kimi-k2.7-code 这类内部费率 ID 不是可接受的写法。" },
      ] },
      { h2: "两个家族背后是同一个预付费余额", blocks: [
        { type: "p", text: "没有按模型挑选的套餐。一把 apiToken.sale 密钥覆盖受支持的 Claude、GPT、Gemini 和 Kimi 目录，每个请求按官方费率减固定五折计量，费用从永不过期的预付费余额里扣。一个用 Kimi for Coding 跑量、用 K3 攻坚的团队，看到的是一个余额、一条发票轨迹和仪表板里的按请求用量。" },
        { type: "p", text: "因为余额永不过期，在两个家族之间拆分工作没有任何承诺风险：K3 重度使用的那周充的值，下个月照样按同样的折扣价支付 Kimi for Coding 的流量。" },
        { type: "note", text: "通过 Google 或 GitHub 注册的新账户自带 $5 平台赠金，可用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱/密码注册的账户没有赠金。" },
        { type: "link", text: "各 Kimi 模型的完整费率，含缓存分段", href: "/models" },
      ] },
    ],
    faq: [
      { q: "哪款 Kimi 模型最适合编程？", a: "Kimi for Coding 是经济型默认：官方价每百万 token 缓存命中 $0.19、缓存未命中 $0.95、输出 $4，固定五折后再减半。更难的推理或长上下文代码库工作升级到 K3；只有当更低延迟值回恰好双倍的基础费率时才用 High Speed。" },
      { q: "k3 和 k3[1m] 是不同模型吗？", a: "不是。两者选择的是同一个 K3 1M 模式；方括号形式是兼容别名，路由器会把它规范化为提供商真实的 k3 线上模型，也没有单独定价。" },
      { q: "k3-256k 和 k3 的区别是什么？", a: "上下文模式。k3-256k 在 256K 窗口内运行 K3 及其推理强度控制（low、high、max，默认 high）；k3 / k3[1m] 则启用 1M 上下文模式，面向长代码库和长文档。" },
      { q: "Kimi for Coding High Speed 是更聪明的模型吗？", a: "不是。它是同一个编程模型，以更低延迟提供服务，缓存命中、未命中和输出费率恰好翻倍。有人在等响应时买它；批处理和 CI 工作不要买。" },
      { q: "能通过路由器请求 Kimi 的内部官方模型 ID 吗？", a: "不能。使用按密钥过滤的 GET /v1/models 目录返回的公开订阅别名。kimi-k2.7-code 这类内部费率 ID 不被接受，kimi-k2.6 也不是可寻址的公开模型。" },
      { q: "Kimi 的推理 token 额外收费吗？", a: "它们作为输出 token 的子集按输出费率结算——Kimi for Coding 官方每百万 $4，K3 为 $15，折扣后减半——绝不会作为独立的 token 类别叠加收费。" },
    ],
  };
