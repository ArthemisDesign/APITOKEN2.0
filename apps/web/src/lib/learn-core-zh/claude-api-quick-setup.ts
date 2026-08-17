import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "两分钟完成 Claude API 配置",
    h1: "两分钟配置好 Claude API",
    description: "两分钟的 Claude API 快速上手：创建密钥、把 Base URL 设为 router.apitoken.sale，然后用 curl、Python 或你的 IDE 发出第一个 /v1/messages 请求。",
    keywords: ["claude api 快速上手", "claude api 配置", "claude api 第一个请求", "anthropic messages api", "claude api base url"],
    dek: "这是从零到跑通 Claude API 调用的最快路径。下面的一切都使用标准的 Anthropic Messages API，因此可以直接嵌入你现有的代码。",
    sections: [
      { h2: "1. 创建密钥", blocks: [ { type: "p", text: "注册、打开控制台并生成一把密钥。它形如 sk-pool-…，可用于所有受支持的模型。" } ] },
      { h2: "2. 设置端点", blocks: [
        { type: "p", text: "将任意兼容 Anthropic 的客户端指向网关：" },
        { type: "code", code: `Base URL:  https://router.apitoken.sale\nEndpoint:  POST /v1/messages\nHeaders:   x-api-key: sk-pool-•••\n           anthropic-version: 2023-06-01` },
      ] },
      { h2: "3. 发出第一个请求", blocks: [
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-opus-4-8",\n    "max_tokens": 1024,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额；邮箱密码账户不享受此奖励。" },
      ] },
      { h2: "首次调用的常见错误", blocks: [
        { type: "list", items: [
          "401 Unauthorized——缺少或写错 x-api-key，或 Base URL 有误。",
          "400 Bad Request——检查模型 ID，并确认已设置 max_tokens。",
          "429 Too Many Requests——遵守 Retry-After 并降低并发。",
          "402 / 余额不足——充值任意整数美元金额即可。",
        ] },
      ] },
    ],
    faq: [
      { q: "我该用哪个 Base URL？", a: "在任意兼容 Anthropic 的工具中使用 https://router.apitoken.sale，并向 /v1/messages 发送请求。基于旧主机 https://api.apitoken.sale 的既有集成仍可正常使用——统一 router 只是新设置的推荐端点。" },
      { q: "需要哪个鉴权请求头？", a: "发送 x-api-key（携带你的密钥）和 anthropic-version，与官方 Anthropic API 完全一致。" },
    ],
  };
