import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Anthropic SDK 自定义 Base URL 配置指南",
    h1: "把 Anthropic SDK 指向 apitoken.sale",
    description: "只需把 base_url 设为 router.apitoken.sale,即可在官方 Anthropic Python 和 TypeScript SDK 中接入 apitoken.sale。同一个 SDK、同一份代码,每 token 成本更低。",
    keywords: ["anthropic sdk base url", "anthropic sdk 自定义 base url", "anthropic python sdk 自定义端点", "claude sdk base url", "anthropic typescript sdk", "claude api sdk", "ANTHROPIC_BASE_URL 环境变量", "claude api 自定义端点", "anthropic sdk 代理", "@anthropic-ai/sdk baseURL"],
    dek: "每个官方 Anthropic SDK 都支持自定义 Base URL,所以迁移到 apitoken.sale 只是改一个参数的事。你的模型 ID、消息代码和流式逻辑完全不变——变的只有端点和每 token 的价格。",
    sections: [
      { h2: "一个参数,切换端点", blocks: [
        { type: "p", text: "两个官方 Anthropic SDK——Python 和 TypeScript——都允许在构造客户端时覆盖 API 根地址。把它设为 https://router.apitoken.sale,代码里已有的每个请求就会由 apitoken.sale 的网关接管,而不再发往 api.anthropic.com。代码库中其他一切都不用动:同一个 anthropic 包、同一个 Messages API、同样的模型 ID(如 claude-opus-4-8)、同样的响应对象。" },
        { type: "p", text: "变化的是计费。每次调用按 Anthropic 官方 token 费率计量,再减去固定 50% 折扣,净额从你的预付余额中扣除——余额按整数美元金额充值。没有订阅、没有按席位收费,闲置的日子一分钱不花。" },
      ] },
      { h2: "Python:客户端上的 base_url", blocks: [
        { type: "code", code: `from anthropic import Anthropic\n\nclient = Anthropic(\n    base_url="https://router.apitoken.sale",\n    api_key="sk-pool-•••",\n)\nmsg = client.messages.create(\n    model="claude-opus-4-8",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Hello"}],\n)` },
        { type: "p", text: "异步客户端接受完全相同的关键字参数:AsyncAnthropic(base_url=..., api_key=...)。通过 client.messages.stream 的流式输出、工具调用、系统提示词和提示词缓存都走同一条连接——没有需要单独配置的端点。" },
        { type: "note", text: "传裸根地址,不要带路径。SDK 会自己补上 /v1/messages,所以 base_url=\".../v1\" 会产生指向 /v1/v1/messages 的请求并返回 404。TypeScript SDK 同样遵循这条规则。" },
      ] },
      { h2: "TypeScript:客户端上的 baseURL", blocks: [
        { type: "code", code: `import Anthropic from "@anthropic-ai/sdk";\n\nconst client = new Anthropic({\n  baseURL: "https://router.apitoken.sale",\n  apiKey: "sk-pool-•••",\n});\nconst msg = await client.messages.create({\n  model: "claude-opus-4-8",\n  max_tokens: 1024,\n  messages: [{ role: "user", content: "Hello" }],\n});` },
        { type: "p", text: "@anthropic-ai/sdk 包会自动替你发送 x-api-key 和 anthropic-version 请求头,与访问官方端点时完全一样。重试、超时和错误类(APIError、RateLimitError 等)的行为完全一致,现有的错误处理逻辑照常工作。" },
      ] },
      { h2: "共享代码优先用环境变量", blocks: [
        { type: "p", text: "当构造函数参数缺省时,两个 SDK 都会从环境变量读取 ANTHROPIC_BASE_URL 和 ANTHROPIC_API_KEY。这让切换端点变成部署层面的细节,而不是代码改动——当同一个仓库在开发和生产环境要打不同端点时尤其有用。" },
        { type: "code", code: `export ANTHROPIC_BASE_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••\n\n# your code now constructs Anthropic() with no arguments` },
        { type: "p", text: "基于 SDK 构建的工具会继承同一组环境变量。比如 Claude Code 直接读取 ANTHROPIC_BASE_URL 和 ANTHROPIC_API_KEY;LangChain、LiteLLM 这类框架也会把同样的环境传递给底层的 Anthropic 客户端。两者同时设置时,显式的构造函数参数优先于环境变量,所以脚本里的一次性覆盖不会泄漏进你的部署配置。" },
      ] },
      { h2: "原样穿过网关的能力", blocks: [
        { type: "list", items: [
          "完整的 Messages API 接口:POST /v1/messages,请求和响应 JSON 完全一致。",
          "SSE 流式输出——增量分块的表现与 api.anthropic.com 一模一样。",
          "工具调用与函数调用,包括多轮 tool_result 循环。",
          "系统提示词、视觉输入,以及带 cache_control 断点的提示词缓存。",
          "每个响应都带 usage 对象,你的 token 与成本统计代码照常工作。",
          "模型 ID:claude-opus-4-8、claude-sonnet-5、claude-haiku-4-5 以及支持目录中的其余模型。",
        ] },
        { type: "p", text: "一个密钥覆盖所有支持的模型——Claude 与 GPT、Gemini、Kimi 并列——多供应商项目只需一份凭证、一份余额。每次调用后,控制台会显示该请求的消费和已应用的折扣。" },
        { type: "link", text: "支持的模型 ID 与各模型定价", href: "/models" },
        { type: "link", text: "在成本计算器中估算月度开销", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "首次请求检查清单与常见错误", blocks: [
        { type: "steps", items: [
          "注册免费账户,打开控制台并生成密钥——它形如 sk-pool-…,可用于支持的 Claude、GPT、Gemini 和 Kimi 模型。",
          "在代码中把 base_url / baseURL 设为 https://router.apitoken.sale,或导出 ANTHROPIC_BASE_URL 和 ANTHROPIC_API_KEY。",
          "运行一次上面的 Python 或 TypeScript 示例,确认你能收到正常的 Anthropic 消息响应。",
          "打开控制台,确认该请求连同它的 token 用量、费用和折扣都已显示。",
        ] },
        { type: "table", headers: ["状态码", "含义", "解决办法"], rows: [
          ["401 Unauthorized", "x-api-key 缺失或错误,或 Base URL 不对", "重新检查密钥,并确认 URL 是裸根地址"],
          ["400 Bad Request", "请求体格式有误", "检查模型 ID,并确认设置了 max_tokens"],
          ["402 Payment Required", "预付余额不足", "在控制台按整数美元金额充值"],
          ["429 Too Many Requests", "并发超出当前限额", "遵守 Retry-After 并降低并行度"],
        ] },
        { type: "p", text: "由于 SDK、线上格式和错误分类在两个端点上完全一致,切换随时可逆:把 base_url 指回 api.anthropic.com(或删掉覆盖项),同一份代码就重新直接对接 Anthropic。很多团队在迁移周会让两个客户端并存,先把一小部分流量路由到新端点,再全量切换。" },
        { type: "note", text: "旧的 https://api.apitoken.sale 主机上的既有集成继续可用。新接入建议使用统一路由器 router.apitoken.sale,因为一个 Base URL 即可服务全部四家供应商。" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台欢迎奖励余额——适用于支持的 Claude、GPT、Gemini 与 Kimi 模型;邮箱密码账户不参与。" },
      ] },
    ],
    faq: [
      { q: "我还能继续用官方 Anthropic SDK 吗?", a: "可以。把 base_url(Python)或 baseURL(TypeScript)设为 https://router.apitoken.sale,其余一切——import、模型 ID、流式、错误处理——都保持不变。" },
      { q: "切换 Base URL 后模型 ID 会变吗?", a: "不会。沿用官方 API 上的模型 ID,例如 claude-opus-4-8、claude-sonnet-5 和 claude-haiku-4-5。" },
      { q: "Base URL 要以 /v1 结尾吗?", a: "不要。SDK 会在你传入的根地址后面自己补上 /v1/messages,末尾带 /v1 会把路径弄错。原样传入 https://router.apitoken.sale 即可。" },
      { q: "自定义 Base URL 下,流式和工具调用能用吗?", a: "可以。网关服务的是标准 Anthropic Messages API,所以 SSE 流式、工具调用、系统提示词和提示词缓存的表现与 api.anthropic.com 完全一致。" },
      { q: "以后怎么切回 Anthropic?", a: "删掉 base_url / baseURL 参数,或取消设置 ANTHROPIC_BASE_URL。SDK 会默认回到 https://api.anthropic.com——不需要其他任何代码改动。" },
    ],
  };
