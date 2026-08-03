// Simplified-Chinese (zh-CN) translations for the tool-error cluster.
// Contract: only explanatory prose is translated. Verbatim tool output,
// tool names, env vars, codes, URLs and config keys stay in English —
// people paste exactly what their terminal printed.

import type { ToolErrorTranslations } from "./tool-errors";

export const toolErrorsZh: ToolErrorTranslations = {
  ui: {
    eyebrow: "疑难排查",
    indexTitle: "AI 编码工具报错大全——Claude Code、Cursor、Codex、opencode、Cline、Zed",
    indexDescription:
      "覆盖开发者实际用来调用 Claude API 的工具的错误页面：Claude Code、Cursor、Codex CLI、opencode、Cline 和 Zed。每个故障都有报错原文、原因和解决方法。",
    indexIntro:
      "选择出问题的工具。每个页面都会逐字引用工具打印的报错文本，解释它是如何产生的，并给出恢复正常配置的最短路径。若需要按状态码组织的 API 层面参考，请查看 Claude API 错误码页面。",
    errorsIn: "{tool} 错误",
    whatYouSee: "你看到的报错",
    why: "为什么会发生",
    how: "如何解决",
    faqHeading: "常见问题",
    alsoSearched: "相关搜索",
    colError: "错误",
    colMeaning: "含义",
    backToTool: "全部 {tool} 错误",
    allTools: "全部工具",
    fullReference: "Claude API 错误码",
    fullReferenceBlurb:
      "API 层面的参考：每个状态码及其逐字响应体，按错误码而非工具组织。",
    setupGuide: "配置指南",
    ctaHeading: "跳过折腾坏掉的配置",
    ctaBody:
      "apiToken.sale 提供标准的 Anthropic API——同样的模型、同样的 SDK、一个预付余额。把你工具的 base URL 指向它，本页的配置即可原样生效。",
    ctaButton: "获取密钥",
    ctaDocs: "API 文档",
  },
  index: {
    title: "AI 编码工具报错大全——Claude Code、Cursor、Codex、opencode、Cline、Zed",
    description:
      "覆盖开发者实际用来调用 Claude API 的工具的错误页面：Claude Code、Cursor、Codex CLI、opencode、Cline 和 Zed。每个故障都有报错原文、原因和解决方法。",
    intro:
      "选择出问题的工具。每个页面都会逐字引用工具打印的报错文本，解释它是如何产生的，并给出恢复正常配置的最短路径。若需要按状态码组织的 API 层面参考，请查看 Claude API 错误码页面。",
  },
  tools: {
    "claude-code": {
      title: "Claude Code API 报错——429、401、529、Usage Limit",
      description:
        "Claude Code 会打印的每种错误的解决方法：API Error 429 rate_limit_error、401 invalid x-api-key、529 Overloaded 以及用量上限提示。报错原文、原因与修复。",
      intro:
        "Claude Code 把 API 故障显示为以 \"API Error:\" 开头、后跟原始响应体的一行文本，把订阅上限显示为一句普通提示。下面的页面逐一覆盖每种形式：报错原文、产生原因以及解决办法。",
    },
    cursor: {
      title: "Cursor 模型提供商报错——401、429、Unable to Reach",
      description:
        "使用自定义 Anthropic API 密钥时 Cursor 的错误解决方法：unable to reach model provider、验证时提示 invalid API key、提供商返回的 401 以及速率限制。每个错误都有原因和修复。",
      intro:
        "使用自定义 Anthropic 密钥时，Cursor 会直接与提供商通信，因此大多数故障要么出在密钥/base URL 配置上，要么是提供商自身的响应被原样透传。下面的页面会区分这两类。",
    },
    codex: {
      title: "Codex CLI 报错——Missing OPENAI_API_KEY、config.toml、stream error",
      description:
        "使用自定义模型提供商时 Codex CLI 的故障解决方法：Missing OPENAI_API_KEY、config.toml 配置错误、auth.json 登录状态以及 stream error: unexpected status 401。",
      intro:
        "Codex CLI 先读取 ~/.codex/config.toml，解析出一个模型提供商，然后通过 Responses API 进行流式请求。每个阶段的失败方式各不相同：缺失的环境变量、格式错误的 profile、失效的登录状态，或流式过程中的 HTTP 错误。下面的页面按这个顺序逐一讲解。",
    },
    opencode: {
      title: "opencode 报错——AI_APICallError、Model Not Found、认证",
      description:
        "使用自定义提供商时 opencode 的故障解决方法：AI_APICallError、模型未出现在提供商列表中、API 密钥未被读取，以及被悄悄丢弃的图片附件。",
      intro:
        "opencode 基于 Vercel AI SDK 构建，因此提供商故障以 AI_* 错误类的形式出现；不在 models.dev 目录里的模型需要在 opencode.json 中手动声明其能力。下面的页面覆盖真正会影响用户的那些故障。",
    },
    cline: {
      title: "Cline API 报错——API Request Failed、401、429、上下文超限",
      description:
        "使用 Anthropic 密钥时 Cline 的错误解决方法：API Request Failed 横幅、401 invalid x-api-key、429 rate_limit_error 重试循环以及上下文超限 400。每个错误都有原因和修复。",
      intro:
        "Cline 把大多数故障显示为 \"API Request Failed\" 横幅，下方附带提供商的响应。横幅本身并不是诊断结论——响应体里的状态码和 error.type 才是。下面的页面把每种响应体映射到对应的修复方法。",
    },
    zed: {
      title: "Zed Claude 报错——401、429、529、自定义 api_url",
      description:
        "Zed 助手使用 Anthropic 模型时的错误解决方法：401 invalid x-api-key、429 速率限制、529 Overloaded 以及自定义 api_url 配置，包括 /v1/v1 404 陷阱。",
      intro:
        "Zed 的助手几乎原样透传 Anthropic 的响应，因此你看到的报错文本就是 API 自己的。Zed 特有的部分只是密钥和自定义 api_url 在 settings.json 中的位置——大多数持续性故障最终都能追溯到这两个字段。",
    },
  },
  entries: {
    // ——— Claude Code ———
    "claude-code/api-error-429": {
      title: "Claude Code API Error 429（rate_limit_error）报错原因与解决方法",
      description:
        "当每分钟吞吐量耗尽时，Claude Code 会打印 API Error: 429 rate_limit_error。这个限制到底是什么、为什么并行智能体会触发它，以及如何解决。",
      causes: [
        "你的密钥背后 API 组织的每分钟 token 或请求上限被超出。消息中的数字是该组织自己的限额，因此不同账户各不相同。",
        "多个 Claude Code 会话或子智能体并行使用同一把密钥——每个会话每轮都会重发完整上下文，突发流量累积得比表面上看起来快得多。",
        "单个超大上下文本身就可能超出每分钟 token 预算，这也是消息同时建议缩短提示词而不只是等待的原因。",
        "重试叠加在引发第一个 429 的突发流量之上，不但没有排空窗口，反而把它撑得更满。",
      ],
      fixes: [
        "等过这一分钟窗口——Claude Code 会自动重试并遵守 Retry-After。如果持续复发，就减少共享同一把密钥的会话数量。",
        "缩减每轮携带的内容：用 /compact 压缩过大的对话，或者开一个新会话，而不是拖着冗长的历史继续。",
        "不要把它和 \"Claude usage limit reached\" 混淆——后者是带重置时间的订阅上限，不是每分钟吞吐量。两者的解决方法不同。",
        "如果你持续需要的吞吐量超过密钥所属组织的允许值，那是容量问题而不是重试问题——去找你的提供商谈。",
      ],
      faq: [
        {
          q: "Claude Code 里的 API Error 429 意味着我的订阅用完了吗？",
          a: "不是。429 是 API 密钥的每分钟吞吐量限制。订阅用完会显示为带重置时间的 \"Claude usage limit reached\"。",
        },
        {
          q: "Claude Code 会自己重试 429 吗？",
          a: "会——它会自动退避并重试。如果错误持续，说明窗口被填充的速度快于排空的速度，通常是同一把密钥上的并行会话造成的。",
        },
        {
          q: "为什么一个巨大的提示词自己就能引发 429？",
          a: "速率限制按每分钟 token 计数，一个超大的上下文可能在单次请求中就花光整分钟的预算。",
        },
      ],
    },
    "claude-code/api-error-401": {
      title: "Claude Code API Error 401 invalid x-api-key 解决方法",
      description:
        "当密钥没有到达它应该访问的端点时，Claude Code 会打印 API Error: 401 invalid x-api-key。引发它的环境变量规则以及修复方法。",
      causes: [
        "ANTHROPIC_API_KEY 和 ANTHROPIC_AUTH_TOKEN 同时被设置——两个请求头一起发出，请求被拒绝。空字符串也算已设置。涉及自定义 base URL 时这是最常见的原因。",
        "变量设置在与启动 claude 的 shell 不同的 shell 里——一个终端里的 export 在另一个终端里不存在，图形界面启动器也不会读取你的 shell 配置文件。",
        "ANTHROPIC_BASE_URL 指向一个提供商，而密钥却由另一个签发。有效的密钥发到错误的端点仍然是 401。",
        "密钥已被吊销，或者带过期日期的密钥已经过期。",
      ],
      fixes: [
        "只保留一个变量并 unset 另一个：对于本网关，使用 ANTHROPIC_AUTH_TOKEN 加 ANTHROPIC_BASE_URL，并确保 ANTHROPIC_API_KEY 没有同时导出。",
        "在真正运行 claude 的那个 shell 里验证：启动前打印变量的前几个字符确认它存在。",
        "在控制台确认密钥处于激活状态，且 base URL 与密钥的签发方一致。",
      ],
      snippetLabel: "本网关的可用环境配置",
      faq: [
        {
          q: "为什么我刚设置了自定义 ANTHROPIC_BASE_URL，Claude Code 就返回 401？",
          a: "通常是 ANTHROPIC_API_KEY 和 ANTHROPIC_AUTH_TOKEN 同时被设置，或者密钥属于与 base URL 指向不同的端点。unset 其中一个变量，并让密钥与端点匹配。",
        },
        {
          q: "该用 ANTHROPIC_API_KEY 还是 ANTHROPIC_AUTH_TOKEN？",
          a: "ANTHROPIC_API_KEY 会变成 x-api-key 请求头；ANTHROPIC_AUTH_TOKEN 会变成 Authorization: Bearer。二者只用其一。对于本网关，文档推荐 ANTHROPIC_AUTH_TOKEN。",
        },
        {
          q: "同一把密钥在 curl 里能用，在 Claude Code 里却不行——为什么？",
          a: "运行 claude 的那个 shell 环境状态不同：存在冲突变量、值已过期，或压根没有值。请在那个确切的 shell 里检查环境。",
        },
      ],
    },
    "claude-code/api-error-529": {
      title: "Claude Code API Error 529 Overloaded 是什么意思",
      description:
        "当上游容量饱和时，Claude Code 会打印 API Error: 529 Overloaded。为什么这不是你的请求的问题、为什么它会集中出现，以及什么才真正有用。",
      causes: [
        "上游容量暂时饱和。529 描述的是那一刻的服务状态，而不是你的请求——你的负载里没有任何东西引发了它。",
        "它在故障期和高峰时段集中出现：同样的请求通常几分钟后就能成功，你这边无需任何改动。",
      ],
      fixes: [
        "让 Claude Code 自己重试——它会自动退避。如果一次运行反复失败，等几分钟，而不是在同一分钟里反复轰击。",
        "对于长时间无人值守的运行，机械性步骤优先用较小的模型：它们通常竞争更少，运行更能扛过容量波动。",
        "如果 529 长时间持续，先查看提供商的状态页面，再考虑改动你的配置——你的配置几乎从来不是原因。",
      ],
      faq: [
        {
          q: "529 和 429 有什么区别？",
          a: "有。429 是你自己的吞吐量上限；529 是上游容量问题。退避对两者都有帮助，但只有 429 会因为你减少用量而缓解。",
        },
        {
          q: "是我的提示词导致了 529 吗？",
          a: "不是。Overloaded 是服务端状况。完全相同的请求通常在容量恢复后就能成功。",
        },
        {
          q: "看到 529 时应该换模型吗？",
          a: "对延迟敏感的工作，临时改用较小的模型有帮助，因为它竞争更少。其他情况下，等待就足够了。",
        },
      ],
    },
    "claude-code/usage-limit-reached": {
      title: "Claude Code 提示 Claude usage limit reached——重置时间与应对方案",
      description:
        "订阅套餐下 Claude Code 会显示 \"Claude usage limit reached. Your limit will reset at…\"。这个上限到底统计什么、何时重置，以及按量付费的 API 访问有何不同。",
      causes: [
        "这是 Claude Pro 或 Max 的订阅上限，不是 HTTP 错误。它在 Claude Code 以订阅而非 API 密钥登录时出现。",
        "上限按滚动窗口执行——通常是五小时的会话窗口加每周总额——因此高强度使用可能在一周结束前很早就耗尽每周额度。",
        "长会话每轮都会重发全部历史，所以一个繁忙的对话消耗额度的速度远超其可见输出所显示的程度。",
      ],
      fixes: [
        "等到提示中说明的重置时间——消息里写明了时间，且它是滚动窗口而不是日历边界。",
        "精简每轮携带的内容：用 /compact 压缩长对话，不要拖着一个巨型会话去做不相关的任务。",
        "如果工作等不到重置，按量付费的 API 访问按 token 计费而不是按套餐额度，所以没有可耗尽的会话或每周上限。区别就是这么直白：计费方式不同，而不是绕过限制。",
      ],
      snippetLabel: "把 Claude Code 从订阅切换到 API 余额",
      faq: [
        {
          q: "Claude 用量上限什么时候重置？",
          a: "在消息本身指出的时间。会话窗口按滚动方式重置（通常五小时）；每周总额在填满它的使用发生一周后重置。",
        },
        {
          q: "这和 API Error 429 是一回事吗？",
          a: "不是。用量上限是订阅额度；429 是 API 的每分钟吞吐量。它们来自不同的系统，解决方法也不同。",
        },
        {
          q: "API 访问有同样的每周限制吗？",
          a: "没有。API 用量按 token 从余额中计量扣费，有每分钟速率限制，但没有会话或每周额度。",
        },
      ],
    },

    // ——— Cursor ———
    "cursor/unable-to-reach-model-provider": {
      title: "Cursor Unable to Reach the Model Provider 报错原因与解决方法",
      description:
        "当请求在收到任何 HTTP 响应之前就中断时，Cursor 会报告无法连接模型提供商：base URL 写错、网络路径受阻，或提供商故障。如何判断是哪一种。",
      causes: [
        "自定义 base URL 在传输层就是错的——主机名打错、协议写错，或 URL 根本解析不到任何地方——所以永远收不到 HTTP 响应。",
        "网络路径被阻断：Cursor 与端点之间的代理、VPN 或防火墙丢弃了连接。",
        "提供商本身短暂宕机。这种情况下你这边什么都没变，错误会自行消失。",
        "粘贴的覆盖 URL 带了提供商不提供服务的尾部路径段，导致 TLS 或 HTTP 层在任何 API 错误返回之前就已失败。",
      ],
      fixes: [
        "在 Cursor 之外用 curl 测试完全相同的 base URL。如果 curl 也连不上，问题在 URL 或网络，而不是 Cursor。",
        "覆盖 URL 只填源站（origin），让客户端自己拼接 /v1 路径。",
        "如果 curl 正常而 Cursor 不行，检查 Cursor 是否走了终端没有使用的代理或 VPN。",
        "如果你这边什么都没改而错误是新出现的，等几分钟——提供商的瞬时故障产生的正是这条消息。",
      ],
      snippetLabel: "证明端点是可达的",
      faq: [
        {
          q: "\"unable to reach model provider\" 和 401 或 429 是一回事吗？",
          a: "不是。这条消息意味着根本没有收到任何 HTTP 响应。401 或 429 意味着提供商已应答并拒绝了请求——不同的层面，不同的修复方法。",
        },
        {
          q: "Cursor 昨天还能用，今天什么都没改就失败了——怎么办？",
          a: "这种模式是瞬时故障或网络路径变化（VPN、代理、强制门户）。先用 curl 验证；不要去重写一个原本就能用的配置。",
        },
        {
          q: "Anthropic 覆盖项应该填什么 base URL？",
          a: "只填源站——对于本网关是 https://router.apitoken.sale。不要追加 /v1：客户端会自己添加 API 路径，路径重复会导致失败。",
        },
      ],
    },
    "cursor/invalid-api-key": {
      title: "Cursor Invalid API Key（Anthropic）——验证失败的原因与解决方法",
      description:
        "当密钥与 base URL 不属于同一签发方，或覆盖字段仍指向默认端点时，Cursor 会在验证阶段拒绝 Anthropic 密钥。用这份清单修复它。",
      causes: [
        "密钥属于自定义端点，但 \"Override Anthropic Base URL\" 关闭或为空，于是 Cursor 拿默认的 api.anthropic.com 去验证密钥——那里从未见过它。",
        "base URL 已设置，但密钥粘贴时带了空白字符、缺少前缀，或者根本是另一家提供商的密钥格式。",
        "密钥已被吊销或已过期。",
        "覆盖 URL 包含路径后缀，导致验证请求发往端点不提供服务的 URL。",
      ],
      fixes: [
        "先启用 base URL 覆盖并设置为签发密钥的源站，再粘贴密钥本身——验证会针对当时生效的任何 URL 进行。",
        "粘贴密钥时不要带任何前后空白，并核对前缀与控制台显示的一致。",
        "在签发方的控制台确认密钥处于激活状态，然后在 Cursor 里重新验证。",
      ],
      snippetLabel: "针对本网关验证通过的 Cursor 设置",
      faq: [
        {
          q: "为什么 Cursor 说我的密钥无效，而它在 curl 里能用？",
          a: "Cursor 针对当时配置的 base URL 进行验证。如果覆盖项是关闭的，验证会打到 api.anthropic.com，它会拒绝网关密钥。先设置覆盖项，再验证。",
        },
        {
          q: "在 Cursor 里用自定义的 Anthropic 兼容密钥需要 Anthropic 账户吗？",
          a: "不需要。Anthropic 提供商字段接受任何 Anthropic 兼容端点：把覆盖 URL 设为签发方，并粘贴该签发方的密钥。",
        },
        {
          q: "密钥验证通过后哪些模型可用？",
          a: "覆盖 URL 背后端点所提供的那些。用同样的 base URL 和密钥请求 GET /v1/models 即可列出。",
        },
      ],
    },
    "cursor/provider-returned-401": {
      title: "Cursor Request failed with status code 401——Anthropic 密钥修复",
      description:
        "Cursor 聊天中出现 401 意味着提供商已应答并拒绝了凭据：密钥与 base URL 不匹配、密钥被吊销，或错误的请求头到达了端点。修复路径如下。",
      causes: [
        "密钥曾验证通过但后来被吊销或过期——Cursor 会一直使用它，直到提供商开始返回 401。",
        "保存密钥之后 base URL 覆盖项被改动，于是保存的密钥现在发往一个从未见过它的端点。",
        "密钥背后的账户被停用或访问权限被收回。",
      ],
      fixes: [
        "按当前实际状态重新核对这一对配置：覆盖 URL 和密钥必须来自同一签发方。哪边变了就修哪边。",
        "用 curl 拿完全相同的密钥测试完全相同的 base URL——一步就把 Cursor 排除出等式。",
        "如果 curl 也返回 401，说明密钥本身已失效：去签发方控制台查看状态或重新签发。",
      ],
      snippetLabel: "在 Cursor 之外复现",
      faq: [
        {
          q: "Cursor 之前验证过我的密钥——为什么现在 401？",
          a: "验证只是一个快照。之后被吊销的密钥，或之后被改动的 base URL，会让后续每个请求都 401，即使最初的验证通过了。",
        },
        {
          q: "这是 Cursor 的 bug 吗？",
          a: "几乎从来不是。401 是提供商的应答被透传。用 curl 复现：如果 curl 也拿到 401，问题出在凭据上。",
        },
        {
          q: "如果 curl 成功但 Cursor 仍然 401 呢？",
          a: "那说明 Cursor 发送的凭据和你以为的不一样——重新打开设置，在覆盖 URL 已启用的状态下重新粘贴密钥。",
        },
      ],
    },
    "cursor/rate-limit-exceeded": {
      title: "Cursor 使用自有 Anthropic 密钥遇到 Rate Limit / 429 的解决方法",
      description:
        "使用自定义 Anthropic 密钥时，Cursor 里的 429 是密钥自身的每分钟上限，而不是 Cursor 的套餐限制。为什么长对话会触发它，以及如何终止循环。",
      causes: [
        "你的密钥所属组织的每分钟 token 上限被超出。使用自定义密钥时，Cursor 自己的套餐限制完全不参与——这是你的密钥的预算。",
        "长对话每条消息都会重发整个会话加附带上下文，一个繁忙的标签页就能独自花光一分钟的 token 预算。",
        "同一把密钥与其他工具或同事共享，他们的流量计入同一个窗口。",
        "第一个 429 之后的连珠炮式重试让窗口一直处于饱和。",
      ],
      fixes: [
        "新任务开新对话，不要在一个巨型对话里继续延伸——每条消息重发的上下文才是吃掉预算的元凶。",
        "如果当前密钥是共享的，给 Cursor 一把专用密钥，让一个工具的突发流量不会饿死另一个。",
        "重试前等过这一分钟窗口；密集的手动重试只会延长这种状况。",
      ],
      faq: [
        {
          q: "这是 Cursor 的 fast-requests 限制吗？",
          a: "不是。使用自有密钥时，请求完全绕过 Cursor 的套餐计量。429 来自你密钥背后的 API 组织。",
        },
        {
          q: "为什么长对话更容易撞上 429？",
          a: "每条消息都会重发全部历史和附件。可见的回复很小，但被计数的输入 token 随对话增长。",
        },
        {
          q: "升级 Cursor 套餐有用吗？",
          a: "对这个错误没用。上限属于 API 密钥。减少每分钟用量、停止共享密钥，或与签发方商定更高的吞吐量。",
        },
      ],
    },

    // ——— Codex CLI ———
    "codex/missing-openai-api-key": {
      title: "Codex CLI Missing OPENAI_API_KEY——自定义提供商配置方法",
      description:
        "当提供商期望的环境变量未设置时，Codex 拒绝启动。config.toml 中 env_key 的工作方式、export 为什么在 shell 之间会消失，以及一份可用的 profile。",
      causes: [
        "提供商 env_key 所指的环境变量在运行 codex 的 shell 里没有设置。使用自定义提供商时变量可以是任何名字——报错会指出缺失的是哪一个。",
        "密钥在另一个终端里导出，或加进了这个 shell 从未 source 过的配置文件。",
        "变量已设置但为空，这同样算缺失。",
        "实际使用的 profile 指向的提供商和你以为的不同，于是 Codex 寻找的变量与你导出的不是同一个。",
      ],
      fixes: [
        "在 profile 中声明提供商，并在同一个 shell 里、运行 codex 之前，导出其 env_key 指名的那个变量。",
        "启动前打印该变量确认它还在：export 只在当前 shell 有效，不是全局的。",
        "运行 codex 时显式指定 profile，这样对哪个提供商——以及哪个 env_key——生效不再有歧义。",
      ],
      snippetLabel: "本网关的可用 profile",
      faq: [
        {
          q: "运行 Codex CLI 需要 OpenAI 账户的密钥吗？",
          a: "不需要。使用自定义模型提供商时，env_key 可以指定你选的任何变量名，密钥来自那个提供商——OPENAI_API_KEY 本身只是默认提供商的变量。",
        },
        {
          q: "我导出了密钥，但 Codex 仍然说它缺失——为什么？",
          a: "export 只在当前 shell 有效。如果 codex 在另一个终端、复用器面板或 IDE 任务里运行，那个环境从未见过你的 export。请在 codex 实际运行的地方设置它。",
        },
        {
          q: "密钥应该放在 TOML 里还是 shell 里？",
          a: "shell 里。config.toml 通过 env_key 只存储变量的名字，这样密钥永远不会躺在配置文件里。",
        },
      ],
    },
    "codex/config-toml-error": {
      title: "Codex config.toml 报错——可用的 model_providers 配置",
      description:
        "~/.codex/config.toml 格式错误时 Codex 无法加载：未知的提供商名、错误的 wire_api、缺失的配置节。自定义提供商所需的确切结构。",
      causes: [
        "model_provider 指定的提供商没有对应的 [model_providers.<name>] 配置节——引用和配置节必须使用同一个标识符。",
        "TOML 语法错误：未闭合的字符串、节标题打错，或从 JSON 粘贴过来的带逗号和花括号的值。",
        "wire_api 与端点实际提供的协议不匹配，请求按错误的协议成形。",
        "编辑的文件不是 Codex 读取的文件——通过 --profile 传入的 profile 文件有特定的预期位置和文件名。",
      ],
      fixes: [
        "让提供商标识符在两处保持一致：model_provider = \"apitoken\" 必须对应 [model_providers.apitoken]。",
        "面向 Responses API 端点使用 wire_api = \"responses\"——本网关的 OpenAI 兼容层同时提供 Responses 和 Chat Completions。",
        "校验文件是合法的 TOML：字符串加引号、每行一个 key = value、节标题用方括号。",
        "运行 codex --profile <name> 并观察它报告加载的是哪个文件；改那个文件，别改长得像的。",
      ],
      snippetLabel: "最小可用 profile",
      faq: [
        {
          q: "为什么 Codex 说我的模型提供商是未知的？",
          a: "model_provider 的值必须与同一文件中的某个 [model_providers.<name>] 配置节完全一致。任何一边打错字都会让查找失败。",
        },
        {
          q: "wire_api 应该填 responses 还是 chat？",
          a: "端点提供什么就填什么。本网关的 OpenAI 兼容层接受 Responses API——wire_api = \"responses\"——也接受 Chat Completions。",
        },
        {
          q: "base_url 需要带 /v1 吗？",
          a: "针对本网关的 Codex profile，需要：文档给出的值是 https://router.apitoken.sale/v1，与文档完全一致。",
        },
      ],
    },
    "codex/auth-json-error": {
      title: "Codex auth.json / 登录错误——改用 API 密钥认证",
      description:
        "Codex 的登录状态存放在 ~/.codex/auth.json，文件失效或缺失会导致登录提示和认证失败。何时需要重新登录，何时自定义提供商完全绕过它。",
      causes: [
        "auth.json 缺失、不可读或已失效，导致默认提供商没有可用的登录状态。",
        "登录属于与会话预期不同的账户或套餐状态——刷新令牌不再成功。",
        "故障被归因错了：使用自定义提供商加 env_key 时，ChatGPT 登录状态毫不相关，真正的问题出在环境变量或 profile 上。",
      ],
      fixes: [
        "对于默认提供商，重新走一遍登录流程，让 auth.json 被重新写入。",
        "对于自定义提供商，auth.json 不参与认证：认证靠的是 env_key 指定的变量。检查 profile 是否真的被选中，以及变量是否在运行中的 shell 里已设置。",
        "把两条路径在脑中分开——默认提供商走订阅登录，自定义提供商走 API 密钥。一条路径的错误不能在另一条上修。",
      ],
      faq: [
        {
          q: "Codex 的自定义提供商使用 auth.json 吗？",
          a: "不使用。[model_providers.*] 条目通过 env_key 指名的环境变量进行认证。auth.json 只承载默认的 ChatGPT 登录。",
        },
        {
          q: "为什么 Codex 一直让我登录？",
          a: "它存储的登录状态无法刷新。重新登录以重写它——或者，如果你本意是用自定义提供商，就显式选择那个 profile，这样根本不需要登录。",
        },
        {
          q: "没有任何 ChatGPT 订阅能运行 Codex 吗？",
          a: "能——配好自定义提供商 profile 并把该提供商的 API 密钥放进环境变量，Codex 就完全以 API 密钥认证运行。",
        },
      ],
    },
    "codex/stream-error": {
      title: "Codex stream error: unexpected status 401/404 解决方法",
      description:
        "Codex 里流式过程中的 HTTP 故障会打印为 stream error: unexpected status。401、404 和 429 在这里分别意味着什么，以及如何用 curl 复现来定位坏掉的那一半。",
      causes: [
        "401——env_key 变量未设置、为空，或它的密钥不属于 profile 里的 base_url。",
        "404——base_url 与线上协议不匹配：缺 /v1、重复成 /v1/v1，或主机根本不提供 Responses API。",
        "429——密钥的每分钟吞吐量耗尽；流在开始之前就被拒绝。",
        "代理或网络设备在响应中途切断 SSE 流，会产生没有状态码的断连变体。",
      ],
      fixes: [
        "用 curl 对 profile 里的确切 base_url 加 /responses 复现，发送同一把密钥——状态码会告诉你坏的是哪一半。",
        "401 就修密钥/端点配对；404 就把 base_url 改成文档给出的值；429 就等过窗口并减少并行运行。",
        "对于反复出现、没有状态码的流中断连，关掉 VPN 或代理再测——SSE 是干扰性中间设备的第一个牺牲品。",
      ],
      snippetLabel: "在 Codex 之外复现",
      faq: [
        {
          q: "Codex 里的 stream error: unexpected status 401 是什么意思？",
          a: "端点在流开始时就拒绝了凭据。检查 profile 的 env_key 变量是否在运行中的 shell 里已设置，以及它的密钥是否属于 profile 的 base_url。",
        },
        {
          q: "主机明明是对的，为什么 404？",
          a: "路径错误：针对本网关，base_url 必须包含 /v1，且不能重复。Responses 路径由 Codex 自己追加。",
        },
        {
          q: "流在半路死掉且没有状态码——是 API 的问题吗？",
          a: "通常是网络路径的问题：缓冲或截断 SSE 的代理和 VPN。先在干净的连接上用 curl 复现，再怪端点。",
        },
      ],
    },

    // ——— opencode ———
    "opencode/ai-apicallerror": {
      title: "opencode AI_APICallError 报错原因与解决方法",
      description:
        "opencode 把提供商故障以 Vercel AI SDK 的 AI_APICallError 形式呈现。如何读取被包裹的状态码，并修复其下的 baseURL、密钥或模型。",
      causes: [
        "AI_APICallError 是包装器而不是诊断结论：AI SDK 对任何非成功的 HTTP 响应都抛它，真正的原因在被包裹的状态码和响应体里。",
        "内层是 401——提供商的 apiKey 选项写错了，或者 {env:...} 占位符指向的变量未设置。",
        "内层是 404——baseURL 与提供商协议不匹配：缺 /v1、重复成 /v1/v1，或端点不提供这个模型 id。",
        "内层是 429 或 529——吞吐量或上游容量问题；你的配置没有任何错误。",
      ],
      fixes: [
        "先读错误的 statusCode 和 responseBody 字段——它们携带了提供商的真实应答。",
        "核对 provider 配置块：baseURL 分毫不差，apiKey 从一个在启动 opencode 的 shell 里真实存在的环境变量解析。",
        "任何 opencode.json 改动之后重启 opencode——配置在启动时读取。",
      ],
      snippetLabel: "本网关的 provider 配置块",
      faq: [
        {
          q: "AI_APICallError 是 opencode 的 bug 吗？",
          a: "不是——它是 AI SDK 在报告提供商返回了非成功响应。被包裹的状态码才标识真正的问题。",
        },
        {
          q: "opencode 从哪里读取我的 API 密钥？",
          a: "从 provider 的 options.apiKey。使用 {env:NAME} 占位符时，该变量必须存在于启动 opencode 的那个环境里。",
        },
        {
          q: "我修好了 opencode.json 却什么都没变——为什么？",
          a: "opencode 在启动时读取配置。每次改动后都要重启它。",
        },
      ],
    },
    "opencode/model-not-found": {
      title: "opencode Model Not Found——声明自定义提供商模型的方法",
      description:
        "opencode 只提供它认识的模型，而自定义提供商的模型不在 models.dev 目录里。如何在 opencode.json 中声明模型，让它出现并可用。",
      causes: [
        "模型没有在 provider 的 models 映射中声明，而自定义提供商的模型不在 opencode 查询的 models.dev 目录里——所以这个模型压根不会被提供。",
        "声明的模型 id 与端点实际提供的不一致——打错字或已下线的 id 会让提供商返回 404。",
        "模型已加进 opencode.json 但 opencode 没有重启，旧配置仍然生效。",
      ],
      fixes: [
        "在 opencode.json 的 provider models 映射中显式声明每个模型，id 与端点提供的完全一致。",
        "用 GET /v1/models 列出端点的在线模型，而不是靠猜 id。",
        "改完之后重启 opencode。",
      ],
      snippetLabel: "显式声明模型",
      faq: [
        {
          q: "为什么我自定义提供商的模型不出现在 opencode 里？",
          a: "不在 models.dev 目录里的模型必须在 provider 的 models 映射中手动声明。未声明的模型完全不会被提供。",
        },
        {
          q: "如何找到要声明的确切模型 id？",
          a: "用你的密钥请求提供商的 GET /v1/models，逐字复制 id。",
        },
        {
          q: "模型声明了但请求 404——还能是什么？",
          a: "opencode.json 里的 id 必须与端点的 id 逐字符一致，baseURL 也必须是文档给出的那个。两者都检查，然后重启 opencode。",
        },
      ],
    },
    "opencode/auth-config-error": {
      title: "opencode API 密钥未生效——auth 与 {env} 占位符解决方法",
      description:
        "opencode 通过 options.apiKey（通常是 {env:...} 占位符）认证自定义提供商。为什么变量会悄悄解析为空、请求以 401 失败。",
      causes: [
        "{env:NAME} 占位符指向的变量在启动 opencode 的环境里没有设置——它解析为空，提供商收不到任何密钥。",
        "变量在一个 shell 里导出，但 opencode 从另一个地方启动：桌面启动器、另一个终端、复用器面板。",
        "密钥被直接粘贴进 opencode.json 且带了前后空白，或者它属于与 baseURL 不同的端点。",
      ],
      fixes: [
        "在同一个 shell 里导出占位符指名的那个变量，然后从该 shell 启动 opencode。",
        "用 curl 验证这一对配置：配置里的 baseURL 加变量里的密钥，必须先在 opencode 之外成功。",
        "优先用 {env:...} 形式而不是把密钥粘进文件——它让密钥远离 dotfiles 和版本控制。",
      ],
      snippetLabel: "密钥走环境变量，不进文件",
      faq: [
        {
          q: "opencode.json 里明明写了密钥，为什么 opencode 不发送任何 API 密钥？",
          a: "{env:NAME} 占位符在启动时从 opencode 自己的环境解析。如果那个 shell 从未导出该变量，密钥就是空的。",
        },
        {
          q: "把密钥直接粘贴进 opencode.json 安全吗？",
          a: "能用，但 env 占位符是更好的习惯：配置文件会进入备份和代码仓库，粘贴的密钥会跟着一起走。",
        },
        {
          q: "如何判断坏的是密钥还是 URL？",
          a: "手动用 curl 拿密钥请求 baseURL。401 说明是密钥/端点配对的问题；连接错误说明是 URL 或网络。",
        },
      ],
    },
    "opencode/image-input-not-supported": {
      title: "opencode 提示 This Model Does Not Support Image Input 的解决方法",
      description:
        "当模型的 modalities 没有声明时，opencode 会悄悄把附加的图片替换成一条备注。一行 opencode.json 声明即可开启图片输入。",
      causes: [
        "自定义提供商的模型不在 models.dev 目录里，opencode 会给它们分配纯文本的默认能力——无论模型实际能做什么。",
        "在纯文本能力下，opencode 会把附加的图片替换成一条内嵌的 \"this model does not support image input\" 备注；提供商根本收不到图片。",
        "modalities 声明了，但之后没有重启 opencode。",
      ],
      fixes: [
        "在 opencode.json 中为该模型显式声明图片模态，然后重启 opencode。",
        "一旦 \"image\" 进入 modalities.input，粘贴和附加的图片就会以标准 Chat Completions 图片分块发出，本网关接受这种格式。",
      ],
      snippetLabel: "为模型声明图片输入",
      faq: [
        {
          q: "为什么模型的回答像是从没看到我的图片？",
          a: "它确实没看到。没有声明图片模态时，opencode 会剥离附件并用一条文本备注代替——到达提供商的请求是纯文本的。",
        },
        {
          q: "这是提供商的限制吗？",
          a: "不是——这是客户端侧的能力门控。同一个模型在 opencode.json 里声明模态后立刻就能接收图片。",
        },
        {
          q: "其他工具也需要这个声明吗？",
          a: "只有带能力门控的客户端需要。线上契约就是普通的 Chat Completions 图片分块，所以像 OpenAI SDK 这类无门控的客户端什么都不用加。",
        },
      ],
    },

    // ——— Cline ———
    "cline/api-request-failed": {
      title: "Cline API Request Failed——诊断真正的错误",
      description:
        "Cline 的 API Request Failed 横幅只是提供商响应的包装。如何读取其下的状态码和 error.type，把自己引导到真正的修复方法。",
      causes: [
        "横幅是通用的：任何失败的提供商调用 Cline 都显示它。诊断依据是下方的状态码和错误响应体，而不是横幅文字。",
        "内层是 401——密钥/端点不匹配或密钥被吊销。",
        "内层是 429——密钥的每分钟预算耗尽，并被 Cline 的自动重试放大。",
        "内层是带上下文提示的 400——对话加 max_tokens 已经装不进模型的窗口。",
      ],
      fixes: [
        "展开错误并阅读 JSON 响应体。每种响应体对应一个具体页面：401 对应无效密钥修复，429 对应速率限制修复，上下文 400 对应上下文超限修复。",
        "如果响应体是连接失败而不是 JSON，就当作网络/base URL 问题处理，用 curl 验证端点。",
        "反复失败时不要乱切无关设置——先在 Cline 之外复现，然后每次只改一个东西。",
      ],
      faq: [
        {
          q: "Cline 里的 API Request Failed 到底是什么意思？",
          a: "只说明对提供商的调用没有成功。真正的错误是随之显示的状态码和响应体——先读它们。",
        },
        {
          q: "Cline 反复重试反复失败——我该继续点重试吗？",
          a: "不该。对于 401 和 400，同样的请求会永远失败；先修复原因。重试只对瞬时的 429/5xx 有用，而且 Cline 自己已经在做了。",
        },
        {
          q: "怎么把 Cline 排除出等式？",
          a: "用 Cline 设置里的 base URL 和密钥，通过 curl 发送同样的请求。curl 的结果会告诉你问题出在配置还是工具。",
        },
      ],
    },
    "cline/invalid-api-key-401": {
      title: "Cline 401 invalid x-api-key（Anthropic）解决方法",
      description:
        "当 Anthropic 兼容端点拒绝密钥时，Cline 返回 401 invalid x-api-key：base URL 字段填错、密钥带空白，或密钥已被吊销。修复清单如下。",
      causes: [
        "自定义 base URL 选项是关闭的，于是密钥被拿去默认端点验证——那里从未见过它。",
        "密钥粘贴时带了前导或尾随空白，或被截断。",
        "base URL 包含路径后缀，请求发往端点不提供服务的 URL。",
        "密钥已被吊销或已过期。",
      ],
      fixes: [
        "在 Cline 的 Anthropic 提供商设置里启用自定义 base URL，并设置为签发密钥的源站——对于本网关是 https://router.apitoken.sale，不带 /v1 后缀。",
        "干净地重新粘贴密钥，并在签发方控制台确认它处于激活状态。",
        "回到 Cline 之前先用 curl 验证这一对配置。",
      ],
      snippetLabel: "Cline 提供商设置",
      faq: [
        {
          q: "为什么 Cline 拒绝一把在别处能用的密钥？",
          a: "如果自定义 base URL 复选框没有勾选，Cline 会把密钥发给 api.anthropic.com。先启用 base URL，同一把密钥就能验证通过。",
        },
        {
          q: "Cline 的 base URL 需要带 /v1 吗？",
          a: "不需要——只填源站。Cline 的 SDK 会自己追加 API 路径；带 /v1 后缀会产生重复路径和难以排查的故障。",
        },
        {
          q: "密钥能用之后我可以选哪些模型？",
          a: "端点提供的那些。用同样的 base URL 和密钥请求 GET /v1/models 即可列出。",
        },
      ],
    },
    "cline/rate-limit-429": {
      title: "Cline 429 rate_limit_error——终止重试循环",
      description:
        "Cline 的智能体运行会快速烧光每分钟 token 预算，然后陷入 429 重试循环。为什么智能体负载会触发速率限制，以及如何让一次运行跑完。",
      causes: [
        "智能体循环极其消耗 token：每一步都重发对话、文件上下文和工具结果，一个活跃任务就能独自花光一分钟的预算。",
        "密钥与其他工具或同事共享，他们的流量填充同一个窗口。",
        "窗口饱和期间的自动重试让窗口一直保持饱和。",
      ],
      fixes: [
        "让第一个 429 过去——Cline 会退避并重试。如果一次运行陷入 429 循环，暂停一分钟，而不是反复重启它。",
        "减少任务携带的上下文：更小的文件选择、每次运行更窄的任务范围。",
        "给 Cline 一把专用密钥，让其他工具的突发流量吃不到它的窗口——如果你持续需要更高吞吐量，就找密钥签发方提高上限，而不是和它对抗。",
      ],
      faq: [
        {
          q: "为什么 Cline 撞上 429 比聊天工具快得多？",
          a: "智能体的一步不是一条消息——它每次迭代都重发历史、文件上下文和工具输出。每分钟 token 吞吐量是聊天的好几倍。",
        },
        {
          q: "立刻重试有用吗？",
          a: "没用——窗口按分钟计，立刻重试只会让它一直满着。退避到窗口剩余时间结束才能清空它。",
        },
        {
          q: "这是 Cline 自己的限制吗？",
          a: "不是。使用你的密钥时 Cline 没有任何自己的计量；429 来自密钥背后 API 组织的每分钟上限。",
        },
      ],
    },
    "cline/context-limit": {
      title: "Cline 报 input length and max_tokens exceed context limit 的解决方法",
      description:
        "当 Cline 累积的任务上下文加上输出预留装不进模型窗口时，会出现 400 input length and max_tokens exceed context limit。如何恢复。",
      causes: [
        "对话历史加附加文件加 max_tokens 预留超出了模型的上下文窗口——消息里的两个数字就是你的输入和上限。",
        "长时间运行的任务会累积每次文件读取和工具结果；在窗口已满之前，什么都不会被自动丢弃。",
        "过大的 max_tokens 设置预留了输出空间，结果输入旁边再也放不下它。",
      ],
      fixes: [
        "为下一个工作单元开一个新任务——把一个巨型任务拖过整个功能开发正是填满窗口的原因。",
        "减少任务持有的内容：更窄的文件提及，避免粘贴本可以引用的整个文件。",
        "如果模型有更大上下文窗口的变体，选它可以抬高上限；否则应精简输入而不是调高 max_tokens。",
      ],
      faq: [
        {
          q: "错误里的数字是什么意思？",
          a: "你的输入 token 加 max_tokens 预留，对照模型的窗口：199999 + 8192 > 200000 表示在预留任何输出之前，输入就几乎填满了窗口。",
        },
        {
          q: "之前一切正常，为什么任务中途冒出这个错误？",
          a: "上下文每一步都在累积。任务在那一步越过了上限——不是那一步有什么特别，只是多走了一步。",
        },
        {
          q: "调低 max_tokens 能修好吗？",
          a: "有时能撑一会儿——它缩小了预留。持久的修复是减少输入：更小的任务、精简的上下文。",
        },
      ],
    },

    // ——— Zed ———
    "zed/invalid-api-key": {
      title: "Zed Claude 401 invalid x-api-key——Anthropic 设置修复",
      description:
        "Zed 的助手会原样透传 Anthropic 的 401 invalid x-api-key。密钥和 api_url 在 Zed 设置中的位置，以及为什么它们必须来自同一签发方。",
      causes: [
        "Zed 的 Anthropic 提供商设置里的密钥与 api_url 指向不同的签发方——网关密钥发给了默认端点，或者反过来。",
        "密钥粘贴时带了空白字符或被截断。",
        "自定义 api_url 设置时带了 /v1 后缀，请求打到重复的路径，在认证之前或代替认证而失败。",
        "密钥已被吊销或已过期。",
      ],
      fixes: [
        "把 language_models.anthropic.api_url 设置为签发密钥的源站，并在提供商设置里粘贴该签发方的密钥。",
        "api_url 只保留源站——Zed 会自己追加 /v1 路径。",
        "改动其他任何东西之前，先用 curl 验证这一对确切的配置。",
      ],
      snippetLabel: "针对本网关的 Zed settings.json",
      faq: [
        {
          q: "Zed 把 Anthropic base URL 存在哪里？",
          a: "在 settings.json 的 language_models.anthropic.api_url 下。API 密钥本身在助手的提供商配置里输入。",
        },
        {
          q: "为什么我的密钥在 Zed 里失败，在 curl 里却能用？",
          a: "Zed 把它发给配置的 api_url。如果那与你 curl 的 URL 不同——包括多出来的 /v1——那么看到密钥的端点就不是签发它的那个。",
        },
        {
          q: "在 Zed 里用 Claude 需要 Anthropic 账户吗？",
          a: "不需要——任何 Anthropic 兼容端点都可以：把 api_url 设为签发方并使用它的密钥。",
        },
      ],
    },
    "zed/rate-limit-429": {
      title: "Zed 助手 429 rate_limit_error 解决方法",
      description:
        "Zed 助手里的 429 是你密钥背后 API 组织的每分钟上限。为什么长线程和共享密钥会触发它，以及什么能让它消退。",
      causes: [
        "密钥所属组织的每分钟 token 预算被超出——长的助手线程每条消息都重发全部历史。",
        "同一把密钥同时被其他工具使用，它们的流量共享同一个窗口。",
        "大的上下文附件成倍放大每条消息携带的 token。",
      ],
      fixes: [
        "等过这一分钟窗口再继续——单个 429 不需要任何配置改动。",
        "新工作开新线程，而不是延伸一个长期运行的对话。",
        "如果当前密钥跨工具共享，就给 Zed 一把专用密钥。",
      ],
      faq: [
        {
          q: "这是 Zed 的限制还是我密钥的限制？",
          a: "密钥的。Zed 没有任何自己的计量；429 来自所配置密钥背后的 API 组织。",
        },
        {
          q: "为什么长线程会让情况更糟？",
          a: "每条消息都重发整个线程。即使你可见的问题很短，被计数的输入也随对话增长。",
        },
        {
          q: "立刻重试有用吗？",
          a: "没用——窗口按分钟计。立刻重试让它一直饱和；短暂停顿才能清空它。",
        },
      ],
    },
    "zed/overloaded-529": {
      title: "Zed 收到 Claude 529 Overloaded——该怎么办",
      description:
        "当上游容量饱和时，Zed 会原样显示 Anthropic 的 529 Overloaded。为什么你的设置不是原因，以及在它持续期间该做什么。",
      causes: [
        "上游容量暂时饱和。529 描述的是服务本身，而不是你的请求或你的 Zed 配置。",
        "它在故障期和高峰时段集中出现，然后在你这边无任何改动的情况下自行消退。",
      ],
      fixes: [
        "稍作停顿后重试——完全相同的请求通常在容量恢复后就能成功。",
        "对等不了的工作，把线程临时切换到较小的模型；较小的模型通常竞争更少。",
        "忍住别改配置：api_url 和密钥的改动修不了容量问题，反而常常在上面叠加一个真正的错误。",
      ],
      faq: [
        {
          q: "是我的 Zed 配置导致了 529 吗？",
          a: "不是。Overloaded 是被原样透传的服务端状况。如果你的配置有问题，产生的会是 401 或 404，而不是 529。",
        },
        {
          q: "一轮 529 会持续多久？",
          a: "通常几分钟。如果持续不退，去查提供商的状态页面，而不是改设置。",
        },
        {
          q: "529 和 429 是一回事吗？",
          a: "不是——429 是你自己的吞吐量上限，529 是上游容量。只有 429 会因为你减少用量而缓解。",
        },
      ],
    },
    "zed/api-url-config": {
      title: "Zed Anthropic api_url 自定义配置与 /v1/v1 404 陷阱",
      description:
        "如何用 language_models.anthropic.api_url 把 Zed 的 Anthropic 提供商指向自定义端点，以及为什么追加 /v1 会因路径重复产生 404。",
      causes: [
        "api_url 设置时包含了 /v1，而 Zed 会自己追加 API 路径——请求发往 /v1/v1/messages，这个路径不存在。",
        "api_url 被加在错误的设置键下或有拼写错误，Zed 悄悄继续使用默认端点。",
        "端点没错，但所选模型 id 是它不提供的——这是 404 的另一个来源。",
      ],
      fixes: [
        "api_url 只设置为源站，让 Zed 自己构建路径。",
        "把设置准确放在 settings.json 的 language_models.anthropic.api_url——放错位置的键不会报错，只是毫无作用。",
        "如果 404 响应体点名了某个模型，说明路径没问题，问题在模型 id：列出端点的模型并选一个它提供的 id。",
      ],
      snippetLabel: "正确的 settings.json",
      faq: [
        {
          q: "Zed 的 api_url 应该带 /v1 吗？",
          a: "不带。Zed 会自己追加 /v1/messages。以 /v1 结尾的 api_url 会产生重复的 /v1/v1 路径和 404。",
        },
        {
          q: "怎么确认我的自定义 api_url 真的生效了？",
          a: "故意把它改错测一次请求（填一个乱写的主机）——如果什么都没变，说明 Zed 没在读你编辑的那个键；修正设置路径。",
        },
        {
          q: "路径没错但仍然 404——为什么？",
          a: "404 响应体通常会点名缺失的东西。如果它点名一个模型，就选一个端点提供的 id；用 GET /v1/models 列出它们。",
        },
      ],
    },
  },
};
