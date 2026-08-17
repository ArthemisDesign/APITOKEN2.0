import type { LocalizedContent } from "../learn";
import { BASE, KEY } from "../learn-shared";

export const content: LocalizedContent = {
    title: "Claude API 速率限制",
    h1: "理解 Claude API 的速率限制",
    description: "apiToken.sale 上的 429 意味着什么、Claude API 如何按每分钟请求数与 token 数计量限流，以及如何借助 Retry-After 和指数退避正确重试。",
    keywords: ["claude api 速率限制", "claude api 429", "claude api 限流", "anthropic rate limit", "claude api rate limit exceeded", "claude api 每分钟请求数", "claude api 每分钟 token 数", "retry-after 响应头", "指数退避 抖动", "claude api 429 错误怎么处理", "rate_limit_error"],
    dek: "Claude API 的速率限制是按分钟计的请求与 token 吞吐上限，触限后返回的是 HTTP 429 而不是补全结果。本指南教你如何读懂这个响应、写出尊重 Retry-After 的重试策略，以及如何区分吞吐限制和 apiToken.sale 密钥上的消费护栏。",
    sections: [
      { h2: "Claude API 的速率限制到底是什么", blocks: [
        { type: "p", text: "Claude API 的速率限制是吞吐上限：你的账户每分钟最多能发出多少请求、推过多少 token。一旦超过，API 返回的不是补全结果，而是 HTTP 429。apiToken.sale 不公布固定的 RPM 表——这里的 429 表示网关或上游容量受限，持久的解法是守规矩的重试加上降低并发，而不是在配置文件里调大一个数字。" },
        { type: "p", text: "在 Anthropic 官方 API 上，限流按三个维度计量：每分钟请求数、每分钟输入 token 数、每分钟输出 token 数，以组织为单位统计。直连官方时，这些上限会随着累计消费增长沿用量等级（usage tiers）上调。三个计数器每分钟都会重置——这就是为什么三十秒的突发流量可能失败，而你的小时均值看起来微不足道。" },
      ] },
      { h2: "读懂 429 响应", blocks: [
        { type: "p", text: "写得好的客户端会把 429 当作数据，而不是失败。响应体里带有 rate_limit_error 类型的错误，响应通常还带有 retry-after 头，标明服务器希望你等待的秒数。" },
        { type: "code", code: `curl -i ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 64,\n    "messages": [{"role":"user","content":"hi"}]\n  }'\n\n# when throttled:\n# HTTP/2 429\n# retry-after: 17\n# {"type":"error","error":{"type":"rate_limit_error","message":"..."}}` },
        { type: "note", text: "retry-after 只是建议，不是契约，也不是每个 429 都带这个头。头部缺失时，退回带抖动的指数退避——并且绝不要用紧凑循环重试 429，那只会加剧造成限流的拥塞。" },
      ] },
      { h2: "能扛住生产环境的重试策略", blocks: [
        { type: "steps", items: [
          "把突发流量排队，而不是一次性打出去：每个 worker 一次只发一个请求。",
          "收到 429 时读取 retry-after，至少睡够它指定的秒数。",
          "头部缺失时，睡眠 base × 2^attempt 再加随机抖动，封顶约 30 秒。",
          "重试 4–6 次后停手，让任务明确失败，而不是无限重试。",
          "记录模型、等待时长和重试次数——如果 429 持续出现，你就有规律可以拿给支持团队看。",
        ] },
        { type: "code", code: `const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));\n\nasync function callClaude(body: unknown): Promise<Response> {\n  for (let attempt = 0; attempt < 5; attempt++) {\n    const res = await fetch("${BASE}/v1/messages", {\n      method: "POST",\n      headers: {\n        "x-api-key": process.env.APITOKEN_KEY!,\n        "anthropic-version": "2023-06-01",\n        "content-type": "application/json",\n      },\n      body: JSON.stringify(body),\n    });\n    if (res.status !== 429 && res.status < 500) return res;\n    const retryAfter = Number(res.headers.get("retry-after"));\n    const wait = retryAfter > 0\n      ? retryAfter * 1000\n      : Math.min(1000 * 2 ** attempt, 30_000) * (0.5 + Math.random() / 2);\n    await sleep(wait);\n  }\n  throw new Error("Claude API still rate-limited after 5 attempts");\n}` },
      ] },
      { h2: "为什么突发流量先于平均值触限", blocks: [
        { type: "p", text: "因为计数器按分钟计，并发才是真正的杠杆。在一分钟的开头并发打出五十个请求，即使接下来一小时什么都不发，也可能耗尽请求预算。token 计数器会放大这个效应：每一个并发的长生成在运行期间都在持续消耗输出 token 预算，所以十个并行的 4,000 token 回答对限流的压力，远大于十个快速短答。" },
        { type: "p", text: "流式不改变任何计量逻辑。流式调用仍然是一个请求，按与非流式完全相同的输入输出 token 计量和计费——它只是让你更早渲染 token，并在智能体拿到所需内容时提前中止。" },
      ] },
      { h2: "吞吐限制不是消费限制", blocks: [
        { type: "p", text: "这里有两套容易被混淆的系统。速率限制是流量整形器：临时的、按分钟计的、靠等待就能解决。消费护栏是预算刹车：它决定一个密钥总共能花多少钱。apiToken.sale 的控制台完全不提供请求吞吐配置——它提供的按密钥护栏只有可选的终身累计消费上限和到期日期。429 说的是“慢一点”，与你的余额无关，充值也解不了它。" },
        { type: "table", headers: ["", "吞吐限制", "密钥消费护栏"], rows: [
          ["限制对象", "每分钟请求数与 token 数", "单个密钥的终身累计消费"],
          ["表现形式", "HTTP 429 与 rate_limit_error", "密钥到达设定上限后停止消费"],
          ["所在位置", "网关与上游容量", "apiToken.sale 控制台，按密钥设置"],
          ["正确应对", "Retry-After、退避、降低并发", "有意识地调高或移除上限"],
        ] },
      ] },
      { h2: "不提额也能降低 429 压力", blocks: [
        { type: "list", items: [
          "给定时任务和批处理任务加随机偏移，避免它们在同一分钟扎堆。",
          "给 worker 并发设上限，让队列吸收突发流量。",
          "精简上下文，让每个请求携带更少的输入 token。",
          "把 max_tokens 限制在响应真正需要的范围。",
          "用提示词缓存（prompt caching）缓存大而稳定的上下文，降低重复请求的计费输入成本。",
        ] },
        { type: "p", text: "大多数 429 风暴是自找的：没有抖动的重试循环、把 worker 数翻倍的部署、整点（:00）扇出的定时任务。先把流量形状修整好，再考虑要不要更高的上限。" },
      ] },
      { h2: "当 429 变成容量问题", blocks: [
        { type: "p", text: "如果流量已经整形、重试也写对了，在目标负载下仍然规律出现 429，那就是容量问题，而不是代码问题。联系支持时说明你使用的模型、目标的每分钟请求数与 token 数、以及负载形态——持续更高的吞吐走账户沟通解决，没有自助滑块。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额——适用于支持的 Claude、GPT、Gemini 与 Kimi 模型；邮箱密码账户不参与。" },
      ] },
    ],
    faq: [
      { q: "Claude API 的速率限制是多少？", a: "直连 Anthropic 时，限制为每分钟请求数加上每分钟输入、输出 token 数，按组织统计，并随累计消费沿用量等级上调。apiToken.sale 不公布固定的 RPM 表；这里的 429 反映网关或上游容量，用 Retry-After 和退避处理。" },
      { q: "Claude API 的 429 错误怎么解决？", a: "有 retry-after 头就按它等待，否则用带抖动的指数退避，并降低并发。如果做完这些后生产负载下 429 仍持续出现，联系支持沟通持续更高的吞吐。" },
      { q: "429 限流错误会扣费吗？", a: "被 429 拒绝的请求在生成之前就失败了，不产生任何 token，也没有可计量的用量。只有完成的调用才会扣减你的预付余额。" },
      { q: "流式输出会占用更多速率限制吗？", a: "不会。流式响应只是一个请求，计量和计费与非流式完全一致；流式改变的只是你看到 token 的时机。" },
      { q: "我能在 apiToken.sale 密钥上设置每分钟请求数限制吗？", a: "不能。请求吞吐不是按密钥可配置的项。控制台的按密钥护栏是可选的终身累计消费上限和到期日期。" },
      { q: "Claude API 的 Retry-After 头是什么？", a: "它是服务器建议你在下次尝试前等待的秒数。把它当作下限；头部缺失时，改用带抖动的指数退避。" },
    ],
  };
