import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "OpenCode 接入 Kimi API：用路由器插件运行 K3 与 Kimi for Coding",
    h1: "在 OpenCode 中运行 Kimi K3 与 Kimi for Coding",
    description: "通过 apiToken.sale 路由器插件在 OpenCode 中配置 Kimi API：按密钥作用域的实时模型目录、明确的 apitoken/kimi/* 模型 ID、真实的 K3 与 Kimi for Coding 费率，一个预付费密钥即可使用。",
    keywords: ["kimi opencode", "kimi api opencode", "opencode 接入 kimi", "opencode 配置 kimi api", "kimi k3 opencode", "kimi for coding 配置", "opencode 自定义 provider", "kimi 编程代理", "opencode router 插件", "opencode.jsonc provider", "opencode models apitoken"],
    dek: "OpenCode 通过一个 apiToken.sale 配置插件运行 Kimi API：安装器在路由器的 OpenAI 兼容通道上注册 apitoken provider，插件在每次启动时按密钥作用域的实时目录重建模型列表。K3 和 Kimi for Coding 都以 apitoken/kimi/{model} 的形式明确寻址，用量与 Claude、GPT、Gemini 共用同一个预付费余额结算。",
    sections: [
      { h2: "一个插件，代替手写的 provider 列表", blocks: [
        { type: "p", text: "“OpenCode 里用 Kimi”的直接答案：安装 apiToken.sale 路由器插件，重启，然后选择一个明确的 apitoken/kimi/* 模型。没有需要维护的静态 provider 配置块——每次启动时，插件都会拉取你个人的 GET /v1/models 响应，把其中的权威限制、能力和当前价格翻译成 OpenCode 原生的模型 schema。目录没有为你的密钥返回的模型，根本不会出现在选择器里。" },
        { type: "p", text: "安装器会在 OpenCode 的全局自动加载目录放一个小型 loader，写入一份经过验证的 fallback 运行时，并把一个 apitoken provider 合并进 ~/.config/opencode/opencode.jsonc——同时备份原有内容。这个 provider 条目通过 @ai-sdk/openai-compatible 对接路由器的 /v1 通道，密钥可以是字面的 sk-pool-…，也可以是 OpenCode 标准的 {env:NAME} 占位符：" },
        sourceBlock("kimi-api-for-opencode", 0, 2),
        { type: "note", text: "优先使用 {env:APITOKEN_API_KEY} 占位符，而不是直接粘贴密钥：这样密钥只存在于你的 shell 配置里，而不是一个可能被提交或同步的配置文件。" },
      ] },
      { h2: "安装并验证连接", blocks: [
        { type: "steps", items: [
          "运行安装器；它会把 apitoken provider 合并进现有的 opencode.jsonc，并保留备份。",
          "如果你选择了占位符形式，在启动 OpenCode 的 shell 中 export APITOKEN_API_KEY，然后重启 OpenCode，让插件拉取按密钥作用域的目录。",
          "列出你的密钥实际能看到的模型：opencode models apitoken。这份输出——而不是某篇博客——才是可用 Kimi ID 的事实来源。",
          "用一个明确命名空间的模型跑一个确定性提示词。一次干净的回答就能同时验证密钥、base URL 和余额。",
        ] },
        sourceBlock("kimi-api-for-opencode", 1, 1),
      ] },
      { h2: "在四个 Kimi 别名之间做选择", blocks: [
        { type: "p", text: "模型访问由目录驱动，所以在项目中固定某个别名之前，先用 opencode models apitoken 确认它存在。四个别名共用一个余额；区别在于成本、上下文和延迟。以下数字均为每 1M token，官方费率叠加 apiToken.sale 统一的 50% 折扣：" },
        { type: "table", headers: ["OpenCode 模型 ID", "定位", "官方 命中 / 未命中 / 输出", "五折后实付"], rows: [
          ["apitoken/kimi/kimi-for-coding", "经济型编程默认", "$0.19 / $0.95 / $4", "$0.095 / $0.475 / $2"],
          ["apitoken/kimi/kimi-for-coding-highspeed", "延迟更低，费率恰好翻倍", "$0.38 / $1.90 / $8", "$0.19 / $0.95 / $4"],
          ["apitoken/kimi/k3-256k", "256K 上下文模式下的 K3 推理", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
          ["apitoken/kimi/k3", "完整 1M 上下文的 K3", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
        ] },
        { type: "list", items: [
          "日常 agent 循环默认用 kimi-for-coding——Kimi 已发布模型中通用编程费率最低的一档。",
          "只有当延迟明显值回票价时才用 highspeed；它的每一档费率都恰好是基础别名的两倍。",
          "想要 K3 推理但不需要 1M 上下文模式时用 k3-256k；目录为长代码库开放完整窗口时用 k3。",
          "Kimi 缓存是自动的：重复上下文按命中费率计费，新写入缓存的 token 算未命中，推理 token 是输出 token 的子集、按输出费率计费——永远不存在第四个计费档。",
        ] },
        { type: "link", text: "深入 Kimi 计费机制：缓存档位、别名与 High Speed", href: "/docs/learn/kimi-api-pricing" },
      ] },
      { h2: "为什么实时目录胜过静态模型列表", blocks: [
        { type: "p", text: "手写的 provider 配置会把模型 ID、上下文限制和价格冻结在你敲下它们的那一天。插件则在每次 OpenCode 启动时重新读取按密钥作用域的 /v1/models，已下线或不可用的别名不会滞留在本地配置里，限制也来自路由器的权威字段，而不是对模型名的子串猜测。因为目录是按你的密钥作用域生成的，它只会提供当前确实可路由、且已为你定价的模型。" },
        { type: "p", text: "如果目录暂时不可达，插件会回退到一份加密的本地 last-good 快照——AES-256-GCM、文件权限 0600、绑定到确切的凭证和 base URL，15 分钟内视为新鲜，最多可复用 7 天。来自快照的模型会明确标注 \"[stale metadata; pricing unavailable]\"，且绝不展示缓存中的成本：价格只有在下一次实时发现成功后才会重新出现。" },
        { type: "note", text: "不要把 Kimi 的内部资费 ID（例如 kimi-k2.7-code）粘贴进 OpenCode。路由器只接受目录返回的公开订阅别名，插件注册的也正是这些别名。" },
      ] },
      { h2: "Kimi 会话实际会抛出的三种错误", blocks: [
        { type: "list", items: [
          "401——密钥错误、已被吊销，或 baseURL 丢了 /v1 后缀。在 OpenCode 之外用 curl 请求 https://router.apitoken.sale/v1/models 复现一次，就能定位是哪一半出了问题。",
          "404——该模型 ID 当前未对你的密钥开放。先查 opencode models apitoken，别想当然认为你输入的别名存在。",
          "402——共享预付费余额耗尽。带退避的重试无济于事；充值后下一个请求就会成功。",
        ] },
        { type: "p", text: "这三种都是配置或余额问题，不是模型问题——重发同一个提示词解决不了其中任何一个。401 尤其如此，几乎总能归结于缺少 /v1 后缀，或粘贴密钥时多了一个字符。" },
      ] },
      { h2: "OpenCode 会话在预付费余额上的成本", blocks: [
        { type: "p", text: "按 token 计费，官方 Kimi 费率先减去统一的 50% 折扣，再从你的预付费余额扣款。没有订阅费，也没有席位费：闲置一周分文不花，一次重度重构会话也只按实际消耗的 token、以官方半价结算。余额在受支持的 Claude、GPT、Gemini 和 Kimi 命名空间之间共享，OpenCode 会话与你运行的其他所有负载从同一个资金池扣费。" },
        { type: "list", items: [
          "用银行卡或加密货币充值任意整数美元金额——你这边不需要单独的 Kimi 套餐。",
          "在密钥上设置累计消费上限，并在控制台查看已结算用量；402 就是额度用完了，没有其他东西坏掉。",
          "长 agent 循环保持在 kimi-for-coding 上，只有困难推理或长上下文任务才升级到 k3——真正的节省就在这种分工里。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
        { type: "link", text: "查看各模型完整规格与折后价格", href: "/models" },
      ] },
      { h2: "如果你也在 Claude Code 或 Kimi Code 中使用 Kimi", blocks: [
        { type: "p", text: "同一个密钥在其他编程 agent 里也能用，但配置方式不同。Claude Code 对接路由器的 Anthropic Messages 端点，需要把每个模型层级都固定住——main、Opus、Sonnet、Haiku 以及 subagent 模型变量全部设为同一个 Kimi 别名。Kimi Code 则在它自己的 config.toml 里声明一个明确的 OpenAI 兼容 provider 配置块，密钥直接存在文件里，因此文件需要 chmod 600。" },
        { type: "p", text: "三者之中只有 OpenCode 直接消费实时目录，这使它成为在 K3 与 Kimi for Coding 之间切换时最省心的方案——无需手工维护 provider 限制，另外两个则完全信任你固定的值。" },
        { type: "link", text: "在 Claude Code 中固定 Kimi 别名", href: "/docs/learn/kimi-api-for-claude-code" },
        { type: "link", text: "在 Kimi Code 的 config.toml 中声明 provider", href: "/docs/learn/kimi-api-for-kimi-code" },
      ] },
    ],
    faq: [
      { q: "OpenCode 支持 Kimi 模型吗？", a: "支持。apiToken.sale 路由器插件会注册实时的 Kimi 命名空间，OpenCode 以 apitoken/kimi/{model} 的形式明确选择模型——例如 apitoken/kimi/kimi-for-coding。" },
      { q: "为什么用路由器插件而不是静态模型列表？", a: "插件在每次启动时重新拉取按密钥作用域的 /v1/models 目录，模型 ID、限制和可用性始终与你的密钥实际能跑的内容保持一致。静态配置会持续提供已下线或不可用的别名，直到你手动修改。" },
      { q: "目录不可达时 OpenCode 会怎样？", a: "插件会从绑定到你的凭证和 base URL 的加密 last-good 快照中恢复能力元数据。缓存的模型会被标注 \"[stale metadata; pricing unavailable]\"，在下一次实时发现成功之前不显示成本。" },
      { q: "Kimi for Coding 在 OpenCode 里的价格是多少？", a: "官方费率为每 1M 缓存命中 token $0.19、每 1M 缓存未命中 token $0.95、每 1M 输出 token $4，apiToken.sale 在预付费余额上按半价收取。highspeed 别名的每一档恰好翻倍。" },
      { q: "哪个 Kimi 模型应该作为我的 OpenCode 默认？", a: "日常 agent 循环默认用 apitoken/kimi/kimi-for-coding；困难推理或长上下文代码库任务升级到 apitoken/kimi/k3；只有在延迟明显值回双倍费率的会话中才用 highspeed。" },
      { q: "Claude Code 也能用 Kimi 吗？", a: "可以，但配置不同。把 Claude Code 指向路由器的 Anthropic Messages 端点，并把它的 main、Opus、Sonnet、Haiku 和 subagent 模型变量固定到同一个 Kimi 别名。" },
    ],
  };
