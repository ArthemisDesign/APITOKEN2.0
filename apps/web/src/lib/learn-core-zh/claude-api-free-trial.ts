import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API 免费试用——几分钟即可开始",
    h1: "免费试用 Claude API",
    description: "几分钟内开始用 Claude 编码。通过 Google 或 GitHub 创建的新账户可获 $5 平台奖励余额，无需银行卡。",
    keywords: ["claude api 免费试用", "claude api free trial", "试用 claude api", "claude api 测试", "claude api 沙盒", "claude api 演示", "免费 claude api", "claude api 无需信用卡", "claude api 免费额度", "try claude api free", "claude api free tier"],
    dek: "这里不需要单独申请试用——Claude API 免费试用指的是：通过 Google 或 GitHub 创建新账户即获 $5 平台奖励余额，可用于对所有受支持模型的真实调用。不需要银行卡，没有沙盒模式，也没有要取消的东西。本指南介绍如何领取这笔余额、先测什么，以及 $5 到底能用多久。",
    sections: [
      { h2: "免费试用到底是什么", blocks: [
        { type: "p", text: "在 apiToken.sale 上，Claude API 免费试用不是演示环境，也不是功能受限的沙盒。通过 Google 或 GitHub 创建的新账户自带 $5 平台奖励余额，这笔余额能对所有受支持的模型发起真实的、按量计费的调用——端点、密钥和流式行为与付费用户完全一致。不需要银行卡，事后也没有要取消的套餐。" },
        { type: "p", text: "有一条资格规则要记住：奖励绑定的是注册方式，而不是账户本身。用邮箱加密码注册也能得到完全可用的账户，但初始余额为零——所以想要免费起步，注册时请选择 Google 或 GitHub。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户自带 $5 平台奖励余额——适用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱密码注册的账户不享受该奖励。" },
      ] },
      { h2: "一次操作领取余额并生成密钥", blocks: [
        { type: "steps", items: [
          "用 Google 或 GitHub 注册。$5 平台奖励会自动到达你的余额——这正是邮箱密码注册不会触发的一步。",
          "打开控制台生成 API 密钥。密钥形如 sk-pool-…，一把密钥覆盖所有受支持的 Claude、GPT、Gemini 和 Kimi 模型，无需按模型单独配置。",
          "选择你的工具已经在用的协议。Anthropic 原生客户端调用 https://router.apitoken.sale 并携带 x-api-key 请求头；OpenAI 兼容客户端调用 https://router.apitoken.sale/v1 并携带 Authorization: Bearer。两条通道从同一份试用余额扣费。",
        ] },
        { type: "p", text: "从注册到拿到可用密钥只要几分钟点击。你和第一个请求之间没有审批环节、没有销售电话、也没有等待名单——密钥生成即生效。" },
      ] },
      { h2: "用一次真实调用验证网关", blocks: [
        { type: "p", text: "最快的自检方式是发一个非流式的 Messages 请求，并把 token 上限设小。目的是确认鉴权、base URL 和模型 ID——不是为了生成内容。" },
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-haiku-4-5",\n    "max_tokens": 64,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
        { type: "p", text: "返回 200、带有 content 块和 usage 对象，就说明整条链路都通了：密钥、端点和计量。每个响应都会如实上报消耗的 token 数，控制台也会显示剩余余额，所以你能实时看到试用的花费，而不是靠猜。" },
        { type: "note", text: "返回 402 说明余额用完了，而不是密钥坏了。充值任意整数美元金额后重试同一个请求即可——密钥本身仍然有效。" },
      ] },
      { h2: "一次就能跑完的试用清单", blocks: [
        { type: "p", text: "把这 $5 当作验证预算来用。试用的目标不是搭出什么东西，而是在投入真金白银之前，证明你计划使用的一切通过网关都行为正常。" },
        { type: "table", headers: ["检查项", "通过的标准"], rows: [
          ["首个 200 OK", "返回带 content 块和 usage 对象的 JSON——密钥、base URL 和模型 ID 全部正确"],
          ["SSE 流式输出", "设置 stream: true 后，token 以 server-sent events 的形式增量到达，而不是一次缓冲返回"],
          ["工具调用", "模型返回 tool_use 块，并在你回复 tool_result 后正确继续"],
          ["你的编辑器", "Cursor、VS Code、Continue、Aider 或 Claude Code 指向网关后完成一次真实请求"],
          ["第二家提供商", "同一把密钥在 OpenAI 兼容通道上用受支持的 GPT 或 Gemini 模型应答"],
        ] },
      ] },
      { h2: "让 $5 撑过更多测试", blocks: [
        { type: "p", text: "如果你像工程师一样做评估，而不是像用户一样闲聊，五美元出人意料地耐用。四个习惯决定这笔余额是撑一个下午还是一个星期：" },
        { type: "list", items: [
          "用 claude-haiku-4-5 做迭代。接线测试、提示词草稿和错误路径检查都跑在最便宜的受支持 Claude 模型上；只有最终的质量对比才切到 claude-sonnet-5 或 claude-opus-4-8。",
          "压低 max_tokens 上限。Messages API 本来就要求这个字段，低上限能防止啰嗦的补全吃掉预算。",
          "用提示词缓存复用长上下文。如果你的测试循环反复发送同一个大提示词，把稳定的前缀标记为缓存，重复调用只按输入 token 的一小部分计费。",
          "读取每个响应的 usage 对象。input_tokens 和 output_tokens 会在你把工作负载放大到生产流量之前，告诉你它精确的成本形状。",
        ] },
      ] },
      { h2: "试用覆盖的不只是 Claude", blocks: [
        { type: "p", text: "奖励余额是全平台通用的，不限于 Claude。同一把密钥、同一份余额可以调用受支持的 GPT、Gemini 和 Kimi 模型，这让试用对跨模型评估真正有用：把同一个提示词发给多个模型，在你自己的任务上并排比较答案。" },
        { type: "code", code: `curl https://router.apitoken.sale/v1/chat/completions \\\n  -H "Authorization: Bearer sk-pool-•••" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "gpt-5.6-terra",\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
        { type: "p", text: "Gemini 原生工具用 x-goog-api-key 请求头对同一个路由域名鉴权，Kimi 模型在 Anthropic Messages 通道和 OpenAI 兼容通道上都能应答。一份余额，四家提供商，零额外注册。" },
      ] },
      { h2: "免费试用结束之后", blocks: [
        { type: "p", text: "没有试用到期日，也没有要选的套餐。余额不足时，充值任意整数美元金额——你的统一折扣立即生效——然后继续用同一把密钥调用。预付余额永不过期，没有订阅，也没有每月最低消费，所以试用结束后你只为实际用到的 token 付费。" },
        { type: "link", text: "根据试用的用量数字估算你真实的月度开销", href: "/tools/claude-api-cost-calculator" },
        { type: "link", text: "对比每个受支持的模型及其价格", href: "/models" },
        { type: "link", text: "把 curl、Python、Node 和你的 IDE 端到端接通", href: "/docs/learn/claude-api-quick-setup" },
      ] },
    ],
    faq: [
      { q: "Claude API 免费试用是独立的沙盒吗？", a: "不是。Google/GitHub 的 $5 平台奖励走的是与付费余额相同的生产端点和受支持模型——没有演示模式，也没有受限的功能集。" },
      { q: "不用信用卡怎么开始 Claude API 免费试用？", a: "用 Google 或 GitHub 创建新账户。$5 平台奖励自动到账，不会要求绑卡；邮箱密码账户不享受该奖励。" },
      { q: "试用期间能测试哪些模型？", a: "所有受支持的 Claude 模型——claude-opus-4-8、claude-sonnet-5、claude-haiku-4-5 等等——再加上同一把密钥、同一份余额下受支持的 GPT、Gemini 和 Kimi 模型。" },
      { q: "试用余额归零后会怎样？", a: "调用会返回 402，直到你充值任意整数美元金额；你的统一折扣立即生效，密钥保持有效，预付余额永不过期。" },
      { q: "试用能配合 Cursor 或 Claude Code 用吗？", a: "可以。把工具的 Anthropic base URL 指向 https://router.apitoken.sale，粘贴 sk-pool-… 密钥，请求就会像其他调用一样从试用余额扣费。" },
    ],
  };
