import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Kimi API 定价详解",
    h1: "Kimi API 定价：缓存命中、未命中、输出与速度",
    description: "了解 Kimi K3、Kimi for Coding 与 High Speed 的缓存命中、未命中、输出费率、别名映射和 apiToken.sale 固定五折。",
    keywords: ["kimi api 定价", "kimi k3 价格", "kimi for coding 价格", "kimi token 成本", "kimi k2.7 code 价格", "便宜 kimi api"],
    dek: "Kimi 分别公布缓存命中、缓存未命中和输出费率。apiToken.sale 按实际服务模型定价，保持计费项互斥，再应用固定 50% 折扣。",
    sections: [
      { h2: "公开别名对应的官方费率", blocks: [
        { type: "table", headers: ["公开别名", "官方命中 / 未命中 / 输出", "五折后价格"], rows: [
          ["kimi/k3 · k3-256k · k3[1m]", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
          ["kimi/kimi-for-coding", "$0.19 / $0.95 / $4", "$0.095 / $0.475 / $2"],
          ["kimi/kimi-for-coding-highspeed", "$0.38 / $1.90 / $8", "$0.19 / $0.95 / $4"],
        ] },
        { type: "p", text: "以上均为每 100 万 token。Kimi 自动缓存，且没有独立缓存写入价格；新缓存 token 视为未命中，而不是免费或隐藏的第四项。" },
      ] },
      { h2: "如何控制成本", blocks: [
        { type: "list", items: [
          "Kimi for Coding 是公开 Kimi 集合中成本最低的通用编程选项。",
          "只有延迟收益值得两倍 token 费率时才使用 High Speed。",
          "任务不需要大窗口时，选择 k3-256k 而不是完整 1M 写法。",
          "设置密钥终身消费上限，并在仪表板检查终态 usage。",
        ] },
        { type: "note", text: "推理 token 是输出的子集，按输出费率结算，不会作为独立项目再次收费。" },
      ] },
    ],
    faq: [
      { q: "Kimi for Coding 多少钱？", a: "官方为 $0.19/百万缓存命中、$0.95/百万缓存未命中、$4/百万输出；apiToken.sale 收取一半。" },
      { q: "为什么缓存命中与未命中价格不同？", a: "Kimi 自动缓存重复上下文。终态 usage 标识缓存命中输入，每个互斥项目使用自己的官方费率。" },
      { q: "High Speed 更贵吗？", a: "是。缓存命中、未命中与输出费率均为基础 Kimi for Coding 的两倍。" },
    ],
  };
