import type { LocalizedContent } from "../learn";
import { BASE, KEY } from "../learn-shared";

export const content: LocalizedContent = {
    title: "Claude Code API 密钥：两个环境变量完成配置",
    h1: "用 apiToken.sale API 密钥运行 Claude Code",
    description: "无需 Anthropic 订阅即可获取 Claude Code API 密钥：把 ANTHROPIC_BASE_URL 和 ANTHROPIC_API_KEY 指向 router.apitoken.sale，用预付余额以官方价格统一 5 折运行所有 Claude 模型。",
    keywords: ["claude code api 密钥", "claude code 配置", "claude code anthropic base url", "claude code 环境变量", "claude code 自定义 api 密钥", "claude code 不用 anthropic 账号", "anthropic_api_key claude code", "claude code 按量付费", "claude code 模型设置", "claude code api 密钥无效"],
    dek: "Claude Code 的端点和凭证来自两个环境变量，因此 apiToken.sale 的 Claude Code API 密钥可以直接替代订阅计费：只需设置一次 ANTHROPIC_BASE_URL 和 ANTHROPIC_API_KEY，CLI 就在你的预付余额上照常运行。下面是完整配置步骤、每次会话该选哪个模型，以及人人都会先踩到的三个错误的修复方法。",
    sections: [
      { h2: "设置 ANTHROPIC_BASE_URL 和 ANTHROPIC_API_KEY", blocks: [
        { type: "p", text: "Claude Code 从 ANTHROPIC_BASE_URL 读取端点，从 ANTHROPIC_API_KEY 读取凭证。把这两个变量指向 apiToken.sale，CLI 的行为和之前完全一致——只是计费从按月订阅变成从预付余额扣减，且相对 Anthropic 官方花费统一打 5 折。不涉及任何插件、代理或封装。" },
        { type: "steps", items: [
          "在 apiToken.sale 注册免费账户，并在控制台生成一把密钥。它形如 sk-pool-…，同一把密钥覆盖所有受支持的 Claude 模型。",
          `在 shell 中导出两个变量：ANTHROPIC_BASE_URL=${BASE} 和 ANTHROPIC_API_KEY=${KEY}。`,
          "在任意项目目录运行 claude，问它一个小问题——一句一行的提问就足以确认密钥已经生效。",
        ] },
        { type: "code", code: `export ANTHROPIC_BASE_URL=${BASE}\nexport ANTHROPIC_API_KEY=${KEY}\n\n# then just run it in your project\nclaude` },
      ] },
      { h2: "让变量在终端之间持久生效", blocks: [
        { type: "p", text: "export 只在当前 shell 会话中有效。关掉终端，Claude Code 就丢了凭证——这是明明配置好的环境“第二天突然失效”最常见的原因。把这两行写进 shell 启动文件——macOS 上是 ~/.zshrc，多数 Linux 环境是 ~/.bashrc——之后就会自动加载。" },
        { type: "code", code: `echo 'export ANTHROPIC_BASE_URL=${BASE}' >> ~/.zshrc\necho 'export ANTHROPIC_API_KEY=${KEY}' >> ~/.zshrc\nsource ~/.zshrc` },
        { type: "note", text: "变量必须用 export 导出，而不只是赋值，因为 Claude Code 是 shell 的子进程，只继承导出的变量。如果你是手动编辑的启动文件，先打开一个新终端，或对文件执行 source，再启动 claude。" },
      ] },
      { h2: "按会话选模型，而不是按月", blocks: [
        { type: "p", text: "模型选择是 Claude Code 里最大的成本杠杆，而预付密钥把它从套餐级决策变成了按会话决策。一把密钥覆盖整条受支持的产品线，所以你可以默认用中档模型，只在任务值得时才升级。" },
        { type: "table", headers: ["模型 ID", "适用场景"], rows: [
          ["claude-sonnet-5", "日常编码：功能开发、测试、小修小补。大多数会话的合理默认。"],
          ["claude-opus-4-8", "高难度重构、多文件推理，以及出错代价高昂的长智能体会话。"],
          ["claude-haiku-4-5", "快速提问、低成本试错，以及速度比深度更重要的大批量步骤。"],
        ] },
        { type: "p", text: "会话中途用 /model 命令切换，或在启动时指定模型：claude --model claude-opus-4-8。一个实用做法是先用 claude-sonnet-5，只有当 Sonnet 在同一个问题上卡住两次时才升级到 Opus。" },
        { type: "link", text: "各模型价格与上下文窗口", href: "/models" },
      ] },
      { h2: "变了什么，什么没变", blocks: [
        { type: "p", text: "自带密钥只改变计费，其他一切不变。Claude Code 还是同一个二进制，对着同一个 Anthropic Messages API，你每天用的功能照常工作。" },
        { type: "list", items: [
          "智能体编辑、工具调用和流式输出行为完全一致——只有计费端点变了。",
          "模型 ID 不变：claude-opus-4-8、claude-sonnet-5、claude-haiku-4-5。",
          "同一把密钥也能在 Cursor、Cline、Continue、Aider 和 Anthropic 官方 SDK 中使用，一份余额覆盖你的整条工具链。",
          "余额为预付制且永不过期；只在请求真正运行时扣费，B2C 账户相对官方花费统一打 5 折。",
        ] },
      ] },
      { h2: "看清每次会话花了多少", blocks: [
        { type: "p", text: "控制台显示每个请求的 token 用量，漫长的 Claude Code 会话不再是黑盒——你能精确看到哪些提示词和模型消耗了余额。可用银行卡或加密货币充值任意整数美元金额；没有固定商品目录，也没有按月承诺。" },
        { type: "link", text: "在免费计算器中估算一个月的 Claude Code 用量", href: "/tools/claude-api-cost-calculator" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额，可用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "先修好人人都会遇到的三个错误", blocks: [
        { type: "table", headers: ["症状", "可能原因", "解决办法"], rows: [
          ["鉴权失败或 401 错误", "变量有拼写错误，或变量只赋值没有导出", "重新检查 ANTHROPIC_BASE_URL 和 ANTHROPIC_API_KEY，然后重启 shell，确保它们已导出。"],
          ["Claude Code 忽略密钥，仍走订阅", "旧的订阅登录仍然生效", "在 Claude Code 内运行 /logout（或用 /status 检查），然后重新启动，让它使用环境变量里的密钥。"],
          ["返回 429 限流响应", "瞬时并发过高", "遵守 Retry-After，退避重试并降低并行度；如需持续更高的吞吐量，请联系支持。"],
        ] },
        { type: "note", text: "如果按上面的方法处理后错误仍然存在，可通过 Telegram 联系支持，提供英文和俄文服务。" },
      ] },
      { h2: "给 Claude Code 一把专属密钥", blocks: [
        { type: "p", text: "一个 apiToken.sale 账户可以签发多把命名密钥，所以给 Claude Code 单独配一把，而不是所有工具共用一把。万一某把密钥从 shell 历史文件或误提交的 dotfile 里泄露，只需吊销这一把，其余配置照常运行。" },
        { type: "list", items: [
          "给 Claude Code 的密钥设置终身消费上限，把失控会话的影响范围封顶。",
          "如果密钥用于短期项目或外包人员的机器，加上过期时间。",
          "密钥放在环境变量或密钥管理器里——绝不放进 git、公开发布的 dotfile 或聊天消息。",
        ] },
      ] },
    ],
    faq: [
      { q: "如何获取 Claude Code 的 API 密钥？", a: "注册免费的 apiToken.sale 账户，在控制台生成一把密钥（形如 sk-pool-…），然后把 ANTHROPIC_BASE_URL 设为 https://router.apitoken.sale，把 ANTHROPIC_API_KEY 设为这把密钥。运行 claude，就完成了。" },
      { q: "没有 Anthropic 订阅能用 Claude Code 吗？", a: "可以。Claude Code 接受普通 API 密钥，用 apiToken.sale 密钥时按预付余额计费，相对官方花费统一打 5 折。余额永不过期，轻度用户不必再为闲置的月份付费。" },
      { q: "Claude Code 里该设哪个模型？", a: "日常编码默认 claude-sonnet-5，高难度重构和长会话用 /model 切到 claude-opus-4-8，低成本大批量步骤用 claude-haiku-4-5。" },
      { q: "为什么 Claude Code 提示我的 API 密钥无效？", a: "几乎都是拼写错误，或者变量设置了但没有导出。重新检查两个环境变量并重启 shell；如果 Claude Code 仍在用订阅登录，先运行 /logout。" },
      { q: "用预付密钥怎么追踪 Claude Code 的花费？", a: "apiToken.sale 控制台按请求显示这把密钥的 token 用量。如果想给单次会话的消耗加个硬顶，就给密钥设一个终身消费上限。" },
    ],
  };
