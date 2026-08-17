import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "在 LangChain 中使用 Claude API",
    h1: "在 LangChain 中使用 Claude API",
    description: "通过 apitoken.sale 将 LangChain 接入 Claude：把 ChatAnthropic 指向 router.apitoken.sale，模型 ID 保持不变，每 token 费用降低 50%。",
    keywords: ["claude api langchain", "langchain anthropic", "langchain claude", "chatanthropic base url", "langchain claude api 密钥"],
    dek: "LangChain 的 Anthropic 集成支持自定义 API URL，因此只改两行，你的链和智能体就能通过 apitoken.sale 运行 Claude——同样的模型，更低的 token 单价。",
    sections: [
      { h2: "把 ChatAnthropic 指向网关", blocks: [
        { type: "code", code: `from langchain_anthropic import ChatAnthropic\n\nllm = ChatAnthropic(\n    model="claude-opus-4-8",\n    anthropic_api_url="https://router.apitoken.sale",\n    anthropic_api_key="sk-pool-•••",\n)\nprint(llm.invoke("Hello").content)` },
        { type: "p", text: "整个集成就是这些：同一个 langchain-anthropic 包、同样的模型 ID、同样的流式输出与工具调用——变的只有端点和价格。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额，足够你接通工具并在充值前跑通真实调用。" },
      ] },
      { h2: "或通过环境变量配置", blocks: [
        { type: "code", code: `export ANTHROPIC_API_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••` },
        { type: "p", text: "设置好环境变量后，ChatAnthropic 会自动读取这两个值，共享代码库完全无需改代码。" },
      ] },
      { h2: "哪些功能可用", blocks: [
        { type: "list", items: [
          "链、智能体和 LangGraph 工作流——协议不变。",
          "通过标准集成使用流式输出、工具调用和结构化输出。",
          "所有受支持的 Claude 模型（Opus、Sonnet、Haiku）共用一把密钥和一个余额。",
        ] },
      ] },
    ],
    faq: [
      { q: "LangChain 支持自定义 Claude API 端点吗？", a: "支持。ChatAnthropic 接受 anthropic_api_url（或 ANTHROPIC_API_URL 环境变量），把它指向 https://router.apitoken.sale 即可，其余保持不变。" },
      { q: "LangChain 智能体和工具调用还能用吗？", a: "能——网关提供标准的 Anthropic Messages API，工具调用、流式输出和 LangGraph 智能体的行为与官方端点完全一致。" },
      { q: "从 LangChain 能用哪些模型？", a: "所有受支持的 Claude 模型——claude-opus-4-8、claude-sonnet-5、claude-haiku-4-5 等——共用一把密钥和预付余额。" },
    ],
  };
