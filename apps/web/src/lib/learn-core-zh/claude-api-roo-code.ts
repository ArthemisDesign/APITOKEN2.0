import type { LocalizedContent } from "../learn";
import { BASE, OPENAI_BASE, KEY } from "../learn-shared";

export const content: LocalizedContent = {
    title: "在 Roo Code 中使用 Claude API",
    h1: "在 Roo Code 中使用 Claude API",
    description: "通过 apiToken.sale 把 VS Code 里的 Roo Code 接入 Claude：选择 Anthropic 提供方，勾选自定义 base URL，粘贴密钥，即可以统一 50% 折扣写代码。",
    keywords: ["claude api roo code", "roo code 配置 claude", "roo code anthropic 提供方", "roo code 自定义 base url", "roo code claude api 密钥", "roo code 按模式选模型", "roo code api 配置档案", "roo code 提示词缓存", "roo code openai 兼容", "roo code 和 cline 区别", "roo code 便宜 claude"],
    dek: "Claude API 可以通过 Roo Code 扩展原生的 Anthropic 提供方接入：勾选自定义 base URL，指向 apiToken.sale 网关，粘贴一把预付密钥即可。本文给出确切的提供方设置、在 Roo 的 Code、Architect 和 Ask 模式下分别固定模型的方法，以及你实际会遇到的四类报错。",
    sections: [
      { h2: "用一个配置档案把 Roo Code 接到网关", blocks: [
        { type: "p", text: `Roo Code 自带原生 Anthropic 提供方，并支持可选的自定义 base URL，所以接入折扣网关只需填三个字段，不需要插件或代理。提供方选 Anthropic，base URL 填 ${BASE}，粘贴你的 ${KEY} 密钥——之后每个任务都以官方 token 费率统一 50% 折扣跑在 Claude 上。` },
        { type: "steps", items: [
          "打开 Roo Code → Settings → Providers，新建一个 API 配置档案，起个类似 “apiToken” 的名字，方便日后与直连 Anthropic 的档案区分开。",
          `API Provider 选 Anthropic，勾选 “Use custom base URL”，填入 ${BASE}——就是这个地址，不要带任何尾部路径。`,
          `把你的 apiToken.sale 密钥（${KEY}）粘贴到 API Key 字段。密钥会以 x-api-key 发送，这是标准的 Anthropic Messages 请求头。`,
          "保存档案，模型选 claude-sonnet-5，先跑一个小任务——让 Roo 解释一个文件——确认链路通了，再把真正的重构交给它。",
        ] },
        { type: "code", code: `# Roo Code → Settings → Providers (profile "apiToken")\nAPI Provider : Anthropic\n[x] Use custom base URL\nBase URL     : ${BASE}\nAPI Key      : ${KEY}\nModel        : claude-sonnet-5` },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额，可用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "Anthropic 提供方还是 OpenAI 兼容——留在 Anthropic 通道", blocks: [
        { type: "p", text: `Roo Code 还提供 “OpenAI Compatible” 提供方，同一把密钥在两种协议上都能应答：${BASE} 上是 Anthropic Messages，${OPENAI_BASE} 上是带 Authorization: Bearer 的 OpenAI 形态通道。但用 Claude 时请坚持 Anthropic 提供方。Roo Code 的 Claude 专属控制——提示词缓存开关、扩展思考选项，以及它的智能体循环所依赖的工具调用管线——都是按 Messages API 的形态构建的，走通用通道这些都会丢失。` },
        { type: "code", code: `# Only for tools without an Anthropic option:\nAPI Provider : OpenAI Compatible\nBase URL     : ${OPENAI_BASE}\nAPI Key      : ${KEY}\nModel ID     : claude-sonnet-5   # typed by hand, no dropdown` },
        { type: "note", text: "在 OpenAI 兼容通道上，Roo Code 不会拉取模型列表——模型 ID 是自由文本框。这里的拼写错误会表现为 “model not found”，而不是鉴权错误，很多人因此排查错了方向。" },
      ] },
      { h2: "给每个 Roo 模式固定不同的 Claude 模型", blocks: [
        { type: "p", text: "Roo Code 的 API 配置档案不只是存密钥：你可以把档案绑定到每个模式，让模型随模式切换。这样模型选择就从全局妥协变成按模式决策——既保持智能体循环的速度，又不必为每次读文件都付 Opus 的价格。" },
        { type: "table", headers: ["Roo Code 模式", "固定模型", "原因"], rows: [
          ["Ask", "claude-haiku-4-5", "代码问答是简短的只读工作；最便宜的 Claude 通常就够。"],
          ["Code", "claude-sonnet-5", "日常编辑、跑测试、工具循环的主力——接近 Opus 的编码质量，中档价格。"],
          ["Architect", "claude-opus-4-8", "规划阶段一次错误决策的下游代价最高；把强模型花在这里。"],
          ["Debug", "claude-sonnet-5", "与 Code 共用一个档案；循环卡住时手动把难缠的 bug 升级到 claude-opus-4-8。"],
        ] },
        { type: "p", text: "所有受支持的 Claude 代际都在同一把密钥和同一份预付余额后面——Opus 4.8 和 4.7、Sonnet 5 和 4.6、Haiku 4.5——所以每个模型配一个档案，维护成本和额外充值都是零。" },
      ] },
      { h2: "智能体循环会怎样烧你的 token 账单", blocks: [
        { type: "p", text: "Roo Code 不是聊天，而是循环。一个任务会读文件、起草计划、编辑、运行结果、再复查——每一轮都是一次完整的模型调用，带着系统提示词、工具 schema 和到目前为止的全部对话。这正是按 token 折扣最值钱的负载：同样的会话便宜 50%，apiToken.sale 控制台还提供 token 级明细，让你看清哪个模式在花钱。" },
        { type: "note", text: "打开 Roo Code 的提示词缓存选项。Roo 在循环中的每次调用都会重发同一个大前缀——它的系统提示词、你的规则文件、仓库上下文——而缓存输入按更便宜的官方缓存费率计费，再减去你的折扣。长会话里，缓存通常是最省钱的一项。" },
        { type: "p", text: "跑大任务前有两种方法核对开销：用成本计算器估算 token，再到目录页确认每个模型的确切费率。" },
        { type: "link", text: "在 Claude API 成本计算器中估算一次 Roo Code 会话", href: "/tools/claude-api-cost-calculator" },
        { type: "link", text: "所有受支持 Claude 模型的单模型费率", href: "/models" },
      ] },
      { h2: "排查 Roo Code 真正会抛出的错误", blocks: [
        { type: "list", items: [
          `401 Unauthorized——密钥或 base URL 错了。重新粘贴密钥，并确认 base URL 正好是 ${BASE}；Anthropic 通道上多一个尾部斜杠或 /v1 后缀都会破坏路由。`,
          "Model not found——模型 ID 过期或打错了。用当前的 ID，例如 claude-sonnet-5、claude-opus-4-8 或 claude-haiku-4-5。",
          "429 rate limit——Roo 会突发式地发出工具调用。在提供方设置里调大 “Rate limit”（请求之间的最小秒数），而不是狂点重试。",
          "上下文窗口溢出——长会话累积的文件读取比你想象的快。按工作单元开新任务，不要把一条线程无限拉长；或者当 Roo 提议压缩上下文时让它压缩。",
        ] },
        { type: "note", text: "智能体编辑值得开启检查点（Checkpoints）：Roo 会在改动前给工作区做快照，于是一次糟糕的循环是一键回滚，而不是一场 git 考古。" },
      ] },
      { h2: "同一把密钥复用到 Cline、Cursor 和各 SDK", blocks: [
        { type: "p", text: "这把密钥并不绑定 Roo Code。一把密钥同时覆盖 Roo Code、Cline、Cursor 以及 Anthropic 和 OpenAI SDK，全部从同一份预付余额扣费——所以换个智能体试用不需要第二个账号，也不需要第二次充值。" },
        { type: "p", text: "如果你是从 Cline 过来的，概念上没有任何变化：两者都是带 Anthropic 提供方、接受自定义 base URL 的 VS Code 智能体，设置差别只在入口位置。Roo Code 多了按模式绑定档案和更细的自动批准控制；Cline 是更精简的单模式智能体。按工作流选智能体，而不是按密钥——它在两者中都能用。" },
      ] },
    ],
    faq: [
      { q: "Roo Code 支持自定义 Anthropic base URL 吗？", a: `支持。Roo Code 设置里的 Anthropic 提供方有 “Use custom base URL” 复选框；勾上它，把 URL 设为 ${BASE}，用你的 apiToken.sale 密钥鉴权即可。` },
      { q: "这把密钥能让 Roo Code 用哪些 Claude 模型？", a: "所有受支持的 Claude 模型——Opus 4.8 和 4.7、Sonnet 5 和 4.6、Haiku 4.5——共用一把密钥和一份预付余额，因此你可以给不同的 Roo Code 模式固定不同的模型。" },
      { q: "Roo Code 能按模式用不同的模型吗？", a: "能。API 配置档案可以按模式绑定，所以 Ask 跑 claude-haiku-4-5、Code 跑 claude-sonnet-5、Architect 跑 claude-opus-4-8，任务之间不用动设置。" },
      { q: "提示词缓存经过网关还能用吗？", a: "能。保持 Roo Code 的提示词缓存选项开启；缓存输入按更便宜的官方缓存费率计费，再减去你 50% 的统一折扣，在长的智能体循环上会持续叠加收益。" },
      { q: "Roo Code 的设置和 Cline 有区别吗？", a: "几乎没有。两者都是 Anthropic 提供方接受自定义 base URL 的 VS Code 智能体，同一把密钥和同一个 URL 在任一个里都能用；挑你更喜欢的工作流即可。" },
    ],
  };
