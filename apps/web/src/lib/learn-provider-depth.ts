import type { Locale } from "./learn";

export type ProviderDepthList = Record<Locale, string[]>;

function three(en: string, ru: string, zh: string, ko: string): ProviderDepthList {
  const split = (value: string): string[] => value.split(" | ");
  const result = { en: split(en), ru: split(ru), zh: split(zh), ko: split(ko) };
  for (const [locale, items] of Object.entries(result)) {
    if (items.length !== 3) throw new Error(`Provider depth copy must have three ${locale} items`);
  }
  return result;
}

/**
 * Topic-specific failure signals used by the provider parity guides. These are
 * deliberately editorial data, not a generic SEO paragraph: every topic names
 * the concrete mistake that invalidates its decision or rollout.
 */
export const PROVIDER_TOPIC_RISKS: Record<string, ProviderDepthList> = {
  buy: three(
    "Treating the key as a login to {direct} instead of an independent apiToken.sale credential. | Sending traffic before the prepaid balance is credited and the live catalog answers. | Copying the wrong authorization header or an unlisted model ID into the first request.",
    "Считать ключ логином в {direct}, хотя это отдельный credential apiToken.sale. | Отправлять трафик до зачисления баланса и ответа live catalog. | Скопировать в первый запрос неверный auth header или model ID вне каталога.",
    "把密钥当成 {direct} 登录，而不是独立的 apiToken.sale 凭据。 | 预付余额到账且实时目录可用之前就发送流量。 | 首次请求使用错误鉴权 header 或目录之外的 model ID。",
    "key를 독립 apiToken.sale credential이 아니라 {direct} login으로 취급합니다. | 선불 잔액 반영과 live catalog 응답 전에 traffic을 보냅니다. | 첫 요청에 잘못된 auth header나 catalog에 없는 model ID를 사용합니다.",
  ),
  quickstart: three(
    "Appending /v1 or /v1beta twice because the SDK and configured base URL both add a path prefix. | Testing an old marketing alias instead of a model returned for the same key. | Declaring success from HTTP 2xx without checking non-empty output and terminal usage.",
    "Дважды добавить /v1 или /v1beta, потому что prefix задают и SDK, и base URL. | Проверять старый marketing alias вместо модели, возвращённой тем же ключом. | Объявить успех по HTTP 2xx, не проверив непустой output и terminal usage.",
    "SDK 与 base URL 都追加路径，导致 /v1 或 /v1beta 重复。 | 测试旧营销别名，而不是同一密钥实际返回的模型。 | 只看到 HTTP 2xx 就宣布成功，未检查非空输出和终态 usage。",
    "SDK와 base URL이 모두 path를 붙여 /v1 또는 /v1beta가 중복됩니다. | 같은 key가 반환한 model 대신 오래된 marketing alias를 테스트합니다. | 비어 있지 않은 output과 terminal usage 없이 HTTP 2xx만으로 성공을 선언합니다.",
  ),
  pricing: three(
    "Comparing only the cheapest input headline while omitting cached input, output and model-specific legs. | Estimating the charge from characters instead of authoritative terminal usage. | Assuming a rate card is an availability promise without checking the key-scoped catalog.",
    "Сравнивать только минимальную input-ставку, забывая cached input, output и model-specific legs. | Оценивать списание по символам вместо authoritative terminal usage. | Считать rate card обещанием доступности без проверки key-scoped catalog.",
    "只比较最低 input 报价，遗漏 cached input、output 与模型特定分类。 | 按字符估算费用，而不是使用权威终态 usage。 | 不检查密钥范围目录，就把价目表当作可用性承诺。",
    "가장 싼 input 가격만 비교하고 cached input, output, model-specific leg를 누락합니다. | authoritative terminal usage 대신 문자 수로 charge를 추정합니다. | key-scoped catalog 확인 없이 rate card를 availability 약속으로 봅니다.",
  ),
  "model-comparison": three(
    "Choosing the flagship for every request without a task-level quality gain. | Comparing model family labels while ignoring exact catalog IDs, controls and latency. | Measuring price per call without counting retries, escalations and failed validations.",
    "Выбирать flagship для каждого запроса без прироста качества по классам задач. | Сравнивать названия family, игнорируя точные catalog IDs, controls и latency. | Считать цену вызова без retries, escalations и failed validations.",
    "所有请求都选旗舰，却没有任务级质量提升。 | 只比较模型系列名称，忽略准确目录 ID、控制项与延迟。 | 只算单次调用价格，遗漏重试、升级与验证失败。",
    "task별 품질 향상 없이 모든 요청에 flagship을 선택합니다. | 정확한 catalog ID, control, latency를 빼고 model family 이름만 비교합니다. | retry, escalation, validation 실패를 제외한 호출 가격만 측정합니다.",
  ),
  cheapest: three(
    "Routing every request to {flagship} before an eval proves that the quality gain pays for it. | Comparing only input price while omitting cached-input and output charges. | Leaving output or agent loops unbounded, so a low token rate hides a high total bill.",
    "Отправлять все запросы в {flagship} до eval, который докажет окупаемость качества. | Сравнивать только input price, не учитывая cached-input и output. | Не ограничивать output или agent loops, из-за чего низкая ставка скрывает большой счёт.",
    "评测尚未证明质量收益值得付费，就把所有请求路由到 {flagship}。 | 只比较 input 价格，遗漏 cached-input 与 output 费用。 | 不限制 output 或 agent loop，让低单价掩盖高总账单。",
    "eval로 품질 이득의 가치가 확인되기 전에 모든 요청을 {flagship}으로 보냅니다. | cached-input과 output charge를 빼고 input 가격만 비교합니다. | output이나 agent loop를 제한하지 않아 낮은 단가가 큰 총액을 숨깁니다.",
  ),
  "restricted-regions": three(
    "Presenting a payment route as a way to bypass law, sanctions or local network policy. | Treating a checkout receipt as proof that usable API balance already exists. | Promising a static country or model list instead of checking lawful access and {catalog}.",
    "Выдавать платёжный маршрут за способ обойти законы, санкции или local network policy. | Считать checkout receipt доказательством уже доступного API-баланса. | Обещать статичный список стран или моделей вместо проверки законного доступа и {catalog}.",
    "把付款路径描述成绕过法律、制裁或本地网络政策的方法。 | 把结账收据当成 API 可用余额已经到账的证明。 | 承诺固定国家或模型列表，而不检查合法访问与 {catalog}。",
    "결제 경로를 법률, 제재, local network policy 우회 수단으로 설명합니다. | checkout 영수증을 사용 가능한 API 잔액의 증거로 봅니다. | 합법적 접근과 {catalog} 확인 없이 고정 국가·모델 목록을 약속합니다.",
  ),
  "crypto-payment": three(
    "Using the wrong asset, network or destination instead of the checkout processor's exact instructions. | Starting production after a blockchain transfer but before the dashboard balance is credited. | Pasting the model API key into a wallet, exchange or payment-support message.",
    "Использовать неверные asset, network или адрес вместо точных инструкций checkout processor. | Запускать production после транзакции, но до зачисления баланса в дашборде. | Вставлять model API key в кошелёк, биржу или сообщение платёжной поддержке.",
    "未按结账处理商的准确说明选择资产、网络或收款地址。 | 链上转账后但仪表板余额尚未到账就启动生产流量。 | 把模型 API 密钥粘贴到钱包、交易所或付款支持消息中。",
    "checkout processor 지침과 다른 asset, network, 주소를 사용합니다. | blockchain 전송 후 dashboard 잔액 반영 전에 production을 시작합니다. | model API key를 wallet, exchange, 결제 지원 메시지에 붙여넣습니다.",
  ),
  "no-waitlist": three(
    "Reading immediate key issuance as a permanent promise that every model is enabled. | Skipping {catalog} and deploying an ID copied from an old article or screenshot. | Retrying 402 balance failures as if they were 404 model-availability failures.",
    "Понимать мгновенную выдачу ключа как вечное обещание доступности любой модели. | Пропускать {catalog} и брать ID из старой статьи или screenshot. | Повторять 402 balance failures так, будто это 404 model-availability failures.",
    "把即时签发密钥理解为所有模型永久启用的承诺。 | 跳过 {catalog}，部署从旧文章或截图复制的 ID。 | 把 402 余额错误当作 404 模型不可用错误进行重试。",
    "즉시 key 발급을 모든 모델의 영구 활성화 약속으로 해석합니다. | {catalog}를 건너뛰고 오래된 글이나 screenshot의 ID를 배포합니다. | 402 잔액 실패를 404 모델 가용성 실패처럼 retry합니다.",
  ),
  "free-key": three(
    "Claiming that every signup receives $5 when the bonus is limited to eligible Google/GitHub registrations. | Describing finite platform credit as an unlimited vendor free tier. | Testing autonomous workloads before setting a small lifetime spending limit on the key.",
    "Обещать $5 любой регистрации, хотя бонус доступен только подходящим Google/GitHub аккаунтам. | Называть конечный platform credit безлимитным vendor free tier. | Тестировать autonomous workload до небольшого lifetime spending limit ключа.",
    "声称所有注册都获得 $5，而奖励仅适用于符合条件的 Google/GitHub 注册。 | 把有限平台余额描述成无限厂商免费层。 | 尚未设置较小的密钥终身消费上限就测试自主工作负载。",
    "모든 가입에 $5를 약속하지만 bonus는 조건에 맞는 Google/GitHub 가입만 대상입니다. | 유한 platform credit을 무제한 vendor free tier로 설명합니다. | 작은 key lifetime spending limit 설정 전에 autonomous workload를 테스트합니다.",
  ),
  "free-trial": three(
    "Judging model quality from one prompt instead of proving authentication, discovery, generation and billing separately. | Spending trial credit on a stale model ID before the free catalog check. | Scaling prompts before terminal usage and the dashboard ledger agree on the first request.",
    "Судить о качестве по одному prompt вместо отдельных проверок auth, discovery, generation и billing. | Тратить trial credit на устаревший model ID до бесплатной проверки catalog. | Увеличивать prompts до совпадения terminal usage и dashboard ledger первого запроса.",
    "用一个 prompt 判断模型质量，而不是分别验证鉴权、发现、生成和结算。 | 未先免费检查目录，就把试用余额花在过期 model ID 上。 | 首次请求的终态 usage 与仪表板账本尚未一致就扩大 prompt。",
    "auth, discovery, generation, billing을 따로 입증하지 않고 한 prompt로 모델 품질을 판단합니다. | 무료 catalog 확인 전에 오래된 model ID에 trial credit을 씁니다. | 첫 요청 terminal usage와 dashboard ledger가 맞기 전에 prompt를 확장합니다.",
  ),
  "cli-without-subscription": three(
    "Mixing a direct vendor login and an API-key profile, leaving the active payer ambiguous. | Letting the CLI replace the pinned catalog model with its vendor default. | Saving the raw key in shell history, repository config or a world-readable profile.",
    "Смешивать direct vendor login и API-key profile, оставляя active payer неясным. | Позволять CLI заменять закреплённую catalog model на vendor default. | Сохранять raw key в shell history, repo config или world-readable profile.",
    "混用厂商直接登录与 API-key profile，导致当前付款方不明确。 | 允许 CLI 用厂商默认模型替换已固定的目录模型。 | 把原始密钥保存到 shell history、仓库配置或全局可读 profile。",
    "direct vendor login과 API-key profile을 섞어 active payer를 모호하게 만듭니다. | CLI가 고정 catalog model을 vendor default로 바꾸게 둡니다. | raw key를 shell history, repository config, world-readable profile에 저장합니다.",
  ),
  "flagship-model": three(
    "Making {flagship} the global default before it beats {balanced} on a fixed eval. | Using a vendor UI alias that is absent from the key-scoped router catalog. | Comparing quality without the settled cost, output cap and latency of the same cases.",
    "Делать {flagship} global default до победы над {balanced} на фиксированном eval. | Использовать alias из vendor UI, которого нет в key-scoped router catalog. | Сравнивать quality без settled cost, output cap и latency тех же кейсов.",
    "在固定评测中尚未优于 {balanced}，就把 {flagship} 设为全局默认。 | 使用密钥范围路由目录中不存在的厂商 UI 别名。 | 比较质量时未同时统计相同案例的结算成本、output 上限与延迟。",
    "고정 eval에서 {balanced}를 이기기 전에 {flagship}을 global default로 둡니다. | key-scoped router catalog에 없는 vendor UI alias를 사용합니다. | 같은 case의 settled cost, output cap, latency 없이 품질만 비교합니다.",
  ),
  "balanced-model": three(
    "Using {balanced} for every task without explicit down-routing and escalation rules. | Looking only at an aggregate quality score that hides high-risk task failures. | Allowing a moving default alias to change production behavior without a canary.",
    "Использовать {balanced} для всего без явных down-routing и escalation rules. | Смотреть только на aggregate quality, скрывающий ошибки high-risk задач. | Позволять moving default alias менять production без canary.",
    "所有任务都使用 {balanced}，却没有明确下沉与升级规则。 | 只看汇总质量分数，掩盖高风险任务失败。 | 允许变化中的默认别名未经 canary 就改变生产行为。",
    "명시적 down-routing과 escalation rule 없이 모든 작업에 {balanced}를 사용합니다. | high-risk task 실패를 숨기는 aggregate quality만 봅니다. | moving default alias가 canary 없이 production 동작을 바꾸게 둡니다.",
  ),
  "fast-model": three(
    "Sending ambiguous or high-risk work to {fast} without local schema and quality validation. | Retrying failed fast-tier validations until total cost exceeds one {balanced} call. | Calling the tier cheaper without measuring its actual price, retries and escalation frequency.",
    "Отправлять ambiguous/high-risk работу в {fast} без local schema и quality validation. | Повторять failed fast-tier validations, пока их цена не превысит один вызов {balanced}. | Называть tier дешёвым без измерения его цены, retries и escalation frequency.",
    "把模糊或高风险工作交给 {fast}，却没有本地 schema 与质量验证。 | 反复重试快速层验证失败，直到总成本超过一次 {balanced} 调用。 | 未测量实际价格、重试与升级频率就称该层级更便宜。",
    "local schema와 quality validation 없이 모호하거나 high-risk한 작업을 {fast}로 보냅니다. | fast-tier validation 실패를 반복해 총비용이 {balanced} 한 번보다 커집니다. | 실제 가격, retry, escalation 빈도 측정 없이 더 싼 tier라 부릅니다.",
  ),
  cursor: three(
    "Selecting a bundled vendor preset instead of Cursor's custom OpenAI-compatible route. | Entering a marketing model name rather than the namespaced ID returned by {catalog}. | Leaving unsupported provider controls enabled and treating the resulting 400 as an outage.",
    "Выбрать bundled vendor preset вместо custom OpenAI-compatible route Cursor. | Ввести marketing model name вместо namespaced ID из {catalog}. | Оставить unsupported provider controls и принять полученный 400 за outage.",
    "选择编辑器内置厂商 preset，而不是 Cursor 的自定义 OpenAI-compatible 路由。 | 输入营销模型名，而不是 {catalog} 返回的命名空间 ID。 | 保留不支持的提供商控制项，并把由此产生的 400 当成服务中断。",
    "Cursor custom OpenAI-compatible route 대신 bundled vendor preset을 선택합니다. | {catalog}가 반환한 namespaced ID 대신 marketing model name을 입력합니다. | unsupported provider control을 남겨 발생한 400을 outage로 봅니다.",
  ),
  vscode: three(
    "Committing the key in workspace JSON instead of the extension secret store or environment. | Enabling autonomous shell or browser tools before one bounded chat and tool-call smoke test. | Using a Claude-only native adapter for {provider} when the universal route is required.",
    "Закоммитить ключ в workspace JSON вместо secret store расширения или env. | Включить autonomous shell/browser tools до bounded chat и tool-call smoke test. | Использовать Claude-only native adapter для {provider}, когда нужен universal route.",
    "把密钥提交到 workspace JSON，而不是扩展 secret store 或环境变量。 | 尚未完成有界聊天与 tool-call smoke test 就启用自主 shell 或浏览器工具。 | {provider} 需要通用路由时仍使用 Claude-only 原生适配器。",
    "extension secret store나 env 대신 workspace JSON에 key를 commit합니다. | bounded chat과 tool-call smoke test 전에 autonomous shell/browser tool을 켭니다. | universal route가 필요한 {provider}에 Claude-only native adapter를 사용합니다.",
  ),
  "cursor-no-direct": three(
    "Assuming a custom model key unlocks unrelated paid Cursor editor features. | Letting Cursor silently choose its bundled route instead of https://router.apitoken.sale/v1. | Keeping hidden direct-vendor login state that makes the payer impossible to audit.",
    "Считать, что custom model key открывает несвязанные платные функции Cursor. | Позволять Cursor скрытно выбрать bundled route вместо https://router.apitoken.sale/v1. | Оставлять hidden direct-vendor login, из-за которого payer нельзя проверить.",
    "认为自定义模型密钥会解锁无关的 Cursor 付费编辑器功能。 | 允许 Cursor 静默选择内置路由，而不是 https://router.apitoken.sale/v1。 | 保留隐藏厂商直登状态，导致付款方无法审计。",
    "custom model key가 관련 없는 Cursor 유료 기능을 연다고 가정합니다. | Cursor가 https://router.apitoken.sale/v1 대신 bundled route를 조용히 선택하게 둡니다. | payer 감사가 불가능한 hidden direct-vendor login 상태를 남깁니다.",
  ),
  sdk: three(
    "Duplicating /v1 or /v1beta when the SDK appends a prefix to an already versioned base URL. | Mixing native request types with the universal OpenAI-compatible lane and losing protocol guarantees. | Retrying a streamed non-idempotent turn after the first response byte.",
    "Дублировать /v1 или /v1beta, когда SDK добавляет prefix к versioned base URL. | Смешивать native request types с universal OpenAI lane и терять protocol guarantees. | Retry streamed non-idempotent turn после первого response byte.",
    "SDK 在已带版本的 base URL 后再次追加前缀，造成 /v1 或 /v1beta 重复。 | 把原生请求类型与通用 OpenAI-compatible 通道混用，丢失协议保证。 | 收到首个响应字节后重试非幂等流式 turn。",
    "SDK가 이미 versioned base URL에 prefix를 붙여 /v1 또는 /v1beta가 중복됩니다. | native request type과 universal OpenAI-compatible lane을 섞어 protocol guarantee를 잃습니다. | 첫 response byte 이후 streamed non-idempotent turn을 retry합니다.",
  ),
  langchain: three(
    "Using a wrapper that drops terminal usage and then estimating cost from characters. | Applying a second vendor prefix to an already namespaced catalog model. | Allowing automatic retries or fallback to hide which route produced the answer and charge.",
    "Использовать wrapper, который теряет terminal usage, а затем считать цену по символам. | Добавлять второй vendor prefix к уже namespaced catalog model. | Позволять auto retries/fallback скрывать route ответа и списания.",
    "包装器丢弃终态 usage，随后按字符估算成本。 | 对已有命名空间的目录模型再次添加厂商前缀。 | 让自动重试或 fallback 隐藏实际生成答案并扣费的路由。",
    "terminal usage를 버리는 wrapper를 쓰고 문자 수로 비용을 추정합니다. | 이미 namespaced catalog model에 vendor prefix를 다시 붙입니다. | auto retry/fallback이 답과 charge를 만든 route를 숨기게 둡니다.",
  ),
  litellm: three(
    "Prefixing the provider twice and sending a model ID that is absent from the unified catalog. | Leaving fallback enabled while validating one model, so errors and usage lose attribution. | Letting both LiteLLM and the client retry the same delivered turn.",
    "Дважды добавить provider prefix и отправить model ID вне unified catalog. | Оставить fallback при проверке одной модели, потеряв attribution errors и usage. | Разрешить LiteLLM и клиенту retry один и тот же delivered turn.",
    "重复添加提供商前缀，发送统一目录中不存在的 model ID。 | 验证单一模型时仍启用 fallback，使错误与 usage 无法归因。 | LiteLLM 与客户端同时重试同一个已交付 turn。",
    "provider prefix를 두 번 붙여 unified catalog에 없는 model ID를 보냅니다. | 한 모델 검증 중 fallback을 켜 error와 usage attribution을 잃습니다. | LiteLLM과 client가 같은 delivered turn을 모두 retry합니다.",
  ),
  aider: three(
    "Starting an autonomous edit loop on a dirty working tree with no bounded rollback point. | Configuring editor or weak models before both IDs appear in {catalog}. | Running edit/test retries with no turn, output or lifetime-key spending cap.",
    "Запускать autonomous edit loop на dirty worktree без bounded rollback point. | Настраивать editor/weak models до появления обоих IDs в {catalog}. | Выполнять edit/test retries без turn, output и lifetime spending cap ключа.",
    "在 dirty working tree 上启动自主编辑循环，没有有界回滚点。 | editor/weak model 两个 ID 尚未都出现在 {catalog} 就配置。 | 编辑/测试重试没有 turn、output 或密钥终身消费上限。",
    "bounded rollback point 없이 dirty working tree에서 autonomous edit loop를 시작합니다. | 두 ID가 {catalog}에 나오기 전에 editor/weak model을 설정합니다. | turn, output, key lifetime spending cap 없이 edit/test retry를 실행합니다.",
  ),
  "roo-code": three(
    "Saving the API key in a plain workspace file instead of Roo Code's protected secret field. | Enabling browser or shell tools before conservative context and output limits are proven. | Responding to unsupported_parameter by switching to a hidden provider preset.",
    "Хранить API key в plain workspace file вместо protected secret field Roo Code. | Включать browser/shell tools до проверки консервативных context/output limits. | На unsupported_parameter переходить в hidden provider preset.",
    "把 API 密钥保存在普通 workspace 文件，而不是 Roo Code 受保护 secret 字段。 | 保守 context 与 output 上限尚未验证就启用浏览器或 shell 工具。 | 遇到 unsupported_parameter 就切换到隐藏提供商 preset。",
    "Roo Code protected secret field 대신 plain workspace file에 API key를 저장합니다. | 보수적 context/output limit 검증 전에 browser/shell tool을 켭니다. | unsupported_parameter에 hidden provider preset 전환으로 대응합니다.",
  ),
  "vscode-agents": three(
    "Sharing one unbounded key across every project and extension. | Routing every agent turn to {flagship}, including deterministic parsing steps. | Enabling autonomous tools without maximum turns, tool calls, wall time and output.",
    "Использовать один unbounded key во всех проектах и extensions. | Отправлять каждый agent turn в {flagship}, включая deterministic parsing. | Включать autonomous tools без max turns, tool calls, wall time и output.",
    "所有项目与扩展共享一个无上限密钥。 | 每个 agent turn 都路由到 {flagship}，包括确定性解析步骤。 | 未设置最大 turn、tool call、wall time 与 output 就启用自主工具。",
    "모든 project와 extension에서 하나의 unbounded key를 공유합니다. | deterministic parsing까지 모든 agent turn을 {flagship}으로 보냅니다. | max turn, tool call, wall time, output 없이 autonomous tool을 켭니다.",
  ),
  "direct-provider": three(
    "Choosing from one headline price while ignoring credentials, protocol, catalog and support ownership. | Comparing different model IDs or feature sets and calling the result price parity. | Ignoring direct enterprise contracts or native-only features that the live gateway catalog does not claim.",
    "Выбирать по одной цене, игнорируя ownership credentials, protocol, catalog и support. | Сравнивать разные model IDs/features и называть результат price parity. | Игнорировать direct enterprise contracts или native-only features вне live gateway catalog.",
    "只按一个报价选择，忽略凭据、协议、目录与支持责任。 | 比较不同 model ID 或功能集，却称为价格一致性。 | 忽略实时网关目录未宣称支持的直连企业合同或原生专属功能。",
    "credential, protocol, catalog, support ownership을 빼고 headline 가격 하나로 선택합니다. | 다른 model ID나 feature set을 비교하고 price parity라 부릅니다. | live gateway catalog에 없는 direct enterprise 계약이나 native-only feature를 무시합니다.",
  ),
  openrouter: three(
    "Treating the same family name as proof of identical model ID, controls and routing. | Skipping streaming, tool-call and error-shape tests where abstraction differences appear. | Applying marketplace-normalized assumptions to the native {protocol} lane.",
    "Считать одно family name доказательством одинаковых model ID, controls и routing. | Пропускать streaming, tool-call и error-shape tests, где видна абстракция. | Переносить marketplace-normalized assumptions на native {protocol} lane.",
    "把相同系列名称当作 model ID、控制项与路由完全相同的证明。 | 跳过最能暴露抽象差异的 streaming、tool-call 与错误结构测试。 | 把市场归一化假设套用到原生 {protocol} 通道。",
    "같은 family name을 동일 model ID, control, routing의 증거로 봅니다. | abstraction 차이가 드러나는 streaming, tool-call, error-shape test를 생략합니다. | marketplace-normalized 가정을 native {protocol} lane에 적용합니다.",
  ),
  proxyapi: three(
    "Accepting estimated usage without proving the terminal provider counters and final ledger charge. | Leaving ownership of the customer key, upstream account or balance undocumented. | Moving production before a small paid request exercises support and refund evidence.",
    "Принимать estimated usage без terminal provider counters и final ledger charge. | Не документировать ownership customer key, upstream account или balance. | Переносить production до small paid request, проверяющего support/refund evidence.",
    "未证明提供商终态计数与最终账本扣费，就接受估算 usage。 | 未说明客户密钥、upstream 账户或余额由谁负责。 | 尚未用小额付费请求验证支持与退款证据就迁移生产流量。",
    "terminal provider counter와 final ledger charge 입증 없이 estimated usage를 받아들입니다. | customer key, upstream account, balance ownership을 문서화하지 않습니다. | support/refund evidence를 작은 유료 요청으로 확인하기 전에 production을 옮깁니다.",
  ),
  portkey: three(
    "Comparing an observability layer with a funded API endpoint as if they were the same product category. | Giving both layers ownership of retries or fallback and duplicating recovery attempts. | Losing one request ID across layers, making usage and incident reconciliation ambiguous.",
    "Сравнивать observability layer и funded API endpoint как одну product category. | Отдать retries/fallback обоим слоям и дублировать recovery attempts. | Потерять единый request ID между слоями и сделать reconciliation неоднозначной.",
    "把可观测层与含资金 API endpoint 当成同一产品类别比较。 | 两层都负责重试或 fallback，造成重复恢复尝试。 | 跨层丢失统一 request ID，使 usage 与事件对账不清楚。",
    "observability layer와 funded API endpoint를 같은 product category처럼 비교합니다. | 두 계층 모두 retry/fallback을 소유해 recovery attempt가 중복됩니다. | 계층 간 하나의 request ID를 잃어 usage와 incident reconciliation이 모호해집니다.",
  ),
  "litellm-proxy": three(
    "Comparing only token rates while omitting proxy operations, provider funding and secret rotation. | Running duplicate routing and retry policies in LiteLLM and the downstream gateway. | Rewriting namespaced model IDs or dropping terminal usage before it reaches the ledger.",
    "Сравнивать только token rates без proxy operations, provider funding и secret rotation. | Дублировать routing/retry policies в LiteLLM и downstream gateway. | Переписывать namespaced model IDs или терять terminal usage до ledger.",
    "只比较 token 费率，遗漏代理运维、提供商资金与 secret 轮换。 | LiteLLM 与下游网关重复执行路由和重试策略。 | 重写命名空间 model ID，或在到账本前丢失终态 usage。",
    "proxy 운영, provider funding, secret rotation을 빼고 token rate만 비교합니다. | LiteLLM과 downstream gateway에 routing/retry policy를 중복합니다. | namespaced model ID를 바꾸거나 ledger 전에 terminal usage를 잃습니다.",
  ),
  "save-tokens": three(
    "Removing context without a task-quality regression test. | Keeping stale conversation turns or repeated tool output in every call. | Counting a cheap model as a saving while retries and escalation raise total settled spend.",
    "Удалять context без regression test качества задачи. | Передавать stale turns или повторный tool output в каждом вызове. | Считать дешёвую модель экономией, когда retries/escalation увеличивают settled spend.",
    "删除上下文却没有任务质量回归测试。 | 每次调用都保留过期对话 turn 或重复 tool output。 | 低价模型因重试和升级提高总结算费用，却仍被算作节省。",
    "task 품질 regression test 없이 context를 제거합니다. | 매 호출에 stale turn이나 반복 tool output을 유지합니다. | retry/escalation으로 총 settled spend가 늘어도 싼 모델을 절감으로 계산합니다.",
  ),
  billing: three(
    "Estimating the bill from characters or preflight tokens instead of authoritative terminal usage. | Omitting cached-input, output or model-specific usage legs from reconciliation. | Inferring whether a failed transport was charged from HTTP status alone.",
    "Оценивать счёт по символам или preflight tokens вместо authoritative terminal usage. | Пропускать cached-input, output или model-specific usage legs при сверке. | Определять списание failed transport только по HTTP status.",
    "按字符或预检 token 估算账单，而不是使用权威终态 usage。 | 对账时遗漏 cached-input、output 或模型特定 usage 分类。 | 只凭 HTTP 状态判断传输失败是否扣费。",
    "authoritative terminal usage 대신 문자나 preflight token으로 bill을 추정합니다. | reconciliation에서 cached-input, output, model-specific usage leg를 누락합니다. | HTTP status만으로 failed transport charge 여부를 추론합니다.",
  ),
  activation: three(
    "Promising an exact payment-confirmation time that the selected payment rail controls. | Treating a receipt as activation before usable balance appears. | Configuring a long-running client before {catalog} and one low-cap generation succeed.",
    "Обещать точное payment-confirmation time, которое зависит от payment rail. | Считать receipt активацией до появления usable balance. | Настраивать long-running client до {catalog} и успешной low-cap generation.",
    "承诺由付款通道决定的准确确认时间。 | 可用余额尚未出现，就把收据当成激活。 | {catalog} 与一次低上限生成尚未成功就配置长时间运行客户端。",
    "payment rail이 결정하는 정확한 payment-confirmation 시간을 약속합니다. | usable balance 전에 receipt를 activation으로 봅니다. | {catalog}와 low-cap generation 성공 전에 장기 client를 설정합니다.",
  ),
  countries: three(
    "Copying {company}'s direct-country list onto an independent gateway with different account boundaries. | Claiming that gateway access overrides law, sanctions or local network rules. | Publishing a permanent model-availability list instead of checking {catalog} before deployment.",
    "Переносить direct-country list {company} на independent gateway с другими account boundaries. | Утверждать, что gateway отменяет law, sanctions или local network rules. | Публиковать вечный model list вместо проверки {catalog} перед deployment.",
    "把 {company} 直连国家列表套用到账号边界不同的独立网关。 | 声称网关访问可覆盖法律、制裁或本地网络规则。 | 发布永久模型可用列表，而不是部署前检查 {catalog}。",
    "account boundary가 다른 independent gateway에 {company} direct-country list를 복사합니다. | gateway access가 법률, 제재, local network rule을 무효화한다고 주장합니다. | 배포 전 {catalog} 대신 영구 model availability 목록을 게시합니다.",
  ),
  refund: three(
    "Sending the full API key to support instead of a masked key and request IDs. | Starting a chargeback before support can reconcile the original payment and usage ledger. | Describing already settled provider usage as unused refundable balance.",
    "Отправлять support полный API key вместо masked key и request IDs. | Открывать chargeback до сверки original payment и usage ledger. | Называть уже settled provider usage неиспользованным refundable balance.",
    "向支持发送完整 API 密钥，而不是 masked key 与 request ID。 | 支持尚未核对原付款与 usage 账本就发起拒付。 | 把已结算的提供商 usage 描述成未使用可退款余额。",
    "masked key와 request ID 대신 전체 API key를 support에 보냅니다. | support가 original payment와 usage ledger를 대조하기 전에 chargeback을 시작합니다. | 이미 settled provider usage를 미사용 refundable balance라 설명합니다.",
  ),
  "best-coding-model": three(
    "Choosing from a public benchmark without a repository-level eval. | Comparing first-pass quality while omitting edit retries, test failures and latency. | Using one model globally instead of routing routine edits, hard reviews and deterministic substeps.",
    "Выбирать по public benchmark без repository-level eval. | Сравнивать first-pass quality без edit retries, test failures и latency. | Использовать одну модель вместо routing routine edits, hard reviews и deterministic substeps.",
    "依据公开 benchmark 选择，却没有仓库级评测。 | 比较首轮质量时遗漏编辑重试、测试失败与延迟。 | 全局只用一个模型，不区分常规编辑、困难审查与确定性子步骤。",
    "repository-level eval 없이 public benchmark로 선택합니다. | edit retry, test failure, latency를 빼고 first-pass 품질만 비교합니다. | routine edit, hard review, deterministic substep을 routing하지 않고 한 모델을 전역 사용합니다.",
  ),
  "subscription-vs-api": three(
    "Treating a consumer subscription and metered API as interchangeable entitlements. | Expecting an API key to unlock unrelated app or editor subscription features. | Comparing a monthly fee with API spend without a measured workload and usage ledger.",
    "Считать consumer subscription и metered API взаимозаменяемыми entitlements. | Ожидать, что API key откроет несвязанные app/editor subscription features. | Сравнивать monthly fee и API spend без measured workload и usage ledger.",
    "把消费者订阅与按量 API 当成可互换权益。 | 期望 API 密钥解锁无关应用或编辑器订阅功能。 | 没有测量工作负载与 usage 账本就比较月费和 API 支出。",
    "consumer subscription과 metered API를 교환 가능한 entitlement로 봅니다. | API key가 관련 없는 app/editor subscription feature를 연다고 기대합니다. | measured workload와 usage ledger 없이 monthly fee와 API spend를 비교합니다.",
  ),
  "generation-comparison": three(
    "Migrating because of a version label without a fixed before-and-after eval. | Reusing old prompts and tool schemas without checking new output and control behavior. | Replacing the production default globally instead of canarying a pinned exact model ID.",
    "Мигрировать из-за version label без fixed before/after eval. | Использовать старые prompts/tool schemas без проверки нового output и controls. | Менять production default глобально вместо canary exact model ID.",
    "仅因版本标签就迁移，没有固定的前后评测。 | 沿用旧 prompt 与 tool schema，未检查新的输出与控制行为。 | 直接全局替换生产默认，而不是对固定准确 model ID 做 canary。",
    "fixed before/after eval 없이 version label만 보고 migration합니다. | 새 output/control 동작 확인 없이 오래된 prompt/tool schema를 재사용합니다. | pinned exact model ID canary 대신 production default를 전역 교체합니다.",
  ),
  "why-apitoken": three(
    "Using vague convenience claims without a reproducible protocol, catalog and billing check. | Omitting the gateway's independent-account boundary and live-availability limitations. | Moving production before one request's model, terminal usage and charge reconcile.",
    "Использовать vague convenience claims без protocol, catalog и billing check. | Не указывать independent-account boundary и live-availability limitations gateway. | Переносить production до сверки model, terminal usage и charge одного запроса.",
    "只做模糊便利性宣传，没有可复现的协议、目录与结算检查。 | 省略网关独立账号边界与实时可用性限制。 | 单次请求的模型、终态 usage 与扣费尚未对账就迁移生产流量。",
    "재현 가능한 protocol, catalog, billing check 없이 모호한 편의성 주장을 사용합니다. | gateway independent-account boundary와 live-availability limitation을 누락합니다. | 한 요청의 model, terminal usage, charge 대조 전에 production을 옮깁니다.",
  ),
  gateway: three(
    "Assuming a normalized endpoint makes every provider control semantically identical. | Silently switching model or provider when a pinned route rejects a request. | Letting both the client and gateway own retries after delivery begins.",
    "Считать, что normalized endpoint делает controls всех providers одинаковыми. | Скрытно менять model/provider при отказе pinned route. | Отдать retries после delivery и client, и gateway одновременно.",
    "认为归一化 endpoint 会让所有提供商控制项语义完全相同。 | 固定路由拒绝请求时静默切换模型或提供商。 | 交付开始后让客户端与网关同时负责重试。",
    "normalized endpoint가 모든 provider control을 같은 의미로 만든다고 가정합니다. | pinned route가 거부하면 model/provider를 조용히 바꿉니다. | delivery 시작 후 client와 gateway가 모두 retry를 소유합니다.",
  ),
  "rate-limits": three(
    "Publishing one permanent numeric cap when pool capacity and upstream availability can change. | Retrying 429 indefinitely without jitter, a deadline or an attempt budget. | Confusing key spending limits and balance checks with throughput capacity.",
    "Публиковать вечный numeric cap при изменяемых pool capacity и upstream availability. | Бесконечно retry 429 без jitter, deadline и attempt budget. | Путать key spending limits/balance checks с throughput capacity.",
    "池容量与 upstream 可用性会变化，却发布永久固定数值上限。 | 没有 jitter、deadline 或尝试预算就无限重试 429。 | 把密钥消费上限和余额检查误认为吞吐容量。",
    "pool capacity와 upstream availability가 바뀌는데 영구 numeric cap을 게시합니다. | jitter, deadline, attempt budget 없이 429를 무한 retry합니다. | key spending limit/balance check를 throughput capacity와 혼동합니다.",
  ),
  streaming: three(
    "Parsing arbitrary network chunks as complete JSON instead of following protocol events. | Estimating the final bill from partial text before terminal usage arrives. | Advertising Kimi chunk incrementality as proven while it remains a preview capability under live validation.",
    "Парсить произвольные network chunks как complete JSON вместо protocol events. | Оценивать final bill по partial text до terminal usage. | Называть Kimi chunk incrementality доказанной, пока это preview на live validation.",
    "把任意网络 chunk 当作完整 JSON，而不是按协议事件解析。 | 终态 usage 到达前按部分文本估算最终账单。 | Kimi chunk 增量性仍在实时验证 preview 阶段，却宣传为已证实能力。",
    "protocol event 대신 임의 network chunk를 complete JSON으로 parsing합니다. | terminal usage 전에 partial text로 final bill을 추정합니다. | live validation 중인 preview인데 Kimi chunk incrementality를 검증됐다고 광고합니다.",
  ),
  "prompt-caching": three(
    "Placing volatile user data before the reusable prefix and expecting stable cache hits. | Inventing a cache TTL or cache-write price that is absent from the live provider contract. | Claiming savings without a cold/warm comparison of cache usage, latency, quality and charge.",
    "Ставить volatile user data перед reusable prefix и ждать стабильных cache hits. | Придумывать cache TTL или cache-write price вне live provider contract. | Объявлять экономию без cold/warm сравнения usage, latency, quality и charge.",
    "把易变用户数据放在可复用前缀之前，却期待稳定 cache hit。 | 虚构实时提供商合同中不存在的 cache TTL 或 cache-write 价格。 | 未做 cold/warm 的 usage、延迟、质量与扣费对比就宣布节省。",
    "volatile user data를 reusable prefix 앞에 두고 안정적 cache hit를 기대합니다. | live provider contract에 없는 cache TTL이나 cache-write 가격을 만듭니다. | cold/warm usage, latency, 품질, charge 비교 없이 절감을 주장합니다.",
  ),
  "best-practices": three(
    "Starting with a hard-coded model ID and never validating it against {catalog}. | Retrying a streamed turn after the first byte or without a total deadline. | Rolling out a model or prompt change globally without a fixed eval, canary and rollback rule.",
    "Стартовать с hard-coded model ID без проверки {catalog}. | Retry streamed turn после first byte или без total deadline. | Катить model/prompt change глобально без fixed eval, canary и rollback rule.",
    "使用硬编码 model ID 启动，且从不对照 {catalog} 验证。 | 首字节之后或没有总 deadline 时重试流式 turn。 | 没有固定评测、canary 与回滚规则就全局发布模型或 prompt 变更。",
    "{catalog} 검증 없이 hard-coded model ID로 시작합니다. | first byte 이후 또는 total deadline 없이 streamed turn을 retry합니다. | fixed eval, canary, rollback rule 없이 model/prompt 변경을 전역 rollout합니다.",
  ),
  "cli-setup": three(
    "Mixing vendor login state with the named apiToken.sale profile. | Letting the CLI silently replace the explicit catalog model with its default. | Starting a long repository task before status, one minimal prompt and dashboard usage agree.",
    "Смешивать vendor login state с named profile apiToken.sale. | Позволять CLI скрытно заменять explicit catalog model на default. | Запускать долгую repo task до совпадения status, minimal prompt и dashboard usage.",
    "混用厂商登录状态与命名 apiToken.sale profile。 | 允许 CLI 静默用默认模型替换明确目录模型。 | 状态、最小 prompt 与仪表板 usage 尚未一致就开始长时间仓库任务。",
    "vendor login state와 named apiToken.sale profile을 섞습니다. | CLI가 explicit catalog model을 default로 조용히 바꾸게 둡니다. | status, minimal prompt, dashboard usage가 맞기 전에 긴 repository task를 시작합니다.",
  ),
  "key-security": three(
    "Exposing the bearer key to browser code, git history, screenshots or support messages. | Sharing one non-expiring, unbounded key across services and environments. | Revoking a suspected key without preserving request IDs and usage evidence needed for incident scope.",
    "Открывать bearer key browser code, git history, screenshots или support messages. | Использовать один non-expiring unbounded key во всех services/environments. | Revoke подозрительного ключа без request IDs и usage evidence для incident scope.",
    "把 bearer 密钥暴露给浏览器代码、git 历史、截图或支持消息。 | 所有服务与环境共享一个永不过期且无上限的密钥。 | 未保留界定事件范围所需的 request ID 与 usage 证据就撤销疑似密钥。",
    "bearer key를 browser code, git history, screenshot, support message에 노출합니다. | service/environment 전체에서 non-expiring unbounded key 하나를 공유합니다. | incident scope에 필요한 request ID와 usage evidence를 보존하지 않고 의심 key를 revoke합니다.",
  ),
  "ai-agents": three(
    "Running an unbounded loop with no maximum turns, tool calls, wall time or output. | Executing model-proposed tool arguments without schema and authorization checks. | Sharing one unlimited key across agents, so cost and incident ownership cannot be isolated.",
    "Запускать unbounded loop без max turns, tool calls, wall time и output. | Выполнять model-proposed tool arguments без schema/auth checks. | Использовать один unlimited key для agents, теряя cost/incident ownership.",
    "运行没有最大 turn、tool call、wall time 或 output 的无界循环。 | 未做 schema 与授权检查就执行模型提出的 tool 参数。 | 多个 agent 共享一个无限密钥，无法隔离成本与事件责任。",
    "max turn, tool call, wall time, output 없는 unbounded loop를 실행합니다. | schema/auth check 없이 model-proposed tool argument를 실행합니다. | agent 전체가 unlimited key 하나를 공유해 cost/incident ownership을 분리할 수 없습니다.",
  ),
};

/** Additional topical decisions for the manually authored provider entry pages. */
export const PROVIDER_EXISTING_FOCUS: Record<string, ProviderDepthList> = {
  buy: three(
    "Create an independent apiToken.sale account and issue a named server-side key. | Add prepaid balance by an offered payment method; the provider account is not part of checkout. | Discover a model with {catalog}, then verify one low-cap request on {protocol}.",
    "Создайте независимый аккаунт apiToken.sale и named server-side key. | Пополните prepaid balance доступным способом; provider account не участвует в checkout. | Найдите модель через {catalog} и проверьте low-cap запрос через {protocol}.",
    "创建独立 apiToken.sale 账户并签发命名服务端密钥。 | 使用可用付款方式充值预付余额；厂商账户不参与结账。 | 通过 {catalog} 发现模型，再用 {protocol} 验证一次低上限请求。",
    "독립 apiToken.sale 계정을 만들고 named server-side key를 발급합니다. | 제공 결제 방식으로 선불 잔액을 충전하며 provider account는 checkout에 참여하지 않습니다. | {catalog}로 model을 찾고 {protocol} low-cap 요청을 검증합니다.",
  ),
  quickstart: three(
    "Set the exact base URL and {auth} before copying application prompts. | Call {catalog} with the same key and pin a returned model ID. | Require non-empty output, terminal usage and a matching dashboard entry from the first request.",
    "Задайте точный base URL и {auth} до application prompts. | Вызовите {catalog} тем же ключом и закрепите returned model ID. | Для первого запроса потребуйте output, terminal usage и matching dashboard entry.",
    "复制应用 prompt 前设置准确 base URL 与 {auth}。 | 使用同一密钥调用 {catalog} 并固定返回的 model ID。 | 首次请求必须有非空输出、终态 usage 与匹配仪表板记录。",
    "application prompt 전에 정확한 base URL과 {auth}를 설정합니다. | 같은 key로 {catalog}를 호출하고 반환된 model ID를 고정합니다. | 첫 요청에서 비어 있지 않은 output, terminal usage, 일치하는 dashboard entry를 요구합니다.",
  ),
  pricing: three(
    "Compare fresh input, cached input, output and every model-specific usage leg. | Route routine work to {fast}, balanced work to {balanced}, and use {flagship} only when evals justify it. | Reconcile the terminal provider usage with the discounted dashboard charge for a real request.",
    "Сравните fresh input, cached input, output и все model-specific usage legs. | Routine отправляйте в {fast}, balanced — в {balanced}, а {flagship} используйте по eval. | Сверьте terminal provider usage и discounted dashboard charge реального запроса.",
    "比较 fresh input、cached input、output 与所有模型特定 usage 分类。 | 常规任务使用 {fast}，平衡任务使用 {balanced}，仅评测证明时使用 {flagship}。 | 对真实请求核对终态提供商 usage 与折后仪表板扣费。",
    "fresh input, cached input, output, 모든 model-specific usage leg를 비교합니다. | routine은 {fast}, balanced work는 {balanced}, eval이 정당화할 때만 {flagship}을 사용합니다. | 실제 요청의 terminal provider usage와 discounted dashboard charge를 대조합니다.",
  ),
  "model-comparison": three(
    "Define task classes and a fixed quality threshold before comparing the three tiers. | Measure latency, retries and settled cost on the same prompts, not different demos. | Pin the winning exact model ID and keep an explicit escalation and down-routing rule.",
    "Задайте task classes и fixed quality threshold до сравнения tiers. | Измеряйте latency, retries и settled cost на одинаковых prompts. | Закрепите exact model ID и явные escalation/down-routing rules.",
    "比较三个层级前先定义任务类别与固定质量阈值。 | 在相同 prompt 上测量延迟、重试与结算成本，而不是不同 demo。 | 固定获胜的准确 model ID，并保留明确升级与下沉规则。",
    "세 tier 비교 전에 task class와 fixed quality threshold를 정의합니다. | 다른 demo가 아닌 같은 prompt에서 latency, retry, settled cost를 측정합니다. | winning exact model ID를 고정하고 명시적 escalation/down-routing rule을 유지합니다.",
  ),
};
