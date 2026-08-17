import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API 接入 Aider：配置指南与 50% 折扣",
    h1: "在 Aider 中使用 Claude API",
    description: "通过 apiToken.sale 让 Aider 跑在 Claude 上：导出 ANTHROPIC_API_BASE 和 API 密钥，选好 Claude 模型，即可在终端结对编程，全程统一 50% 折扣。",
    keywords: ["claude api aider", "aider anthropic", "aider claude", "aider anthropic api base", "aider claude api 密钥", "aider 自定义 anthropic 端点", "aider 便宜 claude", "aider weak model", "aider token 用量", "aider claude sonnet"],
    dek: "Aider 通过 LiteLLM 访问 Claude API，而 LiteLLM 认 ANTHROPIC_API_BASE——所以只需两个环境变量，就能把整套 claude-api-aider 配置切到折扣网关。模型不变、命令不变、git 工作流不变；每个 token 都按官方价格的统一 50% 折扣从预付余额扣费。",
    sections: [
      { h2: "把 Aider 指向网关端点", blocks: [
        { type: "p", text: "是的，Aider 支持自定义 Claude 端点，改动不到一分钟。Aider 底层通过 LiteLLM 路由 Anthropic 流量，而 LiteLLM 认 ANTHROPIC_API_BASE 环境变量——不需要配置文件、插件或补丁。导出端点和密钥，然后照常启动 Aider 即可。" },
        { type: "code", code: `export ANTHROPIC_API_KEY=sk-pool-•••\nexport ANTHROPIC_API_BASE=https://router.apitoken.sale\n\naider --model anthropic/claude-opus-4-8` },
        { type: "p", text: "密钥在 apiToken.sale 控制台生成，形如 sk-pool-…；一把密钥通用于所有受支持的模型，所以你之后传给 --model 的任何其他 Claude 模型也由这两个变量覆盖。" },
        { type: "note", text: "ANTHROPIC_API_BASE 不是 Claude Code 读的那个变量（那个是 ANTHROPIC_BASE_URL）。Aider 走 LiteLLM，要的是 API_BASE 这个拼法——如果你在这把密钥上同时用两个工具，就把两个变量都导出来，它们互不冲突。" },
        { type: "p", text: "Windows 上在 PowerShell 里设置同样的两个变量，不用 export：$env:ANTHROPIC_API_KEY 和 $env:ANTHROPIC_API_BASE，或者用 setx 让它们跨会话持久化。Aider 行为完全一致——LiteLLM 在任何平台都读进程环境。" },
      ] },
      { h2: "让配置在新终端里也能生效", blocks: [
        { type: "p", text: "export 出来的变量随 shell 一起消亡。把它们写进 shell 配置文件（~/.zshrc、~/.bashrc），让每个终端启动时就绪；模型选择则放进 Aider 自己的 YAML 配置——Aider 按顺序从用户主目录、git 仓库根目录、当前目录读取 .aider.conf.yml。" },
        { type: "code", code: `# ~/.aider.conf.yml\nmodel: anthropic/claude-sonnet-5\nweak-model: anthropic/claude-haiku-4-5\ncache-prompts: true` },
        { type: "p", text: "密钥不要写进这个文件：密钥留在环境变量里，配置只放行为选项。把项目级的 .aider.conf.yml 提交进仓库，就能为整个团队固定模型选择，而不会固定任何人的密钥。" },
      ] },
      { h2: "按 Aider 的角色选 Claude 模型", blocks: [
        { type: "p", text: "Aider 用的不是一个模型，而是最多三个，每个角色各有不同的性价比甜点。" },
        { type: "table", headers: ["Aider 角色", "参数", "模型", "适用场景"], rows: [
          ["主聊天模型", "--model", "anthropic/claude-sonnet-5", "日常默认；大多数会话的编码质量接近 Opus"],
          ["主模型，最难的任务", "--model", "anthropic/claude-opus-4-8", "深度多文件重构和长程智能体编辑"],
          ["弱模型", "--weak-model", "anthropic/claude-haiku-4-5", "提交信息和聊天历史摘要"],
          ["编辑模型（architect 模式）", "--editor-model", "anthropic/claude-sonnet-5", "把 architect 模型的方案落成具体 diff"],
        ] },
        { type: "p", text: "弱模型是不起眼的省钱点：Aider 每次提交、每次压缩历史都会调用它，把它指向 Haiku，就能省下那些你根本不会去看的开销。三个模型共用同一把密钥、同一个折扣——切换角色永远不需要换账户。" },
        { type: "link", text: "对比当前 Claude 模型和 token 价格", href: "/models" },
      ] },
      { h2: "为什么长 Aider 会话花这么多钱", blocks: [
        { type: "p", text: "Aider 天生烧 token，在看账单之前最好先搞清 token 都花在哪。每一轮对话都会把仓库地图、你 /add 进聊天的每个文件的完整内容、以及对话历史作为输入 token 重新发送；每次编辑又作为输出 token 返回。一次两小时的重构会话，实际是几十万 token，而不是几条聊天消息。" },
        { type: "list", items: [
          "仓库地图：整个仓库的压缩大纲，随变更重新发送。",
          "已添加的文件：你 /add 的每个文件都完整进入每次请求的提示词，直到你 /drop 它。",
          "编辑格式：diff 风格格式比整文件重写重发的代码更少。",
          "多文件编辑：每个被改动的文件都单独计输入和输出 token。",
        ] },
        { type: "p", text: "这正是统一 50% 折扣复利效应最明显的地方：按官方 token 价格要花 $10 的会话，在这里只要 $5，而且会话每多跑一小时，差距就更大。在此之上还有两个习惯可以叠加：弱模型换成 Haiku，接管源源不断的提交信息和摘要调用；diff 风格的编辑格式让输出 token 与改动量成正比，而不是与文件大小成正比。两者都不改变 Aider 的行为——只改变同样工作的花费。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户自带 $5 平台奖励余额——适用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱/密码注册的账户不享受该奖励。" },
      ] },
      { h2: "在运行中的会话里削减 token", blocks: [
        { type: "steps", items: [
          "尽早并经常使用 /tokens——Aider 会打印当前上下文的 token 数和会话累计总量，让你在发送前就看到臃肿上下文的代价。",
          "改完的文件立刻 /drop。文件会一直留在提示词里直到移除，一个被遗忘的大文件是最常见的隐形 token 泄漏。",
          "互不相关的任务之间执行 /clear。聊天历史随每条消息重发，新任务值得一个干净的上下文。",
          "便宜的问题用 /model anthropic/claude-haiku-4-5 降级处理，之后再切回来——会话中途换模型无需重启。",
          "启动 Aider 时加 --cache-prompts（或在配置里写 cache-prompts: true），让重复的文件上下文走 Anthropic 的提示词缓存，而不是每轮都按新输入计费。",
        ] },
        { type: "link", text: "用成本计算器在跑之前估算会话费用", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "排障：Aider 仍在直连 Anthropic", blocks: [
        { type: "list", items: [
          "变量是在另一个 shell 里导出的——Aider 读的是自己进程的环境，所以要在启动它的同一个 shell 里导出变量，或者写进 shell 配置文件。",
          "Aider 或 LiteLLM 版本太旧——端点覆盖逻辑在 LiteLLM 里，先执行 pip install -U aider-chat 升级，再排查其他问题。",
          "第一个请求就返回 401——密钥输错或已被吊销；端点没问题，问题出在凭证上。",
        ] },
        { type: "p", text: "要定位问题出在哪一半，可以绕过 Aider，用最小的 Messages 调用直接打网关：" },
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"claude-haiku-4-5","max_tokens":16,"messages":[{"role":"user","content":"ping"}]}'` },
        { type: "p", text: "返回 JSON 说明端点和密钥都健康，问题在 Aider 的环境；如果这里就报错，要修的是环境变量，而不是工具本身。" },
      ] },
    ],
    faq: [
      { q: "Aider 支持自定义 Claude 端点吗？", a: "支持。Aider 对 Anthropic 模型走 LiteLLM，而 LiteLLM 认 ANTHROPIC_API_BASE 环境变量——把它设为 https://router.apitoken.sale，然后正常启动 Aider 即可。" },
      { q: "Aider 里哪个 Claude 模型最好？", a: "claude-sonnet-5 是大多数编码任务的最佳默认；最难的多文件工作切到 claude-opus-4-8。把 claude-haiku-4-5 设为弱模型，让提交信息和摘要按 Haiku 价格计费——三个模型共用同一把密钥。" },
      { q: "长 Aider 会话能便宜多少？", a: "每个请求按官方 token 价格减去你的统一 50% 折扣计费，所以直连要花 $10 的会话在这里只要 $5。" },
      { q: "ANTHROPIC_API_BASE 和 ANTHROPIC_BASE_URL 是一回事吗？", a: "不是。Aider 通过 LiteLLM 访问 Anthropic，LiteLLM 读的是 ANTHROPIC_API_BASE；Claude Code 读的是 ANTHROPIC_BASE_URL。两个工具都用的话，把两个变量都导出来也无妨。" },
      { q: "能在一个 Aider 会话里混用 Claude 模型吗？", a: "可以。主聊天模型用 --model 传，杂务用 --weak-model，会话中途用 /model 切换且无需重启——一把 API 密钥覆盖所有受支持的模型。" },
      { q: "上手需要配置文件吗？", a: "不需要。首次运行两个环境变量就够了；.aider.conf.yml 只用于跨 shell 和项目固化模型选择，比如 cache-prompts 和弱模型。" },
    ],
  };
