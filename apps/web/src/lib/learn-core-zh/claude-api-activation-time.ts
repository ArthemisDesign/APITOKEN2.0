import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API 激活有多快？",
    h1: "你的 Claude API 密钥激活有多快",
    description: "apitoken.sale 的密钥即时激活。生成密钥、充值，几分钟内即可成功发出 Claude API 调用——无需人工审核或排队。",
    keywords: ["claude api 激活时间", "claude api 密钥多快", "即时 claude api 密钥", "claude api 就绪时间"],
    dek: "从创建密钥到使用它之间没有任何等待期。激活是即时的，速度唯一的限制就是你把密钥粘贴进工具有多快。",
    sections: [
      { h2: "为即时而设计", blocks: [
        { type: "p", text: "密钥一经生成即刻可用。充值在支付确认后立即入账，而银行卡支付几秒内即可确认。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "什么可能带来短暂延迟", blocks: [
        { type: "p", text: "唯一的等待是支付确认。银行卡充值几秒内到账；加密货币充值则在网络确认交易后入账，具体时间取决于你选择的币种和手续费。" },
        { type: "list", items: [
          "密钥生成：即时。",
          "银行卡充值：几秒。",
          "加密货币充值：网络确认之后。",
        ] },
      ] },
    ],
    faq: [
      { q: "我的密钥多久能用？", a: "立即可用。没有人工审核——刚生成的密钥在下一次请求即可使用。" },
      { q: "充值需要多久？", a: "银行卡支付几秒内到账；加密货币在网络确认交易后入账。" },
    ],
  };
