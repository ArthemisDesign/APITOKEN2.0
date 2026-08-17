import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "用 apiToken.sale 配置 Codex CLI——GPT-5.6 配置档",
    h1: "在 apiToken.sale 上运行 Codex CLI",
    description: "用指向 apiToken.sale OpenAI 兼容端点的命名 model_providers 配置档配置 Codex CLI——GPT-5.6 模型按预付余额统一 50% 折扣计费，无需 ChatGPT 账户。",
    keywords: ["codex cli 配置", "codex config.toml", "codex 自定义模型提供商", "codex api 密钥", "codex cli gpt-5.6", "codex responses api", "codex cli 无需 chatgpt"],
    dek: "只要给 Codex CLI 一个自定义模型提供商，它就能完全以 API 密钥认证运行。一个 TOML 配置档把它指向 apiToken.sale，预付余额覆盖每次会话——无需登录 ChatGPT，比官方费用统一便宜 50%。",
    sections: [
      { h2: "创建配置档", blocks: [
        { type: "p", text: "将以下内容保存为 ~/.codex/apitoken.config.toml。命名配置档不会改动你的默认 Codex 配置和可能的 ChatGPT 登录——每次运行显式选择启用。" },
        { type: "code", code: `# ~/.codex/apitoken.config.toml\nmodel = "gpt-5.6-sol"\nmodel_provider = "apitoken"\n\n[model_providers.apitoken]\nname = "apiToken.sale"\nbase_url = "https://router.apitoken.sale/v1"\nwire_api = "responses"\nenv_key = "APITOKEN_API_KEY"` },
        { type: "p", text: "env_key 指定 Codex 读取密钥的环境变量名——密钥留在 shell 中，绝不写入 TOML 文件。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额——适用于支持的 Claude、GPT、Gemini 与 Kimi 模型；邮箱密码账户不参与。" },
      ] },
      { h2: "运行与验证", blocks: [
        { type: "code", code: `export APITOKEN_API_KEY=sk-pool-•••\ncodex --profile apitoken` },
        { type: "list", items: [
          "始终显式传入 --profile apitoken，明确哪个提供商——以及哪个环境变量——在生效。",
          "按项目修改 model 行切换模型：最重的任务用 gpt-5.6-sol，日常使用 gpt-5.6-terra，快速便宜的步骤用 gpt-5.6-luna。",
          "用同一个 Bearer 密钥 GET https://router.apitoken.sale/v1/models 可列出当前启用的模型集——统一目录按提供方命名 ID（anthropic/*、openai/*、google/*）。",
        ] },
        { type: "note", text: "wire_api = \"responses\" 是本网关的正确值——它同时提供 Responses 和 Chat Completions，而 Codex 通过 Responses 流式输出。只有当特定客户端要求经典形状时才设为 \"chat\"。" },
      ] },
      { h2: "可能遇到的错误", blocks: [
        { type: "list", items: [
          "Missing APITOKEN_API_KEY——env_key 指定的变量没有导出到运行 codex 的 shell。请在同一个 shell（或 shell 配置文件）中导出。",
          "stream error: unexpected status 401——密钥错误、已吊销，或 base_url 丢了 /v1 后缀。在 Codex 之外用 curl 复现以定位问题所在。",
          "stream error: unexpected status 404——模型 ID 未启用；请查询 GET https://router.apitoken.sale/v1/models 而不是凭空猜测。",
          "402——共享预付余额需要充值；等待无法解决。",
        ] },
        { type: "link", text: "完整的 Codex 错误手册——config.toml、auth.json、流式错误", href: "/errors/codex" },
      ] },
    ],
    faq: [
      { q: "需要 ChatGPT 账户或订阅吗？", a: "不需要。配置好自定义 model_providers 配置档并把提供商的 API 密钥放入环境后，Codex 完全以 API 密钥认证运行——auth.json 里的 ChatGPT 登录与此无关。" },
      { q: "这会改变我的默认 Codex 配置吗？", a: "不会。配置档在独立文件中，只有传入 --profile apitoken 时才生效。默认配置和登录保持不变。" },
      { q: "折扣与 Claude 相同吗？", a: "相同。GPT-5.6 用量按官方 OpenAI token 费率计量，你的 B2C 统一 50% 折扣作用于同一个预付余额。" },
      { q: "wire_api 选 Responses 还是 Chat Completions？", a: "用 wire_api = \"responses\"——网关两者都提供，而 Codex 围绕 Responses 流构建。Chat Completions 形状是为需要的客户端准备的。" },
    ],
  };
