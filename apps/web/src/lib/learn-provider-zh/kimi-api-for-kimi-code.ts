import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "在 Kimi Code 中使用 apiToken.sale",
    h1: "在 Kimi Code 中运行 Kimi、Claude、GPT 和 Gemini",
    description: "为 Kimi Code 配置 OpenAI 兼容 provider 接入 apiToken.sale:config.toml 中 provider 与 models 表的完整写法、带命名空间的目录模型 ID、上下文窗口与密钥管理。",
    keywords: ["kimi code api", "kimi code 自定义 provider", "kimi code config.toml", "kimi code api key", "kimi code openai 兼容 provider", "kimi code 第三方 provider", "kimi code claude gpt gemini", "kimi code base_url", "kimi code models 表", "kimi code 预付费 api"],
    dek: "Kimi Code 原生支持第三方 OpenAI 兼容 provider,因此 config.toml 里一个 apiToken.sale provider 块就能触达整个统一目录——一把预付费密钥同时运行 Kimi、Claude、GPT 和 Gemini。整个接入就是两张 TOML 表:一张 provider 表存放端点和凭证,每个要用的模型再加一张模型别名表。",
    sections: [
      { h2: "Kimi Code 天生就会说路由器的协议", blocks: [
        { type: "p", text: "要在 Kimi Code 中使用 apiToken.sale 的 Kimi API 密钥,在 ~/.kimi-code/config.toml 中声明一个 type = \"openai\"、base_url 为 https://router.apitoken.sale/v1 的 provider,再通过 [models] 别名把每个模型绑定到它。不需要插件、代理或补丁:CLI 的 openai provider 类型讲的就是 Chat Completions 协议,而这正是路由器通用通道所服务的协议。" },
        { type: "p", text: "这套配置刻意拆成两半。provider 条目负责协议、端点和凭证;model 条目负责你输入的别名、发给服务器的线上 ID,以及 CLI 用来做预算的上下文窗口。正是这个拆分让多 provider 密钥在这里用得顺手——provider 只写一次,以后加 Claude、GPT 或 Gemini 只是每个模型加一张小表,而不是新增一份凭证。" },
        { type: "p", text: "动手之前有一个行为必须知道:Kimi Code 只从配置文件解析凭证。它先查 provider 的 api_key 字段,再查 [providers.<name>.env] 子表,两者都没有就在启动时直接报错。在 shell 里 export 变量没有任何作用——对 provider 凭证,CLI 从不回退到 shell 环境变量。" },
        { type: "note", text: "通过 Google 或 GitHub 注册的新账户可获得 $5 平台奖励金——可用于受支持的 Claude、GPT、Gemini 和 Kimi 模型;邮箱/密码注册的账户不享受该奖励。" },
      ] },
      { h2: "安装 CLI 并写好 provider 块", blocks: [
        { type: "steps", items: [
          "用官方脚本安装 Kimi Code(无需预装 Node.js):curl -fsSL https://code.kimi.com/kimi-code/install.sh | bash——脚本会校验安装包的校验和,并把 kimi 可执行文件放到 PATH 上。",
          "注册 apiToken.sale 账户,用银行卡或加密货币充值任意整数美元金额,生成一个 API 密钥(形如 sk-pool-…)。同一把密钥和同一份余额覆盖受支持的 Kimi、Claude、GPT 和 Gemini 模型。",
          "按下面的示例,把 provider 和第一个模型别名写入 ~/.kimi-code/config.toml。",
          "用 chmod 600 ~/.kimi-code/config.toml 收紧文件权限——里面存着明文密钥。",
        ] },
        sourceBlock("kimi-api-for-kimi-code", 1, 1),
        { type: "note", text: "这套配置不要执行 /login。该命令会启动指向 Kimi Code 托管服务的 OAuth device-code 流程,走的是 Kimi 会员扣费而不是你的预付费余额——托管账户甚至不会出现在 /provider 列表里。手写 provider 块是确定性路线;交互式的 /provider 管理器确实存在,但它是围绕公开目录设计的,不是为预付费多 provider 网关准备的。" },
      ] },
      { h2: "在第一个真实任务之前先验证路由", blocks: [
        { type: "steps", items: [
          "用别名启动会话:kimi -m apitoken/k3。",
          "运行 /status,确认当前模型显示为 apitoken/k3——它报告的是会话运行时状态:版本、模型、工作目录和权限模式。",
          "发一条确定性提示词:Reply with exactly: connected。一次干净的回答就能在单个往返里同时证明密钥、base_url 和余额都正常。",
          "列出这把密钥实际能调用的模型:curl https://router.apitoken.sale/v1/models -H \"Authorization: Bearer sk-pool-•••\"——目录按密钥限定范围,只显示当前可路由且已对它定价的模型。",
        ] },
        { type: "note", text: "如果你在 TUI 打开时改了 config.toml,运行 /reload。它无需重启 CLI 就能应用 provider 和模型变更;而新的 shell export 不会生效,因为配置文件是唯一的凭证来源。" },
      ] },
      { h2: "一个 provider 块,覆盖所有模型家族", blocks: [
        { type: "p", text: "别名([models.\"...\"] 的键)只是本地名称。路由器真正路由的是 model 字段,它要求统一目录的命名空间 ID——kimi/k3、openai/gpt-5.6-terra、google/gemini-3.6-flash。由于 provider 已经持有端点和密钥,每加一个模型只需要三行:" },
        sourceBlock("kimi-api-for-kimi-code", 3, 1),
        { type: "list", items: [
          "每个别名都必须声明 max_context_size。CLI 用它做溢出检查,并决定何时触发自动压缩,所以照抄模型经过核验的窗口——K3 的 1M 模式是 1048576,Kimi for Coding 是 262144——而不是靠猜。",
          "Kimi Code 会按已知模型名前缀自动识别 thinking、vision、tool use 等能力。对于它可能不认识的命名空间网关 ID,可以显式声明,例如 capabilities = [\"thinking\", \"tool_use\"];显式声明的标签会与自动识别的结果取并集。",
          "会话中途用 /model 在已声明的别名之间切换——不用重启,也不用改配置。",
          "所有 provider 默认流式输出;如果某个网关把推理内容放在非标准字段名下返回,模型别名接受 reasoning_key 覆盖。",
        ] },
        { type: "link", text: "经核验的上下文窗口与各模型价格", href: "/models" },
      ] },
      { h2: "这些会话在预付费余额上的成本", blocks: [
        { type: "table", headers: ["要声明的模型", "官方 缓存命中 / 未命中 / 输出", "五折后在此实付"], rows: [
          ["kimi/k3 · k3-256k · k3[1m]", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
          ["kimi/kimi-for-coding", "$0.19 / $0.95 / $4", "$0.095 / $0.475 / $2"],
          ["kimi/kimi-for-coding-highspeed", "$0.38 / $1.90 / $8", "$0.19 / $0.95 / $4"],
        ] },
        { type: "p", text: "数字均为每 1M tokens。Kimi 的提示词缓存是自动的,终态 usage 会报告哪部分输入由缓存提供;推理 token 按输出计费——它们不是独立的 token 类别。apiToken.sale 对官方费率统一打五折(50% B2C 折扣),所以一节 Kimi Code 会话的成本就是实际消耗的 token 乘以官方价格的一半;闲置一周不花一分钱。" },
        { type: "p", text: "余额在这把密钥的所有别名之间共享,重度用 Claude 的一天和重度用 Kimi 的一天从同一个池子里扣。实用的护栏是每把密钥的终身累计消费上限和到期日期,外加控制面板里已结算的用量。会话中途返回 402 说明池子空了——充值后下一个请求就会成功,重试不会。" },
      ] },
      { h2: "这些故障是配置问题,不是模型质量问题", blocks: [
        { type: "list", items: [
          "任何请求发出之前就启动失败——provider 没有凭证。在 config.toml 里写 api_key(或 [providers.apitoken.env] 子表);shell export 永远不会被读取。",
          "第一轮就 401——密钥错误或已吊销,或者 base_url 丢了 /v1 后缀。用上面的 curl 目录调用复现,定位是哪一半出了问题。",
          "刚声明的模型返回 404——这个 ID 不在按密钥限定范围的目录里。信 GET /v1/models 而不是记忆,把别名钉进长期配置之前先重新查一遍。",
          "压缩比预期早得多就触发——max_context_size 声明得低于模型的真实窗口,CLI 以为空间不够了。",
          "密钥以明文存放——这是该 provider 类型的设计使然,所以 chmod 600 是安装步骤的一部分,这个文件也不该放在任何会同步或提交的目录里。",
        ] },
      ] },
    ],
    faq: [
      { q: "Kimi Code 不用 /login 能用 apiToken.sale 密钥吗?", a: "能。/login 通过 OAuth 把 CLI 绑定到 Kimi Code 托管服务;手写的 [providers] 条目用 type = \"openai\" 加 base_url https://router.apitoken.sale/v1,全程只靠 sk-pool 密钥,完全不碰那套流程。" },
      { q: "Kimi Code 会从环境变量读取 API 密钥吗?", a: "不会。凭证解析顺序是 provider 的 api_key 字段,然后是 config.toml 里的 [providers.<name>.env] 子表;两者都没有,启动直接失败。provider 凭证不会查询 shell export。" },
      { q: "一个 provider 块能在 Kimi Code 里跑 Claude、GPT 和 Gemini 吗?", a: "能。provider 持有端点和密钥;每个模型是独立的 [models] 别名,model 字段填统一目录的命名空间 ID,例如 openai/gpt-5.6-terra 或 google/gemini-3.6-flash。" },
      { q: "Kimi 模型应该声明多大的 max_context_size?", a: "K3 的 1M 模式用 1048576,Kimi for Coding 用 262144。CLI 用这个值做溢出检查和压缩时机判断,数字写小了会悄悄缩短你实际可用的会话长度。" },
      { q: "如何在 Kimi Code 会话中途切换模型?", a: "运行 /model,从 [models] 表里已声明的别名中任选一个。在 TUI 运行期间改 config.toml,执行 /reload 后生效。" },
    ],
  };
