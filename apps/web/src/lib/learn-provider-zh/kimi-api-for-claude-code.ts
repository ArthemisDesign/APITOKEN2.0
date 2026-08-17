import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Kimi API 接入 Claude Code:K3 与 Kimi for Coding",
    h1: "在 Claude Code 中运行 Kimi K3 与 Kimi for Coding",
    description: "通过 apiToken.sale 让 Claude Code 使用 Kimi K3 或 Kimi for Coding:固定每个模型层级、保留 1M 上下文窗口、验证路由并按 token 控制成本。",
    keywords: ["kimi claude code", "kimi k3 claude code", "kimi for coding claude code", "claude code 自定义模型", "claude code kimi api", "claude code anthropic_base_url", "claude code subagent 模型", "k3 1m claude code", "claude code 不用 claude 订阅", "kimi api anthropic messages 端点"],
    dek: "Claude Code 使用 Anthropic Messages 协议访问你指定的任何端点,因此 apiToken.sale 路由器上的 Kimi 订阅别名无需插件、无需补丁即可工作。可靠的配置会把 Claude Code 的每个内部模型层级都固定到 Kimi——未固定的层级会继承 Claude 模型 ID,直到对应的后台路径运行时才报错。所有用量计入同一个预付余额,价格为 Kimi 官方 token 费率的一半。",
    sections: [
      { h2: "Claude Code 原生使用 Kimi 的协议", blocks: [
        { type: "p", text: "Claude Code 会把 Anthropic Messages 请求发送到 ANTHROPIC_BASE_URL 指定的地址,而 https://router.apitoken.sale 的路由器为 Kimi 订阅别名提供该协议。不涉及插件、代理或 fork:只需修改环境变量,每个会话、层级决策和 subagent 调用都会走 Kimi 而不是 Anthropic。计费转到你的 apiToken.sale 预付余额,价格比 Kimi 官方 token 费率统一低 50%。" },
        { type: "p", text: "让这个配置静默失败的唯一原因,是 Claude Code 的内部模型映射。它为主会话、Opus/Sonnet/Haiku 层级和 subagent 分别维护模型。只设置 ANTHROPIC_MODEL 只会重定向可见的对话,而标题生成、上下文压缩、Task subagent 等后台路径仍带着继承来的 Claude ID,一旦运行就会失败。" },
        { type: "note", text: "使用 Google 或 GitHub 注册的新账户可获得 $5 平台赠金——适用于支持的 Claude、GPT、Gemini 和 Kimi 模型;邮箱/密码注册的账户不享受赠金。" },
      ] },
      { h2: "固定端点与每个模型层级", blocks: [
        sourceBlock("kimi-api-for-claude-code", 1, 0),
        { type: "p", text: "三个 ANTHROPIC_DEFAULT_* 变量覆盖 Claude Code 的层级路由,CLAUDE_CODE_SUBAGENT_MODEL 覆盖 Task subagent,两个上下文变量把窗口和自动压缩上限都提升到 K3 的 1M token。Anthropic 通道上使用裸订阅别名;按密钥授权的 GET /v1/models 目录会显示带命名空间的 kimi/* 写法,把别名写进长期环境之前先查一下。" },
        { type: "note", text: "在 k3 别名上不要省略这两个 1M 变量,在 256K 别名上也不要保留它们。它们告诉 Claude Code 压缩之前可以使用多少上下文;填一个实际模型不支持的值,会在两个方向上扭曲这个判断。" },
      ] },
      { h2: "让别名匹配会话场景", blocks: [
        { type: "table", headers: ["别名", "上下文", "官方 命中 / 未命中 / 输出", "本站五折后"], rows: [
          ["kimi-for-coding", "256K", "$0.19 / $0.95 / $4", "$0.095 / $0.475 / $2"],
          ["kimi-for-coding-highspeed", "256K", "$0.38 / $1.90 / $8", "$0.19 / $0.95 / $4"],
          ["k3-256k", "256K", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
          ["k3 · k3[1m]", "1M", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
        ] },
        { type: "p", text: "数字均为每 1M token;Kimi 的缓存是自动的,缓存命中与未命中分开计费。对于 k3-256k 或 kimi-for-coding 这类 256K 别名,层级固定保持不变,但要去掉 CLAUDE_CODE_MAX_CONTEXT_TOKENS 和 CLAUDE_CODE_AUTO_COMPACT_WINDOW。k3[1m] 是 K3 1M 模式的兼容写法——路由器会把它归一化为提供商真实的 k3 wire 模型,两种写法价格相同。" },
        { type: "p", text: "实用的分工:kimi-for-coding 作为日常编辑和测试循环的主力;k3 用于需要对整个仓库做长上下文推理的会话;kimi-for-coding-highspeed 只在延迟敏感、值得付出整整两倍基础费率时使用。" },
        { type: "link", text: "K3 与 Kimi for Coding 完整对比", href: "/docs/learn/kimi-k3-vs-kimi-for-coding" },
      ] },
      { h2: "验证路由,而不是模型的自我介绍", blocks: [
        { type: "steps", items: [
          "启动会话并运行 /status。先确认 Anthropic base URL 是 apiToken.sale,再相信会话里的其他信息。",
          "发一个简单提示词——\"只回复:connected\"。干净的回答一次往返就证明了密钥、base URL 和余额都没问题。",
          "长期固定别名前,先查按密钥授权的目录:用你的密钥 curl https://router.apitoken.sale/v1/models,列出该密钥实际可调用的模型。",
          "跑一次 Task subagent。这是最可能带着未固定层级的路径,要让失败发生在第一天,而不是重构中途。",
        ] },
        { type: "note", text: "不要把让模型自报家门当作验证手段。Claude Code 的 system prompt 可以让任何后端自称 Claude,所以自我介绍证明不了当前由哪个模型服务——/status 和请求路径才是证据。" },
      ] },
      { h2: "推理开关不是模型选择器", blocks: [
        { type: "p", text: "把模型槽位设为 none 或 off 会关闭 K3 推理;它不会切换到其他或更旧的 Kimi 模型。这些轮次无论怎么设都按 K3 费率计费。kimi-k2.6 不是路由器上可寻址的公开模型,输入它什么都选不中——请使用按密钥目录中的别名。" },
        { type: "p", text: "K3 支持 low、high、max 三档推理强度,默认为 high;Kimi for Coding 始终开启思考。推理 token 是输出的子集,按输出费率计费——不会作为独立的 token 类别再收一次,所以重推理的会话体现为输出量,而不是附加费。" },
      ] },
      { h2: "预付余额下 Kimi 会话的成本", blocks: [
        { type: "p", text: "每一轮都按上述 Kimi 官方费率逐 token 计量,统一 50% 折扣在扣费之前先行减去。没有订阅费、没有席位费:闲置一周零成本,重度重构也只按实际消耗的 token、以官方价格的一半计费。同一个余额覆盖支持的 Claude、GPT 和 Gemini 模型,所以 Kimi 上的 Claude Code 会话与你运行的其他所有负载共用同一个资金池。" },
        { type: "list", items: [
          "为密钥设置终身消费上限,并在仪表板查看已结算的用量。",
          "默认使用 kimi-for-coding,整个仓库级的会话再升级到 k3,而不是所有任务都按 K3 费率跑。",
          "kimi-for-coding-highspeed 只留给延迟敏感的循环;它的费率正好是基础档的两倍。",
          "把余额耗尽的响应当作明确的信号——充值后下一个请求就会成功;盲目重试改变不了任何结果。",
        ] },
        { type: "link", text: "各别名 Kimi 费率与缓存计费明细", href: "/docs/learn/kimi-api-pricing" },
        { type: "link", text: "所有支持模型与价格的实时目录", href: "/models" },
      ] },
    ],
    faq: [
      { q: "Claude Code 支持 Kimi K3 吗?", a: "支持。把 ANTHROPIC_BASE_URL 指向 https://router.apitoken.sale,用 apiToken.sale 密钥认证,并把每个模型层级固定到已准入的 Kimi 订阅别名——不需要插件,因为 Claude Code 原生使用 Anthropic Messages。" },
      { q: "为什么必须固定 Claude Code 的每个模型变量?", a: "Claude Code 为主会话、各层级和 subagent 分别选择模型。未固定的层级可能继承 Claude ID,只在对应后台路径运行时才失败,所以会话可能看起来正常,而压缩或 Task 调用其实已经坏了。" },
      { q: "如何在 Claude Code 中保留 K3 的完整 1M 上下文?", a: "使用 k3 或 k3[1m],并把 CLAUDE_CODE_MAX_CONTEXT_TOKENS 和 CLAUDE_CODE_AUTO_COMPACT_WINDOW 都设为 1048576。在 k3-256k 或 kimi-for-coding 等 256K 别名上,两个变量都不要设置。" },
      { q: "kimi-k2.6 在 Claude Code 中是有效的模型 ID 吗?", a: "不是。kimi-k2.6 不是路由器上可寻址的公开模型,模型槽位里的 none/off 是关闭 K3 推理,而不是选择其他模型。请使用按密钥授权的 GET /v1/models 目录返回的订阅别名。" },
      { q: "Claude Code 跑 Kimi 的会话要花多少钱?", a: "按 token 以 Kimi 官方费率计费,预付余额统一打五折——Kimi for Coding 折扣前为每 1M 缓存命中、缓存未命中和输出 token $0.19 / $0.95 / $4,High Speed 正好翻倍。" },
    ],
  };
