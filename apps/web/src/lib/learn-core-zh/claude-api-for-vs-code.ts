import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "VS Code 使用 Claude API（Cline、Continue）",
    h1: "在 VS Code 中使用 Claude API",
    description: "在 VS Code 中通过 Cline 或 Continue 运行 Claude API：把 Anthropic 的 Base URL 设为 router.apitoken.sale，粘贴 apiToken.sale 密钥，按 token 计费，统一 50% 折扣。",
    keywords: ["claude api vs code", "cline claude api", "continue claude api", "vscode 使用 claude", "vscode claude api 密钥", "claude api key", "anthropic 兼容 api", "claude api base url", "cline 自定义 base url", "continue 配置 claude", "vscode 配置 claude api"],
    dek: "在 VS Code 里配置 Claude API，说到底只有一个设置：Cline、Continue 这类免费扩展接受任何兼容 Anthropic 的端点。把其中一个指向 https://router.apitoken.sale 并配上 apiToken.sale 密钥，Claude 就能在编辑器里修改代码、回答问题、审查改动，按 token 从预付余额扣费，价格为官方 API 定价的 50% 折扣。",
    sections: [
      { h2: "一个 Base URL 加一把密钥，就是全部集成工作", blocks: [
        { type: "p", text: "在 VS Code 里运行 Claude 并不需要 Anthropic Console 账户。免费的 Cline 和 Continue 扩展都接受任何兼容 Anthropic 的端点，所以全部工作就是：把它们指向 https://router.apitoken.sale，粘贴你的 apiToken.sale 密钥，选一个模型。从此 Claude 不离开编辑器就能回答问题、修改文件、审查 diff，按 token 从你的预付余额扣费，统一按官方 API 定价的 50% 折扣计费。" },
        { type: "p", text: "底层没有任何黑科技。扩展以普通的 Anthropic Messages 请求携带 x-api-key 头发送到网关；网关验证你的 sk-pool-… 密钥，把调用路由到所请求的 Claude 模型并计量 token。因为线上协议原封不动，这些扩展依赖的能力——流式响应、工具调用、大上下文窗口——表现与直连 Anthropic 官方端点完全一致。" },
        { type: "table", headers: ["配置项", "填写内容"], rows: [
          ["API 提供方", "Anthropic（两个扩展均内置）"],
          ["Base URL", "https://router.apitoken.sale"],
          ["API 密钥", "sk-pool-••• —— 在 apiToken.sale 控制台生成"],
          ["模型", "难题用 claude-opus-4-8，其余一律 claude-sonnet-5"],
          ["费用", "按 token 从预付余额扣费，官方价格 50% 折扣"],
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台奖励余额——适用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "四步装好 Cline", blocks: [
        { type: "steps", items: [
          "从 VS Code Marketplace 安装 Cline，点齿轮图标打开设置。",
          "把 API Provider 设为 Anthropic，并启用自定义 Base URL 选项。",
          "在 Base URL 处粘贴 https://router.apitoken.sale，下方填你的 sk-pool-••• 密钥。",
          "模型填 claude-opus-4-8，然后先跑一个无关紧要的小任务确认密钥可用，再交给它真正的任务。",
        ] },
        { type: "code", code: `# Cline → Settings\nAPI Provider : Anthropic\nBase URL     : https://router.apitoken.sale\nAPI Key      : sk-pool-•••\nModel        : claude-opus-4-8` },
      ] },
      { h2: "让 Cline 的自主会话更省钱", blocks: [
        { type: "p", text: "想要自主编辑时，Cline 是强有力的默认选择：给它一个任务，它会读文件、做计划、应用修改、执行终端命令，并循环复查自己的 diff，直到任务完成。这个循环正是按 token 计费的意义所在——一个任务可能意味着几十次 Messages 调用，而在折扣网关上，同样的会话只花官方价格的一半。" },
        { type: "list", items: [
          "提示词缓存按官方更低的缓存费率再叠加你的折扣计费，跨轮次重复读取同样的大文件，成本只是原价的一小部分。",
          "把日常杂活交给 claude-sonnet-5，把 claude-opus-4-8 留给真正需要它的难题——会话中途即可切换模型，无需改动密钥。",
          "盯紧控制台：它按请求展示 token 级用量，失控的智能体循环在吞掉余额之前就能被发现。",
        ] },
      ] },
      { h2: "把 Continue 指向同一个网关", blocks: [
        { type: "p", text: "Continue 是更轻量的选择：它擅长内联聊天、快速修改和编辑器内答疑，而不是跨文件的自主任务；配置集中在一个文件里，可以提交进仓库，也可以在多台机器间同步。和 Cline 一样，它免费——只有 API 用量会动你的余额。" },
        { type: "code", code: `# ~/.continue/config.yaml\nname: local\nversion: 1.0.0\nschema: v1\nmodels:\n  - name: Claude Opus 4.8 (apiToken.sale)\n    provider: anthropic\n    model: claude-opus-4-8\n    apiBase: https://router.apitoken.sale\n    apiKey: sk-pool-•••\n    roles: [chat, edit, apply]` },
        { type: "note", text: `旧版 Continue 读取的是 ~/.continue/config.json；等价配置是设置 "provider": "anthropic"、"apiBase": "https://router.apitoken.sale"、"apiKey" 和 "model"，依然有效。autocomplete 角色留给一个专用的小模型——Claude 的 token 要花在 chat、edit 和 apply 上。` },
      ] },
      { h2: "Cline 还是 Continue：按工作流选，而不是按价格", blocks: [
        { type: "p", text: "两个扩展都免费，也都从同一份预付余额扣费，所以选择完全取决于你的工作习惯。很多开发者用一把密钥把两个都装上：Cline 负责委派任务，Continue 负责快速提问和内联改写。" },
        { type: "table", headers: ["", "Cline", "Continue"], rows: [
          ["最擅长", "自主完成跨文件任务", "内联聊天与快速修改"],
          ["交互方式", "带审批的规划/执行循环", "侧边栏聊天与内联编辑"],
          ["token 消耗特征", "较高——读/改/验证循环", "较低——多为单轮提示"],
          ["扩展价格", "免费", "免费"],
        ] },
        { type: "p", text: "同一把 sk-pool-… 密钥还能同时用于 Cursor、Roo Code 和 Anthropic SDK——所有工具共用一份余额。如果你在 VS Code 和 Cursor 之间切换使用，Cursor 指南讲的是同样的两分钟流程，模型目录则列出这把密钥解锁的所有模型。" },
        { type: "link", text: "用于 Cursor 的 Claude API 密钥", href: "/docs/learn/claude-api-key-for-cursor" },
        { type: "link", text: "模型目录与按 token 计费价格", href: "/models" },
      ] },
      { h2: "人人都会踩的三个报错", blocks: [
        { type: "list", items: [
          "401 Unauthorized：密钥或 Base URL 填错了。重新完整粘贴 sk-pool-… 密钥，并确认 Base URL 一字不差就是 https://router.apitoken.sale，没有多余的路径段，也没有笔误。",
          "找不到模型：扩展内置的模型列表更新滞后。手动输入当前的模型 ID——claude-sonnet-5 或 claude-opus-4-8——而不是选择过时的条目。",
          "响应缓慢或 429：触发了速率限制。降低扩展的并发，重新提交前遵守 Retry-After 头。",
        ] },
        { type: "p", text: "如果 Cline 任务中途请求失败，不要直接重跑整个任务——智能体会保留自己的计划，修好密钥或模型 ID 后从断点继续，可以避免为同一份上下文重复付费。" },
      ] },
    ],
    faq: [
      { q: "哪些 VS Code 扩展可以用 apiToken.sale 密钥？", a: "任何支持兼容 Anthropic 端点的扩展都可以，包括 Cline 和 Continue。把提供方设为 Anthropic，把 Base URL 覆盖为 https://router.apitoken.sale，再粘贴你的密钥即可。" },
      { q: "Cline 或 Continue 扩展本身要付费吗？", a: "不用。两个扩展都是免费的；你只需为 Claude API 用量付费，按 token 从预付余额扣除，统一按官方定价的 50% 折扣计费。通过 Google 或 GitHub 创建的账户自带 $5 平台奖励余额。" },
      { q: "在 VS Code 里用 Claude 应该填什么 Base URL？", a: "把 Anthropic 提供方的自定义 Base URL 设为 https://router.apitoken.sale，并使用 apiToken.sale 控制台里的 sk-pool-… 密钥。Cline 和 Continue 里的设置完全相同。" },
      { q: "为什么扩展提示找不到模型？", a: "扩展内置的模型列表更新滞后。请手动输入当前的模型 ID——claude-sonnet-5 或 claude-opus-4-8——而不是选择下拉里过时的条目。" },
      { q: "同一把密钥可以同时在 VS Code 和 Cursor 里用吗？", a: "可以。一把密钥可同时用于 Cline、Continue、Cursor、Roo Code 和 Anthropic SDK，全部从同一份预付余额扣费，控制台里能看到 token 级用量。" },
    ],
  };
