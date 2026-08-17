import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
  title: "apiToken.sale vs LiteLLM：Claude 场景怎么选",
  h1: "apiToken.sale 对比 LiteLLM",
  description: "LiteLLM 是自托管代理，把各模型 API 统一到你自行充值的密钥之上；apiToken.sale 是托管端点，直接以 50% 折扣出售密钥和余额。两者可以对比，也可以组合使用。",
  keywords: ["litellm 替代品", "apitoken vs litellm", "litellm claude", "自托管 llm 代理", "litellm proxy 对比托管 api", "claude api 免自托管", "litellm api_base anthropic", "claude api 折扣", "托管 claude api 端点", "便宜 claude api"],
  dek: "找 LiteLLM 替代品，通常意味着你想要两样东西之一：一层不用自己跑代理的统一 API，或者更便宜的 Claude token。apiToken.sale 两者都给——一个托管端点，一把预付密钥覆盖支持的 Claude、GPT、Gemini 和 Kimi 模型，统一 50% B2C 折扣。而当你确实想自己掌控路由层时，LiteLLM 仍然是更好的选择。",
  sections: [
    { h2: "一句话结论：自己跑的代理 vs 直接指过去的端点", blocks: [
      { type: "p", text: "LiteLLM 是软件——一个开源代理，由你部署在你自己充值的提供方账户前面。apiToken.sale 是服务——一个托管的预付端点，密钥和余额本身就是产品。如果你的目标是零基础设施拿到折扣的 Claude 访问，单靠 LiteLLM 做不到；如果你的目标是拥有一个跨多提供方的路由层，apiToken.sale 也不打算做这件事。" },
      { type: "table", headers: ["", "LiteLLM", "apiToken.sale"], rows: [
        ["它是什么", "自托管的代理库和服务器", "托管的多提供方 API 端点"],
        ["谁来运维基础设施", "你自己：进程、可用性、升级", "apiToken.sale"],
        ["密钥从哪来", "你自己开通并充值每个提供方账户", "一把预付密钥覆盖支持的 Claude、GPT、Gemini 和 Kimi 模型"],
        ["Claude 协议", "取决于你配置的上游", "在 https://router.apitoken.sale 上的原生 Anthropic Messages API，使用 x-api-key"],
        ["对 Claude 成本的影响", "没有——上游按标价收费", "在官方提供方费率上统一 50% B2C 折扣"],
        ["最适合", "想把众多提供方统一到一个内部网关后面的团队", "想要 Claude 访问、不想运维任何东西的开发者"],
      ] },
    ] },
    { h2: "LiteLLM 能给你什么——以及它永远给不了什么", blocks: [
      { type: "p", text: "LiteLLM 解决的是集成问题，不是采购问题。它把几十家提供方的 API 归一到一种 OpenAI 风格的调用形态，代理模式还在你自己的部署里加上路由、重试、回退、虚拟密钥和按密钥的花费追踪。当多个团队共用一个网关时，这确实有用。" },
      { type: "p", text: "它做不到的是让底层 token 变便宜。代理后面的每一把上游密钥仍然是你自己的账户，由 Anthropic、OpenAI 或 Google 按标价计费。代理夹在你和账单之间，但它没法把账单变小。" },
      { type: "list", items: [
        "提供方账户、充值和配额管理仍然归你。",
        "代理进程本身的托管、打补丁和安全加固也归你。",
        "没有任何折扣机制——成本原样透传。",
      ] },
    ] },
    { h2: "50% 的折扣到底从哪来", blocks: [
      { type: "p", text: "折扣不是路由技巧。apiToken.sale 持有一个汇集的预付余额，按官方提供方费率卡对每次请求计量——输入、输出和缓存 token——然后在从你的余额扣费之前，先减去统一的 50% B2C 折扣。相比之下 LiteLLM 是成本中性的：它转发请求，上游收多少就是多少。" },
      { type: "p", text: "所以这个对比对两个工具都略显不公平。LiteLLM 决定请求去哪；apiToken.sale 决定请求花多少钱。它们工作在不同层面，这也是为什么两者能很好地组合。" },
      { type: "note", text: "折扣跟着密钥走，不跟着客户端走。直接调 Anthropic SDK、curl、编程 agent，或者在前面架一个 LiteLLM 代理——收费都是同一个计量后减半的金额，在 apiToken.sale 控制台里按请求可见。" },
    ] },
    { h2: "混合方案：在 apiToken.sale 密钥前面架 LiteLLM", blocks: [
      { type: "p", text: "如果你已经围绕 LiteLLM 的接口做了标准化，要拿折扣也不必放弃它。把 apiToken.sale 声明为 Anthropic 上游，经过你代理的每次 Claude 调用都会落到折扣端点上：" },
      { type: "code", code: `# config.yaml\nmodel_list:\n  - model_name: claude-opus-4-8\n    litellm_params:\n      model: anthropic/claude-opus-4-8\n      api_base: https://router.apitoken.sale\n      api_key: sk-pool-•••  # or os.environ/APITOKEN_KEY` },
      { type: "steps", items: [
        "照常安装代理：pip install 'litellm[proxy]'。",
        "保存上面的配置。保留 anthropic/ 模型前缀——正是它让 LiteLLM 用 Anthropic Messages API 跟端点通信。",
        "启动：litellm --config config.yaml。代理默认监听 http://localhost:4000。",
        "把现有的 LiteLLM 客户端指向模型名 claude-opus-4-8。请求会带着你的 sk-pool 密钥发往 router.apitoken.sale，50% 折扣在 apiToken.sale 一侧生效。",
      ] },
      { type: "note", text: "别把密钥提交进仓库——LiteLLM 的 os.environ/VARIABLE 语法可以从环境变量读取。还要注意职责划分：LiteLLM 自己的花费追踪显示的是代理转发了多少，但权威收费以 apiToken.sale 控制台里的 token 级计量为准。" },
    ] },
    { h2: "LiteLLM 开给你的运维账单", blocks: [
      { type: "p", text: "自托管代理是一项实打实的投入，如果是出于成本原因选它，值得先诚实算一笔账。总得有人让进程活着、升级版本、轮换主密钥、保管每一个上游提供方的密钥，并在流量增长时扩容。对一个只是想在编辑器或 agent 循环里用 Claude 的独立开发者来说，这些开销换不来任何东西。" },
      { type: "p", text: "用 apiToken.sale，整个集成就是一个 base URL 加一把密钥：https://router.apitoken.sale 上的原生 Anthropic Messages 端点配 x-api-key 请求头；或者 https://router.apitoken.sale/v1 上的 OpenAI 兼容通道配 Authorization: Bearer，给只认这个协议的工具用。Claude Code、Cursor、Anthropic SDK 和任何 OpenAI 形态的工具都能直接连，中间不需要适配层。" },
      { type: "link", text: "查看一把密钥覆盖的模型", href: "/models" },
    ] },
    { h2: "怎么选", blocks: [
      { type: "list", items: [
        "如果你想要托管、带折扣的 Claude 访问，且唯一愿意做的改动就是换一个 base URL 和密钥——选 apiToken.sale。",
        "如果你确实想拥有一个跨多提供方的统一路由层，并接受自己充值、自己运维全部环节——选 LiteLLM。",
        "如果你已经依赖 LiteLLM 的接口——两个都用：在它后面放一把 apiToken.sale 密钥，把折扣留在底层。",
      ] },
      { type: "note", text: "通过 Google 或 GitHub 创建的新账户自带 $5 平台奖励余额——可用于支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
    ] },
  ],
  faq: [
    { q: "LiteLLM 会给 Claude API 打折吗？", a: "不会。LiteLLM 路由到你自己按标价充值的提供方账户。50% 的折扣来自 apiToken.sale 汇集的预付余额，无论哪个客户端发出请求，它都对官方提供方费率生效。" },
    { q: "用 apiToken.sale 需要自己托管什么吗？", a: "不需要——它是托管端点。你把 base URL 改成 https://router.apitoken.sale，用上你的 sk-pool 密钥即可；没有代理进程、容器或服务器要跑。" },
    { q: "LiteLLM 能搭配 apiToken.sale 密钥用吗？", a: "可以。在 litellm_params 里设 model: anthropic/claude-opus-4-8、api_base: https://router.apitoken.sale 和你的密钥，经过 LiteLLM 代理的 Claude 调用就按折扣价计费。" },
    { q: "LiteLLM 免费吗？", a: "软件是开源的，但「免费」有误导性：你仍要按标价付给每个上游提供方，外加代理本身的基础设施和维护。而 token 成本——开销的大头——正是 apiToken.sale 砍掉一半的那部分。" },
    { q: "Claude Code 或 Cursor 选哪个方案更好？", a: "把工具直接指向 apiToken.sale 更简单：一个 base URL 加一把密钥，原生 Anthropic 协议，没有额外一跳。只有在你已经因为别的原因跑 LiteLLM 时——比如团队共享虚拟密钥——在中间加它才有意义。" },
  ],
};
