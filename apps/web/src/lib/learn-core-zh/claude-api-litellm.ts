import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "在 LiteLLM 中使用 Claude API",
    h1: "在 LiteLLM 中使用 Claude API",
    description: "通过 apitoken.sale 将 LiteLLM 路由到 Claude：在 litellm_params 或代理配置中把 api_base 设为 router.apitoken.sale，每 token 费用降低 50%。",
    keywords: ["claude api litellm", "litellm anthropic", "litellm claude", "litellm api_base anthropic", "litellm 代理 claude"],
    dek: "LiteLLM 原生支持 Anthropic，并允许为每个模型覆盖端点——一行配置即可把你全部的 Claude 流量送经折扣网关。",
    sections: [
      { h2: "直接 SDK 调用", blocks: [
        { type: "code", code: `import litellm\n\nresponse = litellm.completion(\n    model="anthropic/claude-opus-4-8",\n    api_base="https://router.apitoken.sale",\n    api_key="sk-pool-•••",\n    messages=[{"role": "user", "content": "Hello"}],\n)` },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额，足够你接通工具并在充值前跑通真实调用。" },
      ] },
      { h2: "LiteLLM 代理配置", blocks: [
        { type: "code", code: `# config.yaml\nmodel_list:\n  - model_name: claude-opus-4-8\n    litellm_params:\n      model: anthropic/claude-opus-4-8\n      api_base: https://router.apitoken.sale\n      api_key: sk-pool-•••` },
        { type: "p", text: "用这份配置运行代理，你的 LiteLLM 网关的每个客户端都会透明地使用折扣版 Claude 端点——当多个服务共享一个路由层时尤其方便。" },
      ] },
      { h2: "为什么在这里通过 LiteLLM 路由 Claude", blocks: [
        { type: "list", items: [
          "在一个地方把所有服务切到更便宜的端点。",
          "沿用你已有的 anthropic/ 模型前缀和参数。",
          "apitoken.sale 控制台按密钥追踪消费，精确到 token。",
        ] },
      ] },
    ],
    faq: [
      { q: "LiteLLM 支持自定义 Anthropic api_base 吗？", a: "支持——在 litellm.completion() 或代理配置的 litellm_params 中传入 api_base，LiteLLM 就会把 Anthropic 格式的请求发送到 https://router.apitoken.sale。" },
      { q: "模型还用 anthropic/ 前缀吗？", a: "是的。使用 anthropic/claude-opus-4-8（或任何受支持的模型），让 LiteLLM 应用 Anthropic 协议；变的只有端点和密钥。" },
      { q: "基于 LiteLLM 的工具也适用吗？", a: "适用——凡是经 LiteLLM 路由的东西（包括许多编码智能体）都会从同一份配置继承折扣端点。" },
    ],
  };
