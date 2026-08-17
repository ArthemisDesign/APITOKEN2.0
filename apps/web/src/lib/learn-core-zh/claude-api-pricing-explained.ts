import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API 定价详解",
    h1: "Claude API 定价是如何运作的",
    description: "读懂 Claude API 定价：按 token 分别计费的输入与输出费率、提示词缓存，以及 apitoken.sale 如何套用统一 50% 折扣。",
    keywords: ["claude api 定价", "claude api 每 token 费用", "claude token 价格", "claude api 多少钱", "anthropic api 定价详解", "claude api 价格是多少", "claude opus 每百万 token 价格", "claude sonnet api 价格", "claude api 提示词缓存费用", "claude api 计费方式"],
    dek: "Claude API 按 token 计量计费：输入和输出各有一套费率，缓存读取更便宜，没有按请求收取的费用。本文把账算到底——token 数、各模型费率表、缓存与思考——并说明 apitoken.sale 的统一 50% 折扣在计算的哪一步生效。",
    sections: [
      { h2: "你真正付费的东西：输入 token 和输出 token", blocks: [
        { type: "p", text: "Claude API 定价就是纯粹的按 token 计量。每次请求要为你发进去的 token 付费——提示词、系统指令、对话历史、工具定义——也要为模型生成回来的 token 付费。输入和输出各有一套费率，输出更贵，没有按请求计费、没有席位授权费、也没有最低消费。把两个 token 数分别乘上模型的两档费率，就是一次调用的确切成本。" },
        { type: "p", text: "一个 token 大约是四分之三个英文单词、四个字符左右。代码、JSON 和非英文文本的 token 密度更高，所以同样长度下，一个源文件花的 token 比散文多。输出定价更高是有机械原因的：输入是一次性并行处理的，而每个输出 token 都要模型单独跑一遍前向计算。" },
        { type: "p", text: "这些你都不用估。Messages API 会在响应的 usage 对象里精确报告一次调用消耗了多少——流式请求则在终止事件里给出：" },
        { type: "code", code: `"usage": {
  "input_tokens": 12480,
  "cache_read_input_tokens": 0,
  "output_tokens": 1523
}` },
        { type: "p", text: "以 Claude Sonnet 5 为例：每百万输入 token $3、每百万输出 token $15。上面这次调用的成本是 12,480 × $3/M + 1,523 × $15/M ≈ $0.0374 + $0.0228 ≈ $0.06（按官方费率）。这是对权威数字做算术，不是拍脑袋估算——所以做预算时，usage 对象才是事实来源，而不是你提示词的字符数。" },
      ] },
      { h2: "按模型划分的 Claude API token 定价", blocks: [
        { type: "p", text: "Anthropic 把产品线分成三档：Opus 是高端档，面向高难度推理和长重构；Sonnet 是均衡的默认选择，适合日常写代码；Haiku 最便宜，适合高并发、低复杂度的活儿。费率随能力递增——Opus 4.8 和 Haiku 4.5 在输入侧差五倍——所以把每个任务路由到「能胜任的最弱模型」，是你账单上最大的一根杠杆。" },
        { type: "table", headers: ["模型", "官方 输入 / 输出（$ / 1M）", "本站（−50%）"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "p", text: "第三列是同一张表套上 apitoken.sale 折扣之后的样子：B2C 统一 50% 折扣对每个模型都生效，所以排序不变，但每行都半价。智能体式编程循环对此感受最深——它们会串起几十轮调用，每一轮都把累积的上下文当作新输入重新发一遍，按 token 的费率很快就会滚成真金白银。" },
        { type: "link", text: "含缓存费率与上下文窗口的各模型页面", href: "/models" },
        { type: "link", text: "用免费成本计算器估算你的月度开销", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "提示词缓存、思考和流式对账单的影响", blocks: [
        { type: "p", text: "提示词缓存把缓存写入和缓存读取与新输入分开计量。在 Anthropic 官方费率表上，缓存写入比新输入略贵，而缓存读取大约是它的十分之一。该缓存的是长且稳定的前缀——系统提示词、工具定义、大型参考文件——在重复调用中它们几乎免费。" },
        { type: "list", items: [
          "思考 token 按输出计费。扩展思考生成的推理过程可能永远不会出现在回复里，但它全部落进输出桶，按完整的输出费率收费。",
          "流式和非流式请求计费完全相同。服务端推送事件改变的只是你什么时候看到 token，不是它们的价格。",
          "缓存读取只有命中才便宜。改了缓存前缀的中间部分，从修改点往后的内容全部失效，下一次调用又要按完整的输入价格付费。",
        ] },
        { type: "note", text: "缓存条目默认存活约五分钟，每次读取都会刷新计时器；也有更长的 1 小时 TTL，写入价格更高。间隔超过 TTL 的突发流量会反复支付写入溢价却吃不到便宜的读取——把相关调用批量打在一起，或者接受每个会话的第一次调用要按完整输入价重新预热缓存。" },
      ] },
      { h2: "统一 50% 折扣在计算的哪一步生效", blocks: [
        { type: "p", text: "apitoken.sale 不改变上面任何机制。你的请求打到的是同一个 Anthropic Messages API，回答它的是同样的模型 ID，usage 对象报告的也是同样的 token 数。变的只是结算：每次调用先换算成官方 Anthropic 消费，然后在动你的预付费余额之前，先减去 B2C 统一 50% 折扣。没有订阅费，也没有加价——折扣本身就是定价。" },
        { type: "p", text: "第一节的例子算到底就直观了：那次 $0.06 的 Sonnet 5 调用，结算时是 $0.03。同一个预付费余额也按各自的官方费率表计量受支持的 GPT、Gemini 和 Kimi 模型，同样套用这层折扣。" },
        { type: "p", text: "每次请求都会带着 token 级明细出现在你的控制台里——模型、输入、输出和缓存各桶——所以你可以拿真实流量来核对本文的算术，而不是等到月底发票来了才认账。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额，可用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "Claude API 是怎么定价的？", a: "按 token 计费，输入和输出分开、各有一套费率，更便宜的缓存读取单独成桶。模型越大每 token 越贵——从 Haiku 4.5 的每百万 token $1/$5 到 Opus 4.8 的 $5/$25。" },
      { q: "Claude API 每百万 token 多少钱？", a: "官方费率从 Claude Haiku 4.5 的输入 $1 / 输出 $5 到 Claude Opus 4.8 的 $5 / $25，Sonnet 是 $3 / $15。在 apitoken.sale 上，这些数字全部被 B2C 统一 50% 折扣减半。" },
      { q: "apitoken.sale 的折扣如何套用到 Claude API 定价？", a: "每次请求先按真实 token 数换算成官方 Anthropic 消费，再减去统一 50% 折扣，净额从你的预付费余额中扣除。同样的机制也覆盖缓存读取和思考 token。" },
    ],
  };
