import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API 免费试用——几分钟即可开始",
    h1: "免费试用 Claude API",
    description: "几分钟内开始用 Claude 编码。通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额，无需银行卡。",
    keywords: ["claude api 免费试用", "试用 claude api", "claude api 测试", "claude api 沙盒", "claude api 演示"],
    dek: "无需单独申请试用——通过 Google 或 GitHub 创建账户，即可获得 $5 平台欢迎奖励余额，并对所有受支持的模型运行真实调用。",
    sections: [
      { h2: "先验证再付费", blocks: [
        { type: "p", text: "包含的用量正是为端到端检验网关而设计的：创建密钥、连接你的编辑器，确认流式输出、工具调用以及你喜欢的模型都表现如预期。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码注册不享受此奖励。" },
      ] },
      { h2: "随后按你的节奏扩展", blocks: [
        { type: "p", text: "当试用用量所剩不多时，充值任意金额即可。没有订阅、余额永不过期，因此你只为实际调用的部分付费。" },
      ] },
    ],
    faq: [
      { q: "我如何开始试用？", a: "通过 Google 或 GitHub 创建新账户，$5 平台欢迎奖励余额会自动添加；邮箱密码账户不参与。" },
      { q: "免费用量用完后会怎样？", a: "充值任意整数美元金额即可继续；你的统一折扣会立即生效。" },
    ],
  };
