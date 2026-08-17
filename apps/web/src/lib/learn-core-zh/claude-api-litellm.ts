import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "在 LiteLLM 中使用 Claude API",
    h1: "在 LiteLLM 中使用 Claude API",
    description: "通过 apitoken.sale 在 LiteLLM 中使用 Claude API：保留 anthropic/ 前缀，在 litellm.completion() 或代理配置中把 api_base 设为 router.apitoken.sale，每 token 费用降低 50%。",
    keywords: ["claude api litellm", "litellm anthropic", "litellm claude", "litellm api_base anthropic", "litellm proxy claude", "litellm claude api key", "litellm anthropic base url", "litellm 自定义 anthropic 端点", "litellm 代理接入 claude", "litellm claude 便宜"],
    dek: "通过 apitoken.sale 在 LiteLLM 中使用 Claude API，关键只有一个参数：LiteLLM 原生支持 Anthropic Messages 协议，所以保留 anthropic/ 模型前缀，只需覆盖 api_base。请求和响应结构完全不变，每 token 便宜 50%——无论从脚本里调用 litellm.completion()，还是用 LiteLLM 代理统一承接整个技术栈。",
    sections: [
      { h2: "把 litellm.completion() 指向折扣端点", blocks: [
        { type: "p", text: "LiteLLM 本身已实现 Anthropic Messages API，所以把 Claude 路由到 apitoken.sale 只需多传一个参数：保留 anthropic/ 模型前缀，把 api_base 设为网关地址，并传入你的预付费密钥。请求与响应保持标准的 Anthropic 格式——变化的只有端点和每 token 价格，Claude 消费统一比官方标价低 50%。" },
        { type: "code", code: `import litellm\n\nresponse = litellm.completion(\n    model="anthropic/claude-opus-4-8",\n    api_base="https://router.apitoken.sale",\n    api_key="sk-pool-•••",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Hello"}],\n    stream=True,\n)\nfor chunk in response:\n    print(chunk.choices[0].delta.content or "", end="")` },
        { type: "p", text: "这里起作用的有三点。anthropic/ 前缀选中 LiteLLM 的 Anthropic provider，因此 max_tokens、temperature、tools 和流式输出会精确映射到 Messages API，与直连上游一致——而 Messages API 要求必传 max_tokens，所以请显式设置，不要依赖默认值。api_base 决定这些请求发到哪里，按调用粒度生效。api_key 则是你的网关密钥：同一把 sk-pool-… 密钥适用于所有受支持的 Claude 模型，在 claude-opus-4-8、claude-sonnet-5 和 claude-haiku-4-5 之间切换只是改一个字符串，而不是重新接入。" },
        { type: "note", text: "实践中有两个坑。永远不要去掉 anthropic/ 前缀：裸写 claude-opus-4-8 会让 LiteLLM 猜测 provider，猜错就会发错协议或直接拒绝密钥。另外从环境变量读取密钥（api_key=os.environ[\"APITOKEN_KEY\"]），不要把它粘贴进最终会提交到 git 的 notebook 或配置文件。" },
      ] },
      { h2: "一个 LiteLLM 代理，承接所有需要 Claude 的服务", blocks: [
        { type: "p", text: "单个脚本用直接调用就够了。一旦有多个服务、notebook 和编码智能体都要用 Claude，就把 LiteLLM 跑成代理：一份 YAML 文件集中管理端点和密钥，所有客户端通过 LiteLLM 的 OpenAI 兼容接口与代理通信，上游流量仍走 Anthropic 协议。" },
        { type: "code", code: `# config.yaml\nmodel_list:\n  - model_name: claude-opus-4-8\n    litellm_params:\n      model: anthropic/claude-opus-4-8\n      api_base: https://router.apitoken.sale\n      api_key: sk-pool-•••\n  - model_name: claude-haiku-4-5\n    litellm_params:\n      model: anthropic/claude-haiku-4-5\n      api_base: https://router.apitoken.sale\n      api_key: sk-pool-•••\nrouter_settings:\n  fallbacks:\n    - claude-opus-4-8:\n        - claude-haiku-4-5` },
        { type: "steps", items: [
          "安装 proxy 扩展，并把上面的 YAML 保存为 config.yaml：pip install \"litellm[proxy]\"。",
          "启动网关：litellm --config config.yaml --port 4000。",
          "把任意 OpenAI 兼容客户端指向 http://localhost:4000，model=\"claude-opus-4-8\"——代理会把调用转换成发往 https://router.apitoken.sale 的 Anthropic Messages 请求。",
          "在 apitoken.sale 控制台追踪消费：用量按密钥记录、精确到 token，一把代理密钥就能为它背后的每个服务给出一条独立的成本明细。",
        ] },
        { type: "p", text: "router_settings 这两行很值：当 claude-opus-4-8 报错或不可用时，LiteLLM 会改用 claude-haiku-4-5 重试请求，而不是把失败抛给客户端。对于会话一开就是数小时的长时运行智能体，这个 fallback 就是静默重试与进程僵死之间的差别。" },
      ] },
      { h2: "流式输出、工具调用和提示词缓存全部保留", blocks: [
        { type: "p", text: "那些通常在协议转换层之后就会坏掉的功能，在这里都照常工作，因为网关提供的是原生 Anthropic Messages API，而不是把你的流量重新编码成另一种协议。凡是 LiteLLM 能用 Anthropic 术语表达的东西，都会原封不动地到达模型。" },
        { type: "list", items: [
          "流式输出：stream=True 产生同样的增量 SSE 事件，逐 token 渲染的 UI 和智能体行为完全一致。",
          "工具调用：tools、tool_choice 和 tool_result 的完整往返映射到标准 Messages 块——函数调用智能体无需任何改造。",
          "提示词缓存：cache_control 断点按上游文档所述工作，缓存读取按模型页面列出的缓存费率计费。",
        ] },
        { type: "p", text: "这一点对构建在 LiteLLM 之上的工具比 LiteLLM 本身更重要：许多编码智能体和框架的 Anthropic 流量都经由 LiteLLM 路由，它们无需修改自身代码，就能从同一份配置中继承折扣端点。" },
      ] },
      { h2: "把 GPT、Gemini 和 Kimi 混进同一个 model_list", blocks: [
        { type: "p", text: "网关密钥是多供应商的，所以你刚配好的代理并不只服务 Claude。每个供应商通道加一条配置，所有模型都从同一个预付余额扣费——不用开第二个账户，也不用轮换第二把密钥。" },
        { type: "code", code: `# additional model_list entries\n  - model_name: gpt-5.6-terra\n    litellm_params:\n      model: openai/gpt-5.6-terra        # OpenAI-compatible lane\n      api_base: https://router.apitoken.sale/v1\n      api_key: sk-pool-•••\n  - model_name: gemini-3.6-flash\n    litellm_params:\n      model: gemini/gemini-3.6-flash     # native Gemini lane\n      api_base: https://router.apitoken.sale\n      api_key: sk-pool-•••` },
        { type: "p", text: "Kimi 模型走同样的两条通道——Anthropic Messages 或通用 OpenAI 兼容端点——所以一套 LiteLLM 部署可以同时承接受支持的 Claude、GPT、Gemini 和 Kimi 模型。每个供应商继续使用 LiteLLM 本来就会说的协议，只有 base URL 和密钥指向了新地址。" },
      ] },
      { h2: "切换后什么变了，什么保持不变", blocks: [
        { type: "p", text: "切换端点被刻意设计得平淡无奇，但值得精确说明：技术栈里哪些部分会感知到变化，哪些不会。" },
        { type: "table", headers: ["层级", "你要设置什么", "会发生什么"], rows: [
          ["模型 ID", "anthropic/claude-opus-4-8、anthropic/claude-sonnet-5、anthropic/claude-haiku-4-5", "ID 与上游一致；前缀用于选择 Anthropic 协议"],
          ["端点", "https://router.apitoken.sale", "原生 Anthropic Messages API，而非 OpenAI 格式转换"],
          ["功能", "流式输出、工具调用、提示词缓存", "行为与直连官方端点时一致"],
          ["价格", "每 token 比标价低 50%", "适用于同一预付余额下所有受支持的 Claude 模型"],
          ["记账", "一把 sk-pool-… 密钥", "控制台按密钥统计消费，精确到 token"],
        ] },
      ] },
      { h2: "先给流量做预算，再放量", blocks: [
        { type: "p", text: "计费是预付费模式：先充值余额，每个请求按实际 token 成本精确扣费，Claude 模型按折扣价计。没有需要提前预估的月度承诺，这让 LiteLLM 的按模型成本追踪只是锦上添花，而非生存工具——权威数字在 apitoken.sale 控制台里，按密钥拆分，精确到 token。" },
        { type: "p", text: "把整个机群指向代理之前，先用一把密钥跑一天有代表性的流量，从控制台读取真实消耗；按真实 token 外推，而不是按标价算术。如果你已知大致的请求构成，下面链接的成本计算器可以提前帮你算好这笔账。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台奖励余额——适用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱/密码账户不享受此奖励。" },
        { type: "link", text: "各模型价格（含缓存费率）", href: "/models" },
        { type: "link", text: "用免费计算器估算你的 LiteLLM 流量成本", href: "/tools/claude-api-cost-calculator" },
      ] },
    ],
    faq: [
      { q: "如何在 LiteLLM 中设置自定义 Anthropic base URL？", a: "直接向 litellm.completion() 传 api_base，或在代理配置 model_list 的 litellm_params 下设置它。LiteLLM 随后会把 Anthropic Messages 格式的请求发往该端点——对 apitoken.sale 来说就是 https://router.apitoken.sale。" },
      { q: "通过网关路由 Claude 时要保留 anthropic/ 模型前缀吗？", a: "要。使用 anthropic/claude-opus-4-8（或任何受支持的模型），让 LiteLLM 应用 Anthropic 协议；只有端点和密钥变化，去掉前缀会让 LiteLLM 猜测 provider。" },
      { q: "自定义 api_base 时 LiteLLM 的流式输出还能用吗？", a: "能。stream=True 会通过网关返回同样的增量 Anthropic 事件，逐 token 渲染和智能体循环的行为与直连官方端点完全一致。" },
      { q: "单个 LiteLLM 代理能同时服务 Claude、GPT 和 Gemini 吗？", a: "能。一把 apitoken.sale 密钥覆盖 Claude、GPT、Gemini 和 Kimi 的受支持模型；每个供应商作为独立的 model_list 条目添加——anthropic/ 和 gemini/ 模型指向 https://router.apitoken.sale，openai/ 模型指向 https://router.apitoken.sale/v1。" },
      { q: "如何在 LiteLLM 中实现 Claude 模型间的故障转移？", a: "在代理配置中使用 router_settings.fallbacks，把主部署映射到备用部署——例如 claude-opus-4-8 到 claude-haiku-4-5。两个条目指向同一个网关和同一把密钥，所以重试仍在折扣余额上扣费。" },
    ],
  };
