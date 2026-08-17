import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Codex CLI 配置：接入 apiToken.sale 自定义 GPT-5.6 提供商",
    h1: "Codex CLI 配置：为 apiToken.sale 写一个自定义提供商配置档",
    description: "Codex CLI 配置无需 ChatGPT 登录：一个 model_providers 配置档把 Codex 指向 apiToken.sale 的 OpenAI 兼容端点，GPT-5.6 模型用预付余额计费，统一 50% 折扣。",
    keywords: ["codex cli 配置", "codex 自定义模型提供商", "codex config.toml profile", "codex cli api key", "codex cli 无需 chatgpt", "codex cli gpt-5.6", "codex responses api", "codex base_url", "codex cli setup"],
    dek: "Codex CLI 的配置归结为一个 TOML 配置档：声明自定义模型提供商、把 base_url 指向 apiToken.sale、指定存放密钥的环境变量名。之后 Codex 完全以 API 密钥认证运行 GPT-5.6 模型，走预付余额扣费——无需 ChatGPT 登录，支出比 OpenAI 官方统一低 50%。",
    sections: [
      { h2: "Codex CLI 不需要 ChatGPT 账户", blocks: [
        { type: "p", text: "Codex 的认证方式由当前启用的模型提供商决定。在 model_providers 表里定义一个自定义提供商、导出它指定的 API 密钥，Codex 就不会再看 auth.json 里的 ChatGPT 登录——每个请求都用你的密钥签名，由端点归属方计费。把端点指向 apiToken.sale，每次会话就从同一个预付余额扣费：按 OpenAI 官方 token 费率计量，再叠加统一的 50% B2C 折扣。" },
        { type: "p", text: "干净的做法是用命名配置档，而不是改动主配置。配置档独立存放在自己的文件里，你的默认 Codex 配置和已有的 ChatGPT 登录原样保留，每次运行用一个 flag 显式启用。删掉这个文件，环境里就不会再留下任何 apiToken.sale 的痕迹。" },
        { type: "note", text: "通过 Google 或 GitHub 注册的新账户自带 $5 平台奖励余额——适用于支持的 Claude、GPT、Gemini 与 Kimi 模型；邮箱密码注册的账户不享受该奖励。" },
      ] },
      { h2: "一次写好 apitoken 配置档", blocks: [
        { type: "p", text: "将以下内容保存为 ~/.codex/apitoken.config.toml。它声明了提供商、端点、wire 协议，以及 Codex 读取密钥的环境变量：" },
        { type: "code", code: `# ~/.codex/apitoken.config.toml\nmodel = "gpt-5.6-sol"\nmodel_provider = "apitoken"\n\n[model_providers.apitoken]\nname = "apiToken.sale"\nbase_url = "https://router.apitoken.sale/v1"\nwire_api = "responses"\nenv_key = "APITOKEN_API_KEY"` },
        { type: "p", text: "两行承载了安全姿态。env_key 指定变量名而不是存放密钥本身，密钥留在 shell 里，绝不写进可能被提交的文件。base_url 必须保留 /v1 后缀——丢掉它是首次运行失败最常见的原因，因为 Codex 调用的每个路由都挂在这个前缀下。" },
        { type: "note", text: "保持 wire_api = \"responses\"。网关同时提供 Responses API 和 Chat Completions，而 Codex 是围绕 Responses 流构建的。只有共享该文件的其他客户端要求经典格式时才改成 \"chat\"。" },
      ] },
      { h2: "导出密钥、查目录、运行", blocks: [
        { type: "steps", items: [
          "在将要启动 Codex 的 shell 里导出密钥：export APITOKEN_API_KEY=sk-pool-•••——想永久生效就把同一行写进 shell 配置文件。",
          "先确认启用了什么再猜模型 ID：用同一个 Bearer 密钥请求 curl https://router.apitoken.sale/v1/models，返回的是实时目录。",
          "用配置档 flag 启动：codex --profile apitoken。显式传 flag 可以消除本次会话用哪个提供商——以及哪个环境变量——的一切歧义。",
          "先发一个小提示词。一次干净的回答能在一个往返里同时验证密钥、base_url 和余额；这一步出问题，排查成本也最低。",
        ] },
        { type: "code", code: `export APITOKEN_API_KEY=sk-pool-•••\n\ncurl https://router.apitoken.sale/v1/models \\\n  -H "Authorization: Bearer $APITOKEN_API_KEY"\n\ncodex --profile apitoken` },
        { type: "note", text: "目录接口回答的是整个网关，不只是 GPT：统一目录按提供方给 ID 划分命名空间（anthropic/*、openai/*、google/*）。同一把密钥和余额也覆盖支持的 Claude、Gemini 与 Kimi 模型——Codex 只会调用它配置档指向的那个提供商。" },
      ] },
      { h2: "按会话选对 GPT-5.6 档位", blocks: [
        { type: "p", text: "配置档里的 model 行是默认值，不是承诺——按项目改它。GPT-5.6 设三档，是因为智能体编码在不同推理强度下烧 token 的速度差别很大：" },
        { type: "table", headers: ["模型 ID", "档位", "官方输入 / 输出（$ / 1M）", "缓存输入"], rows: [
          ["gpt-5.6-sol", "旗舰", "$5 / $30", "$0.50"],
          ["gpt-5.6-terra", "均衡", "$2 / $12", "$0.20"],
          ["gpt-5.6-luna", "快速", "$0.20 / $1.20", "$0.02"],
        ] },
        { type: "list", items: [
          "gpt-5.6-sol 用于最重的活：多文件重构、隐蔽 bug 的调试，以及任何答错比 token 更贵的场景。",
          "gpt-5.6-terra 作为日常主力——大多数 Codex 会话应该默认这一档。",
          "gpt-5.6-luna 用于又快又省的步骤：样板代码、重命名、一次性脚本，以及延迟比深度更重要的高频循环。",
          "缓存输入是智能体循环真正省钱的地方——重复的上下文读取按缓存费率计费，50% 折扣再叠加其上。",
        ] },
        { type: "link", text: "查看全部模型的完整规格与折后价格", href: "/models" },
      ] },
      { h2: "Codex 真正会报给你的四个错误", blocks: [
        { type: "list", items: [
          "Missing APITOKEN_API_KEY——env_key 指定的变量没有在运行 codex 的 shell 里导出。在同一个 shell（或 shell 配置文件）里导出后重试。",
          "stream error: unexpected status 401——密钥错误、已吊销，或 base_url 丢了 /v1 后缀。在 Codex 之外用 curl 复现调用，定位坏的是哪一半。",
          "stream error: unexpected status 404——模型 ID 未启用。查 GET https://router.apitoken.sale/v1/models，不要假设你敲的 ID 存在。",
          "402——共享预付余额需要充值。退避重试解决不了；充值后下一个请求即可成功。",
        ] },
        { type: "p", text: "这四个都是配置或余额问题，不是模型问题——没有一个靠重复同一条命令能解决。其中 401 几乎总能归结为 /v1 后缀丢失，或密钥里多粘了一个字符。" },
        { type: "link", text: "完整的 Codex 错误手册——config.toml、auth.json、流式错误", href: "/errors/codex" },
      ] },
      { h2: "预付余额下一次 Codex 会话花多少钱", blocks: [
        { type: "p", text: "按 token 计费，费率与 OpenAI 官方一致，你的统一 50% B2C 折扣在扣预付余额之前就已减去——与平台上 Claude 用量的计费规则相同。没有订阅费也没有席位费：闲置一周不花一分钱，重度会话恰好按它消耗的 token 计费，即官方支出的一半。" },
        { type: "p", text: "余额在支持的 Claude、GPT、Gemini 与 Kimi 模型之间共享，Codex 会话和你跑的其他所有东西从同一个池子里扣。在控制台关注用量，把 402 当作它本来的信号——余额见底了，别的什么都没坏。" },
      ] },
    ],
    faq: [
      { q: "Codex CLI 需要 ChatGPT 账户或订阅吗？", a: "不需要。配置好自定义 model_providers 配置档、把提供商的 API 密钥放进环境变量后，Codex 完全以 API 密钥认证运行——auth.json 里的 ChatGPT 登录与此无关。" },
      { q: "这个配置档会改动我的默认 Codex 配置吗？", a: "不会。配置档独立存放，只有传入 --profile apitoken 时才启用。你的默认配置和已有的 ChatGPT 登录保持原样。" },
      { q: "GPT-5.6 的折扣和 Claude 的一样吗？", a: "一样。GPT-5.6 用量按 OpenAI 官方 token 费率计量，你的统一 50% B2C 折扣作用于同一个预付余额。" },
      { q: "wire_api 该用 responses 还是 chat？", a: "用 wire_api = \"responses\"——网关同时提供 Responses API 和 Chat Completions，而 Codex 围绕 Responses 流构建。chat 值是为要求经典格式的客户端准备的。" },
      { q: "不改配置档能切换 GPT-5.6 模型吗？", a: "配置档里的 model 行设定的是默认值；按项目编辑这一行就是在 gpt-5.6-sol、gpt-5.6-terra 和 gpt-5.6-luna 之间切换的受支持方式。" },
      { q: "会话中途报 402 是什么意思？", a: "共享预付余额用完了，需要充值。退避重试没有用——充值后下一个请求就会通过。" },
    ],
  };
