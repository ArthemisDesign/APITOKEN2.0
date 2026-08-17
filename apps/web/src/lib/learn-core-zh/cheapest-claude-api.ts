import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "最便宜的 Claude API——统一 50% 折扣",
    h1: "使用 Claude API 最便宜的方式",
    description: "最便宜的 Claude API：一模一样的 Anthropic Messages API，比官方消费统一低 50%。预付余额、无订阅，所有 Claude 模型均享折扣。",
    keywords: ["最便宜的 claude api", "claude api 折扣", "便宜的 claude api", "claude api 5折", "claude api 半价", "比 anthropic 便宜的 claude api", "claude api 充值额度", "claude api 预付余额", "claude api 价格", "cheapest claude api", "claude api 50% off"],
    dek: "最便宜的 Claude API 不是更小的模型，也不是被限速的山寨版——而是同一套 Anthropic Messages API，统一按官方费率打五折。apiToken.sale 按 Anthropic 公布的 token 价格计量每次请求，把结果减半，再从永不过期的预付余额中扣除。本页给出各模型的折后价格、计费管道的工作方式，以及如何把现有客户端指向它。",
    sections: [
      { h2: "简短答案：同一套 API，账单减半", blocks: [
        { type: "p", text: "使用 Claude API 最便宜的方式，就是通过 apiToken.sale 以统一 50% 的折扣购买同一套 API。你向同一套 Anthropic Messages API 发送同样的请求、用同样的模型 ID、拿到同样的响应——唯一变化的是这次调用花掉你多少钱。没有更便宜的替代模型，没有伪装成折扣的加价，也没有需要解锁的档位。" },
        { type: "p", text: "机制刻意做得很枯燥。每次请求都按 Anthropic 官方 token 费率计量，和你直接调用 Anthropic 一模一样。然后减去统一的 50% 折扣，只把净额从你的预付余额中扣除。一次按官方费率要花 $0.20 的调用，在这里只扣 $0.10。" },
        { type: "list", items: [
          "B2C 账户的每个请求都统一享受比官方消费低 50% 的折扣——无需解锁，没有用量门槛。",
          "折扣对输入、输出和缓存 token 一视同仁，所以你的负载形态永远不会改变这个百分比。",
          "B2B 批量定价在公开 B2C 费率之外单独商议。",
        ] },
      ] },
      { h2: "套用 50% 折扣后的 Claude API 价格", blocks: [
        { type: "p", text: "Anthropic 按每百万 token 为 Claude 定价，区分输入和输出，越大的模型每 token 越贵。折扣保留了这一排序——Opus 仍是高端档，Sonnet 是均衡的默认选择，Haiku 最便宜——只是把每个数字减半：" },
        { type: "table", headers: ["模型", "官方 输入 / 输出（$ / 1M）", "本站（−50%）"], rows: [
          ["Claude Opus 4.8", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Opus 4.7", "$5 / $25", "$2.50 / $12.50"],
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Haiku 4.5", "$1 / $5", "$0.50 / $2.50"],
        ] },
        { type: "p", text: "缓存读取和缓存写入在这些费率之上沿用 Anthropic 自己的倍率，50% 折扣在套用倍率之后再减——所以重度依赖缓存的工作流能省两次：一次来自缓存折扣本身，一次来自统一费率。" },
        { type: "link", text: "各模型的完整价格（含缓存费率）", href: "/models" },
        { type: "link", text: "用免费计算器估算你的月度成本", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "哪些负载最能感受到折扣", blocks: [
        { type: "p", text: "百分比折扣是统一的，但绝对节省额随 token 消耗量放大。真实的 Claude 账单主要由三种负载形态构成：" },
        { type: "list", items: [
          "智能体编码循环：一个任务会扇出成几十次工具调用往返，每一次都会重发不断增长的上下文。",
          "漫长的多轮会话：对话历史在每一轮都被重新按输入计费。",
          "重度依赖缓存的管道：大而稳定的系统提示词或仓库上下文被反复从缓存读取——单次调用本来就很便宜，在这里再减半。",
        ] },
        { type: "p", text: "模型选择会叠加这个效果。把高用量任务从 Opus 路由到 Haiku，在折扣之前就先把每 token 价格降低约十倍；50% 折扣再作用于你选定的任何档位。最便宜的 Claude API 调用，就是在折扣余额上调用 Haiku——每百万输入 token $0.50。" },
        { type: "note", text: "小贴士：把快速、廉价的任务路由给 Haiku，把 Opus 留给高难度推理，能让余额撑得更久。" },
      ] },
      { h2: "预付余额，而非按月订阅", blocks: [
        { type: "p", text: "没有月费，也没有需要挑选的套餐。你充值的预付余额永不过期，只在请求真正运行时才消耗——闲置的日子、闲置的星期、被搁置的业余项目都不花一分钱。这对最常见的真实使用模式很关键：重度智能体工作的爆发期之间隔着数天的沉寂，而这恰恰是按月订阅最吃亏的模式。" },
        { type: "p", text: "因为余额永不过期，在忙碌的一周里充值并不是一种承诺。剩下的钱就躺在那里，直到下一个项目、下一个原型，或下一次深夜重构。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额——适用于支持的 Claude、GPT、Gemini 与 Kimi 模型；邮箱密码账户不参与。" },
      ] },
      { h2: "两个环境变量切换现有客户端", blocks: [
        { type: "p", text: "任何基于 Anthropic SDK 构建的工具——Claude Code、Cursor、Continue、Aider、LangChain、LiteLLM——在没有显式覆盖时都会从环境变量读取端点和凭据。把两者都指向 apiToken.sale，工具原样运行，只是改从你的折扣余额扣费：" },
        { type: "code", code: `export ANTHROPIC_BASE_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••\n\n# your client now sends the same Messages API requests,\n# metered at official rates minus 50%` },
        { type: "p", text: "模型 ID、请求体、流式、工具调用和提示词缓存的请求头，表现都和直接调用 Anthropic 完全一致。只要一个请求能在官方 API 上跑通，同样的请求在这里也能跑通——差别只体现在你的余额上，而不是代码里。" },
      ] },
      { h2: "这里的“最便宜”不意味着什么", blocks: [
        { type: "p", text: "便宜的 Claude 访问通常有三种形态，值得你分清自己面对的是哪一种。重新包装的小模型便宜是因为它们更弱——任务变难之前都没问题。共享或转售的订阅在撞上速率限制或被封禁之前都很便宜。第三种形态才是本页描述的：能力完整的正版 Anthropic Messages API，纯粹通过汇集预付余额并传导折扣，在计费侧把价格降下来。" },
        { type: "p", text: "实际的检验很简单：如果模型 ID、响应格式和功能集（流式、工具调用、提示词缓存）与 Anthropic 文档完全吻合，你用的就是正版 API。这里的一切都对得上。" },
      ] },
    ],
    faq: [
      { q: "最便宜的 Claude API 和直接从 Anthropic 购买是一回事吗？", a: "是的——同一套 Anthropic Messages API、同样的模型 ID、同样的请求与响应格式。每次调用按官方 token 费率计量，先减去统一的 50%，再动你的余额。" },
      { q: "apiToken.sale 比直接调用 Anthropic 便宜多少？", a: "B2C 定价为每个请求统一比官方 API 消费低 50%，覆盖所有 Claude 模型和所有 token 类型。B2B 批量定价单独商议。" },
      { q: "每 token 最便宜的 Claude 模型是哪个？", a: "Claude Haiku 4.5，官方价为每百万输入 / 输出 token $1 / $5——统一 50% 折扣后是 $0.50 / $2.50。" },
      { q: "有月费吗？预付余额会过期吗？", a: "没有月费，预付余额永不过期。它只被真实 API 用量消耗，所以闲置期不花一分钱。" },
      { q: "折扣版 Claude API 能配合 Claude Code、Cursor 或 LangChain 使用吗？", a: "可以。把 ANTHROPIC_BASE_URL 设为 https://router.apitoken.sale，把 ANTHROPIC_API_KEY 设为你的密钥——任何基于 Anthropic SDK 的工具都会原样运行，按折扣价计费。" },
    ],
  };
