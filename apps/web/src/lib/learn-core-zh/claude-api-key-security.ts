import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "保护你的 Claude API 密钥",
    h1: "让你的 Claude API 密钥保持安全",
    description: "如何在 apiToken.sale 上保护 Claude API 密钥：终身累计消费上限、可选到期日期、名称清晰的单独密钥、及时吊销和安全存储。",
    keywords: ["claude api 密钥安全", "保护 api 密钥", "轮换 claude api 密钥", "claude api 密钥管理", "anthropic 密钥安全"],
    dek: "你的密钥会花掉真实余额，所以要把它当作凭据对待。apiToken.sale 提供多种管控，在密钥万一泄露时限制影响范围。",
    sections: [
      { h2: "限制风险的管控", blocks: [
        { type: "list", items: [
          "为密钥设置终身累计消费上限。",
          "如果临时访问应自动结束，请选择到期日期。",
          "为每个工具或环境签发名称清晰的单独密钥。",
          "要更换密钥，请先创建新密钥、更新客户端，再吊销旧密钥。",
        ] },
      ] },
      { h2: "基本卫生习惯", blocks: [
        { type: "list", items: [
          "绝不把密钥提交到 git 或粘贴到聊天中。",
          "把密钥存放在环境变量或密钥管理器中。",
          "一旦密钥暴露，立即吊销并轮换。",
        ] },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
    ],
    faq: [
      { q: "密钥泄露时如何把损失降到最低？", a: "使用终身累计消费上限和到期日期，为不同客户端保留名称清晰的单独密钥，并立即吊销已暴露的密钥。" },
      { q: "密钥应该存在哪里？", a: "存在环境变量或密钥管理器中——绝不提交到 git 或在聊天中分享。" },
    ],
  };
