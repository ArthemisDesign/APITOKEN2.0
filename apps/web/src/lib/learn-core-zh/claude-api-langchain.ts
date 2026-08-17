import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "在 LangChain 中使用 Claude API",
    h1: "在 LangChain 中使用 Claude API",
    description: "通过 apitoken.sale 将 LangChain 接入 Claude：把 ChatAnthropic 指向 router.apitoken.sale，模型 ID 保持不变，每 token 费用降低 50%。",
    keywords: ["claude api langchain", "langchain anthropic", "langchain 接入 claude", "chatanthropic base url", "langchain claude api 密钥", "chatanthropic anthropic_api_url", "langchain 自定义 anthropic 端点", "langgraph claude api", "chatanthropic 流式输出", "langchain claude 更便宜"],
    dek: "Claude API 开箱即可配合 LangChain 使用，而 ChatAnthropic 接受自定义 API URL——所以只要改两行，你的链和智能体就能通过 apitoken.sale 运行 Claude。同一个 langchain-anthropic 包、同样的模型 ID、同样的流式输出与工具调用；变化的只有端点和 token 单价。",
    sections: [
      { h2: "把 ChatAnthropic 指向 router.apitoken.sale", blocks: [
        { type: "p", text: "LangChain 的 Anthropic 集成接受自定义 API URL，因此通过 apitoken.sale 把 Claude API 接入 LangChain，只需要两个构造函数参数：anthropic_api_url 和 anthropic_api_key。现有链中的提示词、输出解析器、回调和重试逻辑完全不用动。" },
        { type: "code", code: `from langchain_anthropic import ChatAnthropic\n\nllm = ChatAnthropic(\n    model="claude-opus-4-8",\n    anthropic_api_url="https://router.apitoken.sale",\n    anthropic_api_key="sk-pool-•••",\n)\nprint(llm.invoke("Hello").content)` },
        { type: "note", text: "严格按示例填写路由根地址：不要末尾斜杠，不要 /v1 后缀。底层 Anthropic 客户端会自己拼接 /v1/messages，路径重复是配置正确却返回 404 的最常见原因。" },
        { type: "p", text: "有一个参数值得显式设置：max_tokens。ChatAnthropic 默认只输出 1024 个 token，长回答会被静默截断——做摘要或代码生成的链要把它调大。temperature、top_p 等采样参数会原样透传，系统提示词和停止序列也一样。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台奖励余额——适用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱/密码账户不享受此奖励。" },
      ] },
      { h2: "用环境变量一次配好", blocks: [
        { type: "p", text: "如果你的代码库要和用官方端点的人共享——或者在不便改源码的 notebook 里跑——完全可以跳过构造函数参数。ChatAnthropic 会从环境变量读取这两个值，已入库的项目零代码改动。" },
        { type: "code", code: `export ANTHROPIC_API_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••` },
        { type: "steps", items: [
          "安装合作方包：pip install -U langchain-anthropic。LangChain 把 Anthropic 支持放在这个包里，而不是 langchain-core。",
          "在 apitoken.sale 控制台生成密钥——以 sk-pool- 开头，可在受支持的 Claude、GPT、Gemini 和 Kimi 模型间通用。",
          "按上面的方式导出 ANTHROPIC_API_URL 和 ANTHROPIC_API_KEY（或写进你的运行器加载的 .env 文件）。",
          "不带其他参数实例化 ChatAnthropic(model=\"claude-sonnet-5\")，跑一次 invoke() 确认返回正常响应。",
        ] },
        { type: "p", text: "显式的构造函数参数优先于环境变量，所以本地覆盖绝不会泄漏进共享配置。环境变量方式还能让密钥不进 git 历史——把 sk-pool-… 当作普通机密对待：.env 文件不提交，CI 从自己的密钥存储里取值。" },
      ] },
      { h2: "流式输出、工具调用和 LangGraph 全部照常", blocks: [
        { type: "p", text: "网关提供标准的 Anthropic Messages API，LangChain 通过官方客户端与之通信。建立在该协议之上的一切——SSE 流式输出、tool-use 块、结构化输出——行为与直连 api.anthropic.com 完全一致。这包括 LangChain 基于工具调用实现的 with_structured_output()，以及异步应用中用于 token 级回调的 .astream_events()。" },
        { type: "code", code: `from langchain_anthropic import ChatAnthropic\nfrom langchain_core.tools import tool\n\n@tool\ndef get_weather(city: str) -> str:\n    """Return the current weather for a city."""\n    return f"Sunny in {city}"\n\nllm = ChatAnthropic(model="claude-sonnet-5")  # env vars supply URL and key\nllm_with_tools = llm.bind_tools([get_weather])\n\nfor chunk in llm_with_tools.stream("What is the weather in Paris?"):\n    print(chunk.content, end="")` },
        { type: "p", text: "LangGraph 智能体继承同一套配置，因为图节点无非是调用一个聊天模型。把模型指向路由一次，建立在它之上的每个智能体、supervisor 和子图都会跟随——没有任何 LangGraph 专属配置需要重做。" },
        { type: "note", text: "token 统计也照常工作：每条 AIMessage 仍然携带包含输入输出 token 数的 usage_metadata，因为网关返回标准的 Anthropic usage 对象。读取 usage_metadata 的 LangSmith 追踪和自定义回调无需任何改动。" },
      ] },
      { h2: "什么变了，什么没变", blocks: [
        { type: "p", text: "迁移生产应用之前，最好把全部差异放在一处看清楚。简版结论：你的代码、你的模型和你的 LangChain 功能原地不动——唯一变动的部分是端点、密钥和每 token 价格。" },
        { type: "table", headers: ["关注点", "通过 apitoken.sale"], rows: [
          ["模型 ID", "不变——claude-opus-4-8、claude-sonnet-5、claude-haiku-4-5 及目录中的其余模型"],
          ["协议", "不变——通过官方客户端走 Anthropic Messages API"],
          ["流式输出与工具调用", "不变——SSE 块和 tool-use 块照常"],
          ["链、智能体、LangGraph", "不变——除 URL 和密钥外零代码改动"],
          ["每 token 价格", "同样的模型便宜 50%"],
          ["API 密钥", "一把 sk-pool-… 密钥通用于受支持的 Claude、GPT、Gemini 和 Kimi 模型"],
          ["计费", "预付余额，控制台提供按密钥的消费与 token 明细"],
        ] },
        { type: "link", text: "查看受支持的 Claude 模型与价格完整列表", href: "/models" },
      ] },
      { h2: "为每个节点选对 Claude 模型", blocks: [
        { type: "p", text: "切换模型只是改一个参数，所以把模型选择当作逐节点的决策，而不是全局决策。负责意图分类的路由链，并不需要和撰写最终答案的节点用同一档模型。" },
        { type: "list", items: [
          "claude-haiku-4-5——快速、便宜的一档：分类、路由、抽取等大批量步骤。",
          "claude-sonnet-5——多数生产链、RAG 流水线和编码智能体的均衡默认选择。",
          "claude-opus-4-8——顶级推理档；留给困难分析、长文档和智能体规划步骤。",
        ] },
        { type: "code", code: `from langchain_anthropic import ChatAnthropic\n\nfast = ChatAnthropic(model="claude-haiku-4-5")      # routing, extraction\nbalanced = ChatAnthropic(model="claude-sonnet-5")   # default nodes\ndeep = ChatAnthropic(model="claude-opus-4-8")       # planning, hard analysis\n\nrouter_chain = router_prompt | fast\nanswer_chain = answer_prompt | balanced | StrOutputParser()` },
        { type: "p", text: "三个实例共用同一套由环境变量提供的 URL 和密钥，每次调用都计入同一个预付余额。这让分档实验成本很低：换掉模型字符串，重跑评估集，留下胜出的组合。" },
        { type: "note", text: "先用 Sonnet 做原型，再把简单节点降级到 Haiku，只把困难节点升级到 Opus。按 token 预付计费下，混档链的成本明显低于全部跑旗舰模型。" },
        { type: "link", text: "用成本计算器估算混合模型链的开销", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "连接问题排查", blocks: [
        { type: "p", text: "变化的只有端点和密钥，所以几乎所有故障都是三种配置错误之一——而不是 LangChain 的问题。按顺序逐项排查，先别动链代码。" },
        { type: "list", items: [
          "401 Unauthorized——密钥缺失或拼错，或者环境变量根本没传到进程。在同一个解释器里打印 os.environ 确认，并记住构造函数参数会覆盖环境变量。",
          "404 Not Found——URL 多带了 /v1 或多余的路径后缀。使用裸路由根地址 https://router.apitoken.sale。",
          "Model not found——对照 /models 目录重新核对模型 ID；这里的 ID 与 Anthropic 官方发布的完全一致。",
        ] },
        { type: "p", text: "如果不确定问题出在网关还是你的链，把 URL 换回官方端点跑一次。行为完全相同说明 bug 在链里；行为有差异则可以把范围缩小到配置上。" },
        { type: "note", text: "遇到偶发的 429 或 5xx 响应不需要自写逻辑：ChatAnthropic 默认以退避策略重试两次（可用 max_retries 调整）。长时间运行的智能体仍应显式设置超时秒数，而不是依赖客户端默认值。" },
      ] },
    ],
    faq: [
      { q: "LangChain 支持自定义 Claude API 端点吗？", a: "支持。ChatAnthropic 接受 anthropic_api_url（或 ANTHROPIC_API_URL 环境变量），把它指向 https://router.apitoken.sale 即可，其余一切——包、模型 ID、链代码——保持不变。" },
      { q: "如何不改代码设置 LangChain 的 Anthropic base URL？", a: "在运行脚本前导出 ANTHROPIC_API_URL=https://router.apitoken.sale 和 ANTHROPIC_API_KEY=sk-pool-…。ChatAnthropic 会自动读取这两个值，共享仓库完全无需改动。" },
      { q: "通过 apitoken.sale 还能用流式输出和工具调用吗？", a: "能。网关提供标准的 Anthropic Messages API，因此 .stream()、bind_tools()、结构化输出和 LangGraph 智能体的行为与官方端点完全一致。" },
      { q: "从 LangChain 可以调用哪些 Claude 模型？", a: "所有受支持的 Claude 模型——claude-opus-4-8、claude-sonnet-5、claude-haiku-4-5 等——共用同一把密钥和预付余额，每 token 便宜 50%。" },
      { q: "可以用 ChatOpenAI 代替 ChatAnthropic 来调 Claude 吗？", a: "可以。路由还提供 OpenAI 兼容通道，地址是 https://router.apitoken.sale/v1，因此 ChatOpenAI(base_url=\"https://router.apitoken.sale/v1\", api_key=\"sk-pool-•••\") 能用同一把密钥访问同样的 Claude 模型——当某个框架只会说 OpenAI 协议时很方便。" },
      { q: "在 LangChain 里用 GPT、Gemini 或 Kimi 需要单独的密钥吗？", a: "不需要。同一把 sk-pool-… 密钥在受支持的 Claude、GPT、Gemini 和 Kimi 模型间通用，多供应商的 LangChain 应用可以共用一把密钥和一个预付余额。" },
    ],
  };
