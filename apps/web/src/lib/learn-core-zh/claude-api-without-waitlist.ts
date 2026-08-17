import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "无需排队或审批的 Claude API",
    h1: "无需排队即可使用 Claude API",
    description: "跳过 Anthropic 的排队和审批。在 apitoken.sale 上创建账户、生成 Claude API 密钥，几分钟内即可发出第一个调用。",
    keywords: ["claude api 无排队", "claude api 即时开通", "claude api 无需审批", "快速获取 claude api 密钥", "claude api 无需 anthropic 账户"],
    dek: "等待审批会消磨积极性。apitoken.sale 让你即时、自助地用上所有受支持的 Claude 模型——无排队、无销售电话、无公司验证。",
    sections: [
      { h2: "即时、自助开通", blocks: [ { type: "steps", items: [
          "创建一个免费账户并打开控制台——无需审批、无需排队。",
          "生成一把 API 密钥（形如 sk-pool-…）。同一把密钥可用于所有受支持的 Claude 模型。",
          "将任意兼容 Anthropic 的工具指向 https://router.apitoken.sale，并携带 x-api-key 请求头向 /v1/messages 发送请求。",
        ] }, { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" } ] },
      { h2: "「即时」到底是什么意思", blocks: [
        { type: "p", text: "你一生成密钥它就是激活的。从注册到第一个成功请求之间没有任何人工审核步骤，因此你可以在同一次坐下就接通工具并交付。" },
      ] },
      { h2: "从零到第一个调用", blocks: [
        { type: "list", items: [
          "注册并打开控制台——没有审批步骤。",
          "生成密钥并把你的工具指向 router.apitoken.sale。",
          "发出请求，即可在用量中看到它被计量。",
        ] },
        { type: "p", text: "通过 Google 或 GitHub 创建的新账户还会附带 $5 平台欢迎奖励余额，因此你可以在充值前验证整个流程。" },
      ] },
    ],
    faq: [
      { q: "真的没有排队吗？", a: "没错。开通是自助且即时的——你生成一把密钥，它在下一次请求就能用。" },
      { q: "我需要联系销售吗？", a: "不需要。B2C 开通完全自助。只有需要商议的 B2B 批量定价才涉及沟通。" },
    ],
  };
