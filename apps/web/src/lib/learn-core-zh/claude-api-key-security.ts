import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API 密钥安全：存储、轮换与泄露应急",
    h1: "保护好你的 Claude API 密钥",
    description: "apiToken.sale 上的 Claude API 密钥安全实践：密钥该存哪里、如何设置终身累计消费上限和到期日期、安全的轮换顺序，以及密钥泄露后的前十分钟该做什么。",
    keywords: ["claude api 密钥安全", "claude api key 泄露", "轮换 claude api 密钥", "吊销 claude api 密钥", "anthropic api key 最佳实践", "api 密钥 环境变量", "api key rotation", "claude api 密钥管理", "api 密钥泄露怎么办", "api key secret manager"],
    dek: "Claude API 密钥安全大多是些枯燥的基本功：让密钥远离版本库、给它的花费封顶、在真出事之前先演练一遍吊销。本文逐项讲清 apiToken.sale 给你的管控——终身累计消费上限、到期日期、按工具命名的独立密钥——外加一套可以直接照抄的存储方案和泄露应急手册。",
    sections: [
      { h2: "密钥被盗后到底能做什么", blocks: [
        { type: "p", text: "你的 apiToken.sale 密钥（形如 sk-pool-•••）是一把持有者凭据：谁出示它，谁就能用你的预付余额调用 Claude、GPT、Gemini 和 Kimi。请求时没有二次验证——持有即授权。所以目标不是让泄露绝无可能，而是让泄露变得代价低、可发现、可回滚。" },
        { type: "p", text: "预付计费已经把最坏情况封在当前余额之内，终身累计消费上限又进一步收紧了它。剩下的只是麻烦和惊吓：攻击者凌晨三点刷你的额度，或者密钥在公开仓库里躺了几个月没人发现。这两个问题，靠同样的三项管控加一次简短的轮换演练就能解决。" },
      ] },
      { h2: "发出第一个请求前就该设好的三项管控", blocks: [
        { type: "p", text: "你在控制台创建的每个密钥都支持这些设置。创建时就配置好——等泄露之后再补救就太迟了。" },
        { type: "table", headers: ["管控", "作用", "什么时候用"], rows: [
          ["终身累计消费上限", "密钥累计消费达到固定金额后硬性停用，无论谁在使用它", "每个密钥都要设——设成这个项目从头到尾该花的钱"],
          ["到期日期", "在你选定的日期自动禁用密钥", "外包人员、试用、演示，以及一切临时访问"],
          ["描述性密钥名称", "几个月后仍能告诉你这把密钥服务于哪个工具、哪个环境", "每个密钥——凌晨两点吊销密钥时你会感谢自己"],
        ] },
        { type: "p", text: "按工具和环境分别签发密钥，而不是共用一把。吊销一把泄露的 Cursor 密钥绝不应该顺带搞挂你的生产后端；控制台里一排名为 prod-backend、cursor-laptop、ci-staging 的密钥，一眼就能看清爆炸半径。" },
        { type: "link", text: "不确定上限该设多少？先用 Claude API 成本计算器给工作负载估个价。", href: "/tools/claude-api-cost-calculator" },
        { type: "note", text: "通过 Google 或 GitHub 创建的新账户可获 $5 平台奖励余额——适用于受支持的 Claude、GPT、Gemini 和 Kimi 模型；邮箱/密码账户不享受此奖励。" },
      ] },
      { h2: "经得起现实考验的存储规则", blocks: [
        { type: "p", text: "一条规则覆盖大部分场景：密钥只放在环境变量或密钥管理器里，绝不写进源码。落到实处就是：.env 文件在第一次提交之前就加进 git 忽略列表，或者用 1Password CLI、Doppler、AWS Secrets Manager 这类管理器在运行时注入变量。" },
        { type: "code", code: `# .env — commit .env.example without values, never this file\nANTHROPIC_BASE_URL=https://router.apitoken.sale\nANTHROPIC_API_KEY=sk-pool-•••\n\n# .gitignore — add this before the first commit, not after\n.env` },
        { type: "list", items: [
          "Git 历史——后续提交里删掉文件并不能抹掉密钥；一律按已泄露处理并轮换。",
          "客户端 JavaScript——任何打进浏览器应用包的东西按定义都是公开的；从你自己的后端调用 API。",
          "CI 日志——在流水线步骤里 echo 环境变量会把密钥打进构建日志；给机密加掩码，绝不打印。",
          "Shell 历史——直接敲 curl -H \"x-api-key: sk-pool-…\" 会把密钥存进历史文件；先 export 成变量再用。",
          "聊天和工单——把密钥粘进 Slack、Telegram 或 issue 跟踪器，会留下一份永久、可搜索的副本。",
        ] },
        { type: "note", text: "截图和屏幕共享也算数。如果密钥出现在录制的会议或共享的截图里，就轮换它——十分钟的代价比另一种结果便宜得多。" },
      ] },
      { h2: "不写死密钥，照样接入工具", blocks: [
        { type: "p", text: "主流客户端都会从环境读取凭据，所以没有任何东西需要写进代码：" },
        { type: "code", code: `# Anthropic SDK and Claude Code\nexport ANTHROPIC_BASE_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••\n\n# OpenAI-compatible clients (GPT models, and Claude via the same lane)\nexport OPENAI_BASE_URL=https://router.apitoken.sale/v1\nexport OPENAI_API_KEY=sk-pool-•••` },
        { type: "p", text: "Anthropic 和 OpenAI 的 SDK 会自动读取这些变量——如果你还在把密钥以字符串字面量传给构造函数，那就是要改的味道。在服务器上，从平台的机密存储注入变量（托管平台的环境变量设置、Docker secrets、systemd 的 EnvironmentFile），而不是把它烘进镜像或配置文件。" },
      ] },
      { h2: "四步完成轮换", blocks: [
        { type: "p", text: "只要练过一次，轮换就很便宜。下面这个顺序能让每个客户端全程保持已认证状态：" },
        { type: "steps", items: [
          "在控制台创建替换密钥。给它和旧密钥相同的终身累计消费上限；临时访问就加上到期日期；名称里带上日期，比如 prod-backend-2026-08。",
          "更新客户端：改掉环境变量或机密存储里的值，然后重启或重新部署，让进程真正重新加载它。",
          "在控制台观察用量，直到请求都在走新密钥、旧密钥彻底安静下来。",
          "吊销旧密钥。只有到这一步才吊销——先吊销等于亲手制造一场本可避免的 401 故障。",
        ] },
        { type: "p", text: "多久轮换一次？只有安全策略有硬性要求时才按固定周期轮换。否则在这些时机轮换：某个工具退役、外包人员离场、笔记本丢失，或者你再也说不清密钥都被粘到过哪些地方。不确定本身就是轮换的触发条件。" },
      ] },
      { h2: "泄露后的前十分钟", blocks: [
        { type: "steps", items: [
          "立即在控制台吊销暴露的密钥。这是全局的关键——新请求马上停止，而终身累计消费上限会封住你赶到之前发生的一切。",
          "打开用量视图，找你不认识的消费或模型；这能告诉你泄露从什么时候开始被利用。",
          "按上面的轮换顺序签发替换密钥并更新客户端。",
          "堵住源头：从 git 历史中清除密钥，清理 CI 日志，并轮换所有与它在同一个文件或同一条消息里出现过的其他机密。",
        ] },
        { type: "note", text: "只要密钥在公开 GitHub 仓库里出现过，哪怕只有一小会儿，就当它已经被抓走了——自动化扫描器几分钟内就会扫到新提交。“我很快就删掉了”不是缓解措施，吊销才是。" },
      ] },
    ],
    faq: [
      { q: "Claude API 密钥被盗会怎样？", a: "持有者可以花你的预付余额调用 Claude、GPT、Gemini 和 Kimi 模型，直到你吊销它或它触达终身累计消费上限。先在控制台吊销，再调查。" },
      { q: "消费上限是按天、按月还是终身？", a: "终身：它限制一把密钥总共能花多少钱，而余额本身是预付的，所以泄露的密钥永远刷不出无上限的账单。临时访问就再配一个到期日期。" },
      { q: "所有工具应该共用一把 API 密钥吗？", a: "不应该。按工具和环境分别签发名称清晰的密钥，这样吊销泄露的密钥绝不会波及无关的客户端，控制台用量也能精确显示每个工具各花了多少。" },
      { q: "如何轮换 Claude API 密钥又不搞挂应用？", a: "先创建替换密钥，更新客户端并在控制台确认新密钥已有流量，然后再吊销旧密钥。客户端还没更新就先吊销，只会把轮换变成故障。" },
      { q: "我把 API 密钥提交到 GitHub 了，删掉文件够吗？", a: "不够——密钥仍留在 git 历史里，而公开仓库几分钟内就会被扫描。先吊销并轮换密钥，再清理历史；只删文件根本算不上缓解。" },
    ],
  };
