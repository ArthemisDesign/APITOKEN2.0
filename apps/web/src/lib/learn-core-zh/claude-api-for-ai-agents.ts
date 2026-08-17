import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "面向 AI 智能体的 Claude API",
    h1: "用 Claude API 构建 AI 智能体",
    description: "用 apitoken.sale 在 Claude API 上构建 AI 智能体：一把密钥通用所有模型，配合流式输出、工具调用、提示词缓存和密钥终身消费上限，控制长时间运行的成本。",
    keywords: ["claude api 智能体", "claude ai agent api", "claude 工具调用", "用 claude 构建 ai 智能体", "claude 智能体循环成本", "claude api 模型路由", "claude 智能体提示词缓存", "claude api 流式输出", "claude api 消费上限", "多智能体 claude api"],
    dek: "Claude API 是构建智能体的坚实基础：工具调用和流式输出都是一等公民，模型梯队也能干净地映射到智能体循环的各个步骤。难点在于经济性——一次任务往往要跑几十次调用，所以模型路由、缓存和硬性消费上限决定了一次运行是否划算。本指南讲清这三件事如何在 apitoken.sale 的 Claude API 上落地。",
    sections: [
      { h2: "为什么智能体循环比聊天更烧 token", blocks: [
        { type: "p", text: "是的，Claude API 很适合做 AI 智能体——工具调用、流式输出和提示词缓存都是 Anthropic Messages API 的标准能力，而且全部可以通过一把 apitoken.sale 密钥访问。和聊天机器人的差别在于量级。聊天用户发一条消息、读一条回复；智能体则要规划、调用工具、读取结果、重新规划、再重复——一个用户可见的任务轻轻松松就是几十次模型调用，而且每次都带着迄今为止的完整对话。" },
        { type: "p", text: "这种模式改变了真正重要的东西。单次调用的延迟不如每个完成任务的成本重要。主导 token 数量的是重复出现的上下文——系统提示词、工具定义、不断累积的工具结果——而不是最终答案。做好三件事，智能体的经济性就能成立：把每一步路由到能胜任的最便宜模型，缓存一切重复内容，给失控循环能花的钱设一个硬上限。" },
      ] },
      { h2: "把循环中的每一步路由到合适的模型", blocks: [
        { type: "p", text: "把整个循环当成一次模型调用，是智能体设计中代价最高的错误。规划步骤需要强推理；从工具结果里抽出一个 URL 的步骤则完全不需要。Anthropic 的模型梯队正好对应这个分层：Haiku 跑廉价的机械步骤，Sonnet 做推理核心，Opus 只留给前两者都搞不定的少数调用。在 apitoken.sale 上，三档模型共用同一把密钥和同一个余额，切换档位只是改请求里的一个字符串——不需要额外的账号，也不需要额外的计费关系。" },
        { type: "table", headers: ["循环步骤", "模型", "原因"], rows: [
          ["规划、任务分解、自我批评", "claude-sonnet-5", "推理质量与成本的最佳平衡；默认主力"],
          ["解析、分类、抽取、路由", "claude-haiku-4-5", "最便宜的档位；这些步骤量大且难度低"],
          ["Sonnet 尝试失败后的最难调用", "claude-opus-4-8", "仅用于升级——留给真正需要它的步骤"],
        ] },
        { type: "p", text: "一个实用的升级模式：先用 Sonnet 跑这一步，只有当输出校验失败时（JSON 格式错误、计划被拒绝、测试没通过），才对这一步单独用 Opus 重试。大多数循环永远不会触发升级，Opus 的费用只花在真正能换来东西的地方。" },
      ] },
      { h2: "最小的智能体调用：基于 SSE 的工具调用", blocks: [
        { type: "p", text: "智能体的一步就是一个普通的 Messages API 请求，只多了两样东西：一个描述模型可调用工具的 tools 数组，以及设为 true 的 stream，这样你可以基于部分输出提前行动。模型返回一个 tool_use 块和 stop_reason \"tool_use\"；你的代码执行工具、追加一条 tool_result 消息，然后再次调用 API。这个往返就是智能体循环的全部——其余的一切都只是编排。" },
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "stream": true,\n    "system": "You are a research agent. Use tools, then answer.",\n    "tools": [{\n      "name": "web_search",\n      "description": "Search the web",\n      "input_schema": {"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}\n    }],\n    "messages": [{"role":"user","content":"Find the latest Anthropic model"}]\n  }'` },
        { type: "p", text: "端点就是原生 Anthropic Messages API，所以官方 Anthropic SDK——以及所有支持这个协议的智能体框架——只需换一个 base URL 和密钥即可原样工作。流式输出以标准的服务器发送事件（SSE）到达，且流式请求与非流式请求的计费方式完全相同：都按输入和输出 token 计费。" },
      ] },
      { h2: "缓存每个请求中不变的部分", blocks: [
        { type: "p", text: "在一个二十步的循环里，系统提示词和工具定义会被发送二十次。提示词缓存把这部分重复上下文从反复发生的成本变成几乎免费：在稳定的前缀上打一个 cache_control 断点，后续调用的缓存读取只需花全新输入 token 的一小部分。缓存条目存活在一个固定的短窗口内（默认五分钟），每次命中都会刷新——这恰好就是一个活跃智能体的访问模式。" },
        { type: "p", text: "顺序很重要。把最稳定的内容放在最前面——先是系统提示词，然后是工具定义，再是最早的对话历史——并且永远不要把易变内容（时间戳、请求 ID）插到断点之前，否则每次调用都会变成缓存写入，而不是缓存读取。" },
        { type: "note", text: "缓存与 apitoken.sale 对官方 token 定价统一执行的 50% 折扣相互叠加：缓存减少 token 数量，折扣降低每个 token 的单价。" },
        { type: "link", text: "在循环无人值守地运行之前，先估算它的真实成本", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "限制失控循环的爆炸半径", blocks: [
        { type: "p", text: "每个智能体迟早会撞上一个无法收敛的循环——一个不断返回错误、模型又不断重试的工具。客户端防护（最大迭代计数器、单任务 token 预算、墙钟超时）是第一道防线，但它们活在你的进程里，也会随你进程里的 bug 一起失效。第二道防线应该落在密钥本身上。" },
        { type: "steps", items: [
          "在 apitoken.sale 控制台为每个智能体创建一把独立的命名密钥——绝不在智能体之间、也不与人共用一把密钥。",
          "让智能体指向 https://router.apitoken.sale，把这把密钥放进 x-api-key 请求头，和普通的 Messages API 客户端完全一样。",
          "为密钥设置终身消费上限：一旦该密钥的累计消费达到上限，后续请求就会被拒绝，失控的循环花不出超过上限的钱。",
          "如果智能体是临时的——演示、CI 任务、外包的原型——再设一个到期时间，让访问自动结束。",
          "在控制台按密钥查看 token 级用量；某一步突然主导账单，通常意味着路由或缓存的 bug，而不是价格问题。",
        ] },
      ] },
      { h2: "在一个智能体中混合多家提供商", blocks: [
        { type: "p", text: "智能体的每一步不必都用 Claude。同一把 apitoken.sale 密钥也能调用支持的 GPT、Gemini 和 Kimi 模型，所以一个循环可以用 Claude 起草、在另一个模型家族的轻量模型上跑廉价的分类步骤，或者跨提供商比对答案来做校验。Anthropic 原生调用保持上面展示的 Messages 形态；GPT 模型走 OpenAI 兼容通道，使用 Authorization: Bearer 请求头。" },
        { type: "code", code: `curl https://router.apitoken.sale/v1/chat/completions \\\n  -H "Authorization: Bearer sk-pool-•••" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "gpt-5.5",\n    "messages": [{"role":"user","content":"Classify: is this tool result an error?"}]\n  }'` },
        { type: "p", text: "所有消费都落在同一个预付余额上，享受同样的按 token 折扣——这正是异构循环可行的原因：没有分提供商的账号，也没有需要对账的独立预算。" },
      ] },
      { h2: "调优良好的智能体在账单上长什么样", blocks: [
        { type: "list", items: [
          "绝大多数调用落在 Haiku 或 Sonnet 上；Opus 只出现在真正的升级场景。",
          "从第二次调用起，缓存读取主导每次调用的输入 token。",
          "流式输出处于开启状态，工具调用畸形时编排器可以提前中止。",
          "每把密钥都有终身消费上限，和一个能告诉你它属于哪个智能体的名字。",
          "预付余额永不过期，所以空闲的月份不花一分钱。",
        ] },
        { type: "link", text: "深入流式机制：SSE 事件、提前中止、计费一致性", href: "/docs/learn/claude-api-streaming" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "Claude API 适合构建 AI 智能体吗？", a: "适合。工具调用和流式输出是 Anthropic Messages API 的一等能力，Haiku/Sonnet/Opus 三档能干净地映射到智能体循环的各个步骤——全部通过一把 apitoken.sale 密钥即可访问。" },
      { q: "智能体默认应该用哪个 Claude 模型？", a: "规划和推理用 claude-sonnet-5，解析、分类这类量大且机械的步骤用 claude-haiku-4-5，claude-opus-4-8 只作为升级手段，留给 Sonnet 校验失败的调用。" },
      { q: "如何防止智能体循环超支？", a: "把客户端防护（迭代次数和 token 上限）与 apitoken.sale 密钥的终身消费上限结合起来——后者会在达到上限时硬性停止消费；临时智能体再加一个到期时间。" },
      { q: "工具调用在 apitoken.sale 上能用吗？", a: "能——router.apitoken.sale 上就是原生 Anthropic Messages API，标准的 tool_use/tool_result 往返和官方 SDK 只需换一个 base URL 和密钥即可工作。" },
      { q: "智能体应该使用流式输出吗？", a: "通常应该：流式让编排器能基于部分输出行动并提前中止，而且流式请求与非流式请求按输入输出 token 的计费方式完全相同。" },
      { q: "一个智能体能混用 Claude 和 GPT、Gemini 或 Kimi 模型吗？", a: "能——同一把密钥、同一个预付余额覆盖全部四个模型家族。Claude 走 Anthropic Messages 端点；GPT 走 OpenAI 兼容通道，使用 Authorization: Bearer 请求头。" },
    ],
  };
