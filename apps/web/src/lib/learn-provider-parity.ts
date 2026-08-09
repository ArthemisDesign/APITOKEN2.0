import type {
  LearnArticle,
  LearnSection,
  LearnCluster,
  Locale,
  LocalizedContent,
} from "./learn";
import {
  PROVIDER_EXISTING_FOCUS,
  PROVIDER_TOPIC_RISKS,
} from "./learn-provider-depth";

export type ParityProvider = "gpt" | "gemini" | "kimi";

type L10n = Record<Locale, string>;
type L10nList = Record<Locale, string[]>;

const l = (en: string, ru: string, zh: string, ko: string): L10n => ({ en, ru, zh, ko });
const ll = (en: string[], ru: string[], zh: string[], ko: string[]): L10nList => ({ en, ru, zh, ko });
const text = (value: L10n, locale: Locale): string => value[locale];

function interpolate(value: string, facts: ProviderFacts): string {
  return value
    .replaceAll("{provider}", facts.name)
    .replaceAll("{company}", facts.company)
    .replaceAll("{direct}", facts.directProduct)
    .replaceAll("{base}", facts.baseUrl)
    .replaceAll("{auth}", facts.auth)
    .replaceAll("{protocol}", facts.protocol)
    .replaceAll("{catalog}", facts.catalogRoute)
    .replaceAll("{sdk}", facts.sdk)
    .replaceAll("{cli}", facts.cli)
    .replaceAll("{flagship}", facts.models.flagship)
    .replaceAll("{balanced}", facts.models.balanced)
    .replaceAll("{fast}", facts.models.fast)
    .replaceAll("{economy}", facts.models.economy)
    .replaceAll("{previous}", facts.previousGeneration)
    .replaceAll("{current}", facts.currentGeneration)
    .replaceAll("{cacheMode}", facts.cacheMode.en)
    .replaceAll("{streamingEvidence}", facts.streamingEvidence.en);
}

function localized(value: L10n, locale: Locale, facts: ProviderFacts): string {
  return interpolate(text(value, locale), {
    ...facts,
    cacheMode: l(text(facts.cacheMode, locale), text(facts.cacheMode, locale), text(facts.cacheMode, locale), text(facts.cacheMode, locale)),
    streamingEvidence: l(text(facts.streamingEvidence, locale), text(facts.streamingEvidence, locale), text(facts.streamingEvidence, locale), text(facts.streamingEvidence, locale)),
  });
}

function localizedList(value: L10nList, locale: Locale, facts: ProviderFacts): string[] {
  const localeFacts = {
    ...facts,
    cacheMode: l(text(facts.cacheMode, locale), text(facts.cacheMode, locale), text(facts.cacheMode, locale), text(facts.cacheMode, locale)),
    streamingEvidence: l(text(facts.streamingEvidence, locale), text(facts.streamingEvidence, locale), text(facts.streamingEvidence, locale), text(facts.streamingEvidence, locale)),
  };
  return value[locale].map((entry) => interpolate(entry, localeFacts));
}

type ProviderFacts = {
  id: ParityProvider;
  name: string;
  company: string;
  directProduct: string;
  baseUrl: string;
  auth: string;
  protocol: string;
  catalogRoute: string;
  sdk: string;
  cli: string;
  models: { flagship: string; balanced: string; fast: string; economy: string };
  previousGeneration: string;
  currentGeneration: string;
  quickstartSlug: string;
  buySlug: string;
  pricingSlug: string;
  compareSlug: string;
  cliSlug: string;
  cacheMode: L10n;
  streamingEvidence: L10n;
};

const PROVIDERS: Record<ParityProvider, ProviderFacts> = {
  gpt: {
    id: "gpt",
    name: "GPT",
    company: "OpenAI",
    directProduct: "OpenAI Platform",
    baseUrl: "https://router.apitoken.sale/v1",
    auth: "Authorization: Bearer",
    protocol: "Responses or Chat Completions",
    catalogRoute: "GET /v1/models",
    sdk: "OpenAI SDK",
    cli: "Codex CLI",
    models: { flagship: "gpt-5.6-sol", balanced: "gpt-5.6-terra", fast: "gpt-5.6-luna", economy: "gpt-5.6-luna" },
    previousGeneration: "GPT-5.5",
    currentGeneration: "GPT-5.6",
    quickstartSlug: "openai-api-quickstart",
    buySlug: "how-to-buy-gpt-api-key",
    pricingSlug: "gpt-api-pricing",
    compareSlug: "gpt-5-6-sol-vs-terra-vs-luna",
    cliSlug: "codex-cli-setup",
    cacheMode: l(
      "explicit cached-input usage plus provider-managed cache writes",
      "явный cached-input usage и управляемая провайдером запись кэша",
      "明确的 cached-input usage 与提供商管理的缓存写入",
      "명시적 cached-input usage와 provider 관리 cache write",
    ),
    streamingEvidence: l(
      "incremental SSE is live-verified on Responses and Chat Completions",
      "инкрементальный SSE live-проверен для Responses и Chat Completions",
      "Responses 与 Chat Completions 的增量 SSE 已通过实时验证",
      "Responses와 Chat Completions의 증분 SSE가 live 검증됨",
    ),
  },
  gemini: {
    id: "gemini",
    name: "Gemini",
    company: "Google",
    directProduct: "Google AI Studio / Vertex AI",
    baseUrl: "https://router.apitoken.sale",
    auth: "x-goog-api-key",
    protocol: "Gemini generateContent",
    catalogRoute: "GET /v1beta/models",
    sdk: "Google GenAI SDK",
    cli: "Gemini CLI",
    models: { flagship: "gemini-3.6-pro", balanced: "gemini-3.6-flash", fast: "gemini-3.6-flash-lite", economy: "gemini-3.6-flash-lite" },
    previousGeneration: "Gemini 3.1",
    currentGeneration: "Gemini 3.6",
    quickstartSlug: "gemini-api-quickstart",
    buySlug: "how-to-buy-gemini-api-key",
    pricingSlug: "gemini-api-pricing",
    compareSlug: "gemini-pro-vs-flash-vs-flash-lite",
    cliSlug: "gemini-cli-api-key",
    cacheMode: l(
      "explicit and implicit Gemini context caching with native usage fields",
      "явное и неявное кэширование контекста Gemini с нативными полями usage",
      "带原生 usage 字段的 Gemini 显式与隐式上下文缓存",
      "native usage field를 제공하는 Gemini 명시적·암시적 context caching",
    ),
    streamingEvidence: l(
      "native streamGenerateContent SSE is live-verified",
      "нативный streamGenerateContent SSE live-проверен",
      "原生 streamGenerateContent SSE 已通过实时验证",
      "native streamGenerateContent SSE가 live 검증됨",
    ),
  },
  kimi: {
    id: "kimi",
    name: "Kimi",
    company: "Moonshot AI",
    directProduct: "Kimi Code membership / Kimi Open Platform",
    baseUrl: "https://router.apitoken.sale",
    auth: "x-api-key",
    protocol: "Anthropic Messages or the universal OpenAI-compatible lane",
    catalogRoute: "GET /v1/models",
    sdk: "Anthropic SDK",
    cli: "Kimi Code",
    models: { flagship: "kimi/k3", balanced: "kimi/kimi-for-coding", fast: "kimi/kimi-for-coding-highspeed", economy: "kimi/kimi-for-coding" },
    previousGeneration: "Kimi for Coding",
    currentGeneration: "Kimi K3",
    quickstartSlug: "kimi-api-quickstart",
    buySlug: "how-to-buy-kimi-api-key",
    pricingSlug: "kimi-api-pricing",
    compareSlug: "kimi-k3-vs-kimi-for-coding",
    cliSlug: "kimi-api-for-kimi-code",
    cacheMode: l(
      "automatic cache-hit and cache-miss accounting reported in terminal usage",
      "автоматический учёт cache hit и cache miss в terminal usage",
      "终态 usage 中自动报告 cache hit 与 cache miss",
      "terminal usage에 자동 보고되는 cache hit·cache miss accounting",
    ),
    streamingEvidence: l(
      "stream:true is accepted, but public chunk incrementality remains a preview capability under live validation",
      "stream:true принимается, но инкрементальность публичных chunks остаётся preview-возможностью на live-проверке",
      "可接受 stream:true，但公共 chunk 的增量性仍是实时验证中的 preview 能力",
      "stream:true는 허용되지만 public chunk 증분성은 live 검증 중인 preview capability",
    ),
  },
};

export const PARITY_PROVIDERS: readonly ParityProvider[] = ["gpt", "gemini", "kimi"];

type TopicKind = "access" | "model" | "tool" | "compare" | "operations";

type TopicSpec = {
  id: string;
  kind: TopicKind;
  cluster: LearnCluster;
  slug: (provider: ProviderFacts) => string;
  title: L10n;
  description: L10n;
  keywords: L10nList;
  dek: L10n;
  focus: L10nList;
  providerExisting?: Partial<Record<ParityProvider, string>>;
};

const accessTopics: TopicSpec[] = [
  {
    id: "cheapest",
    kind: "access",
    cluster: "buy",
    slug: (p) => `cheapest-${p.id}-api`,
    title: l("Cheapest {provider} API: What Actually Lowers Cost", "Самый дешёвый {provider} API: что действительно снижает цену", "最便宜的 {provider} API：真正降低成本的方法", "가장 저렴한 {provider} API: 실제 비용 절감 방법"),
    description: l("Compare direct {provider} API spend with apiToken.sale pricing, model routing, caching and prepaid controls to find the lowest sustainable cost without hiding usage.", "Сравните прямые расходы {provider} API с ценами apiToken.sale, маршрутизацией моделей, кэшированием и предоплатой — без скрытого usage.", "比较 {provider} API 直连费用与 apiToken.sale 定价、模型路由、缓存和预付费控制，在不隐藏 usage 的前提下降低成本。", "{provider} API 직접 비용과 apiToken.sale 가격, 모델 routing, caching, 선불 control을 비교해 usage를 숨기지 않는 최저 지속 비용을 찾습니다."),
    keywords: ll(["cheapest {provider} api", "cheap {provider} api", "{provider} api discount", "lower {provider} token cost"], ["дешёвый {provider} api", "самый дешёвый {provider} api", "скидка {provider} api", "снизить цену токенов {provider}"], ["最便宜 {provider} api", "低价 {provider} api", "{provider} api 折扣", "降低 {provider} token 成本"], ["저렴한 {provider} api", "가장 싼 {provider} api", "{provider} api 할인", "{provider} token 비용 절감"]),
    dek: l("The cheapest route is not one headline rate. It combines the right model tier, real cache usage, bounded output and a discount applied to settled provider spend.", "Самый дешёвый маршрут — не одна ставка. Нужны подходящий tier, реальный cache usage, ограниченный output и скидка от фактического расхода провайдера.", "最低成本不是单一报价，而是合适的模型层级、真实缓存 usage、受控 output，以及对结算后提供商费用应用的折扣。", "최저 비용은 하나의 headline rate가 아니라 적절한 model tier, 실제 cache usage, 제한된 output, 정산된 provider 비용에 적용되는 할인입니다."),
    focus: ll(["Route routine work to {economy}, balanced production work to {balanced}, and reserve {flagship} for eval-proven hard cases.", "Compare complete input, cached-input and output legs rather than advertising only the cheapest input number.", "Use a lifetime key spending limit and inspect per-request usage so a low rate cannot hide a runaway loop."], ["Отправляйте простые задачи в {economy}, обычный production — в {balanced}, а {flagship} оставляйте для сложных случаев, подтверждённых eval.", "Сравнивайте полные input, cached-input и output компоненты, а не только самую низкую входную ставку.", "Задайте lifetime-лимит ключа и проверяйте usage каждого запроса, чтобы низкая ставка не скрыла бесконечный цикл."], ["常规任务使用 {economy}，平衡型生产任务使用 {balanced}，只有评测证明有必要时才升级到 {flagship}。", "比较完整的 input、cached-input 与 output 成本，不要只展示最低 input 单价。", "设置密钥终身消费上限并检查每次请求的 usage，避免低单价掩盖失控循环。"], ["일상 작업은 {economy}, 균형형 production은 {balanced}, eval로 필요성이 확인된 어려운 작업만 {flagship}으로 보냅니다.", "가장 싼 input 숫자만이 아니라 전체 input, cached-input, output leg를 비교합니다.", "key lifetime 지출 한도와 요청별 usage를 확인해 낮은 단가가 runaway loop를 숨기지 않게 합니다."]),
  },
  {
    id: "restricted-regions",
    kind: "access",
    cluster: "buy",
    slug: (p) => `${p.id}-api-for-russia`,
    title: l("{provider} API from Russia and Restricted Regions", "{provider} API из России и регионов с ограничениями", "从俄罗斯及受限地区使用 {provider} API", "러시아 및 제한 지역에서 {provider} API 사용"),
    description: l("Use {provider} through an independent prepaid apiToken.sale account when direct signup, billing or cards are unavailable in your region, without pretending regional rules do not exist.", "Используйте {provider} через независимый предоплаченный аккаунт apiToken.sale, когда прямые регистрация, биллинг или карты недоступны в регионе, с учётом региональных правил.", "当所在地区无法直接注册、结算或使用银行卡时，通过独立预付费 apiToken.sale 账户使用 {provider}，同时遵守地区规则。", "지역에서 직접 가입, billing, 카드 사용이 어려울 때 독립 선불 apiToken.sale 계정으로 {provider}를 사용하되 지역 규정을 무시하지 않습니다."),
    keywords: ll(["{provider} api russia", "{provider} api restricted country", "{provider} api international payment", "{provider} api without local card"], ["{provider} api россия", "{provider} api из ограниченной страны", "международная оплата {provider} api", "{provider} api без зарубежной карты"], ["{provider} api 俄罗斯", "{provider} api 受限国家", "{provider} api 国际支付", "无海外银行卡 {provider} api"], ["{provider} api 러시아", "제한 국가 {provider} api", "{provider} api 해외 결제", "해외 카드 없이 {provider} api"]),
    dek: l("The gateway separates your developer key and prepaid balance from the provider's direct consumer signup flow. Availability still depends on lawful network access and the live model catalog.", "Шлюз отделяет developer key и предоплаченный баланс от прямой регистрации у провайдера. Доступность всё равно зависит от законного сетевого доступа и live-каталога.", "网关将开发者密钥和预付余额与提供商的直接消费者注册流程分离；可用性仍取决于合法网络访问和实时模型目录。", "gateway는 developer key와 선불 잔액을 provider의 직접 consumer 가입과 분리합니다. 사용 가능 여부는 합법적 network access와 live catalog에 달려 있습니다."),
    focus: ll(["Create the apiToken.sale account with an email or supported social login and generate a separate server-side key.", "Top up by an offered card or cryptocurrency method; the checkout provider, not a model vendor account, processes the payment.", "Check {catalog} before deployment because the key-scoped catalog is the authority for currently routable models."], ["Создайте аккаунт apiToken.sale через email или доступный social login и выпустите отдельный серверный ключ.", "Пополните баланс доступным способом — картой или криптовалютой; платёж обрабатывает checkout provider, а не аккаунт model vendor.", "Перед deployment проверьте {catalog}: каталог конкретного ключа определяет доступные модели."], ["使用邮箱或支持的社交登录创建 apiToken.sale 账户，并生成独立的服务端密钥。", "通过可用的银行卡或加密货币方式充值；付款由 checkout provider 处理，而不是模型厂商账户。", "部署前检查 {catalog}，密钥范围目录才是当前可路由模型的权威来源。"], ["email 또는 지원 social login으로 apiToken.sale 계정을 만들고 별도 server-side key를 생성합니다.", "제공되는 카드 또는 암호화폐 방식으로 충전하며 model vendor 계정이 아니라 checkout provider가 결제를 처리합니다.", "배포 전 {catalog}를 확인합니다. key-scoped catalog가 현재 routing 가능한 모델의 권위입니다."]),
  },
  {
    id: "crypto-payment",
    kind: "access",
    cluster: "buy",
    slug: (p) => `${p.id}-api-crypto-payment`,
    title: l("Pay for the {provider} API with Cryptocurrency", "Оплата {provider} API криптовалютой", "使用加密货币支付 {provider} API", "암호화폐로 {provider} API 결제"),
    description: l("Top up a prepaid balance for {provider} API usage with cryptocurrency, then audit exact token charges and keep the model key separate from the payment flow.", "Пополняйте предоплаченный баланс для {provider} API криптовалютой, проверяйте точные token charges и не смешивайте API-ключ с платёжным процессом.", "使用加密货币为 {provider} API 预付余额充值，审计精确 token 扣费，并将模型密钥与支付流程分离。", "암호화폐로 {provider} API 선불 잔액을 충전하고 정확한 token charge를 감사하며 model key와 결제 흐름을 분리합니다."),
    keywords: ll(["{provider} api crypto", "buy {provider} api with usdt", "{provider} api cryptocurrency payment", "prepaid {provider} api"], ["{provider} api криптовалюта", "купить {provider} api за usdt", "оплата {provider} api криптой", "предоплаченный {provider} api"], ["{provider} api 加密货币", "usdt 购买 {provider} api", "{provider} api 加密支付", "预付费 {provider} api"], ["{provider} api 암호화폐", "usdt로 {provider} api 구매", "{provider} api crypto 결제", "선불 {provider} api"]),
    dek: l("Crypto is a balance funding method, not a different API product. Requests still use the normal protocol, model IDs, metering and dashboard ledger.", "Криптовалюта — способ пополнения баланса, а не другой API-продукт. Запросы используют обычные протокол, model IDs, metering и ledger в дашборде.", "加密货币只是余额充值方式，不是另一种 API 产品；请求仍使用正常协议、model ID、计量和仪表板账本。", "crypto는 잔액 충전 방식일 뿐 다른 API product가 아닙니다. 요청은 정상 protocol, model ID, metering, dashboard ledger를 사용합니다."),
    focus: ll(["Choose cryptocurrency at checkout and follow the processor's exact network and confirmation instructions.", "Wait for the credited balance before sending production traffic; a blockchain transaction alone is not an API authorization.", "Use {auth} only for model calls to {base}; never paste the API key into a wallet or payment form."], ["Выберите криптовалюту на checkout и соблюдайте указанные процессором сеть и число подтверждений.", "Дождитесь зачисления баланса до production-трафика: blockchain transaction сама по себе не авторизует API.", "Используйте {auth} только для model calls к {base}; не вставляйте API-ключ в кошелёк или платёжную форму."], ["在结账时选择加密货币，并严格遵循处理商指定的网络与确认要求。", "余额到账后再发送生产流量；区块链交易本身不等于 API 授权。", "{auth} 仅用于调用 {base}；不要把 API 密钥粘贴到钱包或支付表单。"], ["checkout에서 암호화폐를 선택하고 processor가 지정한 network와 confirmation 지침을 따릅니다.", "production traffic 전 잔액 반영을 기다립니다. blockchain transaction 자체는 API 인증이 아닙니다.", "{auth}는 {base} model call에만 사용하고 API key를 wallet이나 결제 form에 붙여넣지 않습니다."]),
  },
  {
    id: "no-waitlist",
    kind: "access",
    cluster: "buy",
    slug: (p) => `${p.id}-api-without-waitlist`,
    title: l("{provider} API without a Waitlist or Vendor Approval", "{provider} API без waitlist и одобрения вендора", "无需候补名单或厂商审批的 {provider} API", "대기 목록이나 vendor 승인 없는 {provider} API"),
    description: l("Generate an apiToken.sale key immediately, fund prepaid balance and call currently enabled {provider} models without waiting for a separate {direct} organization approval.", "Сразу выпустите ключ apiToken.sale, пополните баланс и вызывайте доступные {provider} модели без отдельного одобрения организации {direct}.", "立即生成 apiToken.sale 密钥、充值预付余额并调用当前启用的 {provider} 模型，无需等待单独的 {direct} 组织审批。", "apiToken.sale key를 즉시 발급하고 선불 잔액을 충전해 별도 {direct} 조직 승인 없이 현재 활성화된 {provider} 모델을 호출합니다."),
    keywords: ll(["{provider} api no waitlist", "instant {provider} api access", "{provider} api without approval", "get {provider} api key fast"], ["{provider} api без waitlist", "быстрый доступ {provider} api", "{provider} api без одобрения", "быстро получить {provider} api ключ"], ["{provider} api 无候补", "即时 {provider} api", "{provider} api 无审批", "快速获取 {provider} api 密钥"], ["{provider} api 대기 없음", "즉시 {provider} api", "승인 없는 {provider} api", "빠른 {provider} api key"]),
    dek: l("Account creation and key issuance are immediate, but model availability is never guessed: the key-scoped catalog and balance decide whether a request can run.", "Аккаунт и ключ создаются сразу, но доступность моделей не угадывается: запрос определяют каталог ключа и баланс.", "账户与密钥可立即创建，但绝不猜测模型可用性：密钥范围目录和余额决定请求能否执行。", "계정과 key는 즉시 만들 수 있지만 모델 가용성을 추측하지 않습니다. key-scoped catalog와 잔액이 요청 실행 여부를 결정합니다."),
    focus: ll(["Register, generate a named key and store it in a server-side secret before copying any integration snippet.", "Call {catalog} with that same key and select a returned model instead of relying on an old screenshot.", "Run a minimal request on {protocol}; a 402 means balance, while a 404 means the requested model is not enabled."], ["Зарегистрируйтесь, создайте именованный ключ и сохраните его как server-side secret до копирования integration snippet.", "Вызовите {catalog} тем же ключом и выберите возвращённую модель вместо старого screenshot.", "Отправьте минимальный запрос через {protocol}: 402 означает баланс, 404 — модель не включена."], ["注册后生成命名密钥，并在复制集成代码前将其保存为服务端 secret。", "使用同一密钥调用 {catalog}，选择实际返回的模型，不依赖旧截图。", "通过 {protocol} 发送最小请求；402 表示余额问题，404 表示模型未启用。"], ["가입 후 named key를 만들고 integration snippet을 복사하기 전에 server-side secret으로 저장합니다.", "같은 key로 {catalog}를 호출하고 오래된 screenshot 대신 반환된 모델을 선택합니다.", "{protocol}로 최소 요청을 실행합니다. 402는 잔액, 404는 모델 비활성화를 뜻합니다."]),
  },
  {
    id: "free-key",
    kind: "access",
    cluster: "free",
    slug: (p) => `free-${p.id}-api-key`,
    title: l("Free {provider} API Key: Start with Bonus Credit", "Бесплатный API-ключ {provider}: старт с бонусным балансом", "免费 {provider} API 密钥：使用奖励余额开始", "무료 {provider} API 키: 보너스 크레딧으로 시작"),
    description: l("Create a {provider} API key and use the $5 platform bonus available to eligible Google/GitHub registrations before adding paid balance.", "Создайте API-ключ {provider} и используйте бонус платформы $5 для подходящих регистраций через Google/GitHub до первого пополнения.", "创建 {provider} API 密钥，符合条件的 Google/GitHub 新账户可在充值前使用 $5 平台奖励余额。", "{provider} API key를 만들고 조건에 맞는 Google/GitHub 신규 가입의 $5 platform bonus를 충전 전에 사용합니다."),
    keywords: ll(["free {provider} api key", "{provider} api free credit", "try {provider} api free", "{provider} api bonus"], ["бесплатный {provider} api ключ", "бесплатный баланс {provider} api", "попробовать {provider} api бесплатно", "бонус {provider} api"], ["免费 {provider} api 密钥", "{provider} api 免费额度", "免费试用 {provider} api", "{provider} api 奖励"], ["무료 {provider} api key", "{provider} api 무료 크레딧", "{provider} api 무료 체험", "{provider} api 보너스"]),
    dek: l("The key is real and reusable; the bonus is platform credit with eligibility rules, not an unlimited vendor trial. Use a minimal request and measure every token.", "Ключ настоящий и многоразовый; бонус — это platform credit с условиями, а не безлимитный trial вендора. Начните с минимального запроса и измеряйте токены.", "密钥是真实且可重复使用的；奖励是有资格条件的平台余额，不是无限厂商试用。请从最小请求开始并计量每个 token。", "key는 실제 재사용 가능한 key이며 bonus는 조건이 있는 platform credit이지 무제한 vendor trial이 아닙니다. 최소 요청으로 시작하고 모든 token을 측정합니다."),
    focus: ll(["Register with Google or GitHub; email/password registrations do not receive the welcome bonus.", "Generate a key, call {catalog}, and choose the least expensive suitable model for the first smoke test.", "Set a small lifetime spending limit before adding paid balance so experiments stay bounded."], ["Зарегистрируйтесь через Google или GitHub: email/password не получает welcome bonus.", "Создайте ключ, вызовите {catalog} и выберите самую дешёвую подходящую модель для smoke test.", "До платного пополнения задайте небольшой lifetime-лимит ключа, чтобы эксперименты были ограничены."], ["通过 Google 或 GitHub 注册；邮箱密码注册不获得欢迎奖励。", "生成密钥、调用 {catalog}，并为首次 smoke test 选择最便宜的合适模型。", "充值前设置较小的密钥终身消费上限，确保实验有边界。"], ["Google 또는 GitHub로 가입합니다. email/password 가입은 welcome bonus를 받지 못합니다.", "key를 만들고 {catalog}를 호출해 첫 smoke test에 가장 저렴한 적합 모델을 선택합니다.", "유료 충전 전 작은 lifetime 지출 한도를 설정해 실험 비용을 제한합니다."]),
  },
  {
    id: "free-trial",
    kind: "access",
    cluster: "free",
    slug: (p) => `${p.id}-api-free-trial`,
    title: l("{provider} API Free Trial: What You Can Verify", "Бесплатный trial {provider} API: что можно проверить", "{provider} API 免费试用：可以验证什么", "{provider} API 무료 체험: 검증 가능한 항목"),
    description: l("Use eligible welcome credit to verify {provider} authentication, model discovery, one minimal response and dashboard usage before committing paid balance.", "Используйте доступный welcome credit, чтобы проверить авторизацию {provider}, каталог, минимальный ответ и usage в дашборде до платного пополнения.", "使用符合条件的欢迎余额，在付费充值前验证 {provider} 鉴权、模型发现、最小响应和仪表板 usage。", "조건에 맞는 welcome credit으로 유료 충전 전 {provider} 인증, model discovery, 최소 응답, dashboard usage를 검증합니다."),
    keywords: ll(["{provider} api free trial", "test {provider} api", "{provider} api trial credit", "{provider} api smoke test"], ["{provider} api бесплатный trial", "тест {provider} api", "trial credit {provider} api", "smoke test {provider} api"], ["{provider} api 免费试用", "测试 {provider} api", "{provider} api 试用额度", "{provider} api smoke test"], ["{provider} api 무료 체험", "{provider} api 테스트", "{provider} api trial credit", "{provider} api smoke test"]),
    dek: l("A useful trial proves the complete request path, not model quality from one prompt. Verify auth, protocol, terminal usage and the exact charge independently.", "Полезный trial проверяет весь request path, а не качество модели по одному промпту. Отдельно подтвердите auth, protocol, terminal usage и точное списание.", "有效试用应验证完整请求链路，而不是用一个 prompt 判断模型质量；需分别确认鉴权、协议、终态 usage 和精确扣费。", "유용한 trial은 한 prompt의 모델 품질이 아니라 전체 request path를 증명합니다. auth, protocol, terminal usage, 정확한 charge를 각각 확인합니다."),
    focus: ll(["First call {catalog}; discovery is free and prevents a paid request to a stale model ID.", "Send a deterministic prompt with a low output cap through {protocol} and confirm a non-empty response.", "Match the terminal usage to the dashboard entry before evaluating larger prompts or agent tools."], ["Сначала вызовите {catalog}: discovery бесплатен и предотвращает платный запрос к устаревшему model ID.", "Отправьте детерминированный prompt с малым output cap через {protocol} и подтвердите непустой ответ.", "Сопоставьте terminal usage с записью в дашборде до больших промптов и agent tools."], ["先调用 {catalog}；模型发现免费，可避免向过期 model ID 发送付费请求。", "通过 {protocol} 发送带低 output 上限的确定性 prompt，并确认响应非空。", "在测试大 prompt 或 agent tool 前，将终态 usage 与仪表板记录核对。"], ["먼저 {catalog}를 호출합니다. discovery는 무료이며 오래된 model ID에 유료 요청하는 일을 막습니다.", "{protocol}로 낮은 output cap의 deterministic prompt를 보내고 비어 있지 않은 응답을 확인합니다.", "큰 prompt나 agent tool 평가 전 terminal usage와 dashboard 항목을 대조합니다."]),
  },
  {
    id: "cli-without-subscription",
    kind: "tool",
    cluster: "free",
    slug: (p) => `${p.id}-cli-without-subscription`,
    title: l("Use {cli} without a Direct Subscription", "{cli} без прямой подписки", "无需直接订阅即可使用 {cli}", "직접 구독 없이 {cli} 사용"),
    description: l("Run {cli} with an apiToken.sale API key and prepaid balance instead of binding the tool to a separate {direct} subscription.", "Запускайте {cli} с API-ключом apiToken.sale и предоплаченным балансом вместо отдельной подписки {direct}.", "使用 apiToken.sale API 密钥和预付余额运行 {cli}，无需绑定单独的 {direct} 订阅。", "별도 {direct} 구독 대신 apiToken.sale API key와 선불 잔액으로 {cli}를 실행합니다."),
    keywords: ll(["{cli} without subscription", "{cli} api key", "{provider} cli custom endpoint", "pay as you go {cli}"], ["{cli} без подписки", "api ключ {cli}", "custom endpoint {provider} cli", "{cli} с оплатой по факту"], ["{cli} 无订阅", "{cli} api 密钥", "{provider} cli 自定义端点", "按量付费 {cli}"], ["구독 없는 {cli}", "{cli} api key", "{provider} cli custom endpoint", "종량제 {cli}"]),
    dek: l("The CLI remains the same client; only authentication and billing move to an explicit API-key profile. Keep direct login state separate so each run has an unambiguous payer.", "CLI остаётся тем же клиентом; в отдельный API-key profile переносятся только auth и billing. Не смешивайте direct login, чтобы для каждого запуска был понятен плательщик.", "CLI 客户端不变，只把鉴权和结算移到明确的 API-key profile。请将直接登录状态分离，确保每次运行的付款方清晰。", "CLI는 같은 client이며 인증과 billing만 명시적 API-key profile로 이동합니다. direct login 상태를 분리해 각 실행의 payer를 명확히 합니다."),
    focus: ll(["Create a named profile that points to {base} and reads the key from the tool's supported secret location.", "Pin an explicit model returned by {catalog}; do not rely on the CLI vendor's default model.", "Verify the active provider/base URL inside {cli} before starting a long repository session."], ["Создайте именованный profile на {base}, который читает ключ из поддерживаемого CLI secret location.", "Закрепите явную модель из {catalog}; не полагайтесь на default model вендора CLI.", "Проверьте активные provider/base URL внутри {cli} до длинной сессии с репозиторием."], ["创建指向 {base} 的命名 profile，并从工具支持的 secret 位置读取密钥。", "固定 {catalog} 返回的明确模型，不依赖 CLI 厂商默认模型。", "开始长时间仓库任务前，在 {cli} 中确认当前 provider/base URL。"], ["{base}를 가리키고 tool이 지원하는 secret 위치에서 key를 읽는 named profile을 만듭니다.", "CLI vendor 기본 모델에 의존하지 말고 {catalog}가 반환한 명시적 모델을 고정합니다.", "긴 repository session 전 {cli} 내부의 active provider/base URL을 확인합니다."]),
    providerExisting: { gpt: "codex-cli-setup", kimi: "kimi-api-for-kimi-code" },
  },
];

const modelTopics: TopicSpec[] = [
  {
    id: "flagship-model",
    kind: "model",
    cluster: "free",
    slug: (p) => `${p.id}-${p.models.flagship.replace(/[^a-z0-9]+/gi, "-")}-api`.replace(`${p.id}-${p.id}-`, `${p.id}-`),
    title: l("{flagship} API Access and Best Use Cases", "{flagship} API: доступ и лучшие сценарии", "{flagship} API 使用与最佳场景", "{flagship} API 접근 및 최적 용도"),
    description: l("Use {flagship} for the hardest {provider} reasoning, long-context and agent workloads; learn the correct model ID, protocol, cost controls and escalation criteria.", "Используйте {flagship} для самых сложных reasoning, long-context и agent-задач {provider}: model ID, protocol, контроль цены и критерии эскалации.", "将 {flagship} 用于最困难的 {provider} 推理、长上下文和 agent 工作负载，并掌握正确 model ID、协议、成本控制与升级标准。", "가장 어려운 {provider} reasoning, long-context, agent workload에 {flagship}을 사용하고 정확한 model ID, protocol, 비용 control, escalation 기준을 익힙니다."),
    keywords: ll(["{flagship} api", "{provider} flagship api", "{flagship} coding", "{flagship} price"], ["{flagship} api", "флагман {provider} api", "{flagship} для кода", "цена {flagship}"], ["{flagship} api", "{provider} 旗舰 api", "{flagship} 编程", "{flagship} 价格"], ["{flagship} api", "{provider} flagship api", "{flagship} 코딩", "{flagship} 가격"]),
    dek: l("Flagship does not mean default. Start with a balanced model, measure quality on your eval set, and escalate only the requests where {flagship} earns its higher cost or latency.", "Flagship — не default. Начните с balanced model, измерьте качество на eval и повышайте tier только там, где {flagship} оправдывает цену или latency.", "旗舰不等于默认。先使用平衡模型，在评测集上测量质量，仅在 {flagship} 的更高成本或延迟确有价值时升级。", "flagship은 default가 아닙니다. balanced model로 시작해 eval 품질을 측정하고 {flagship}의 더 높은 비용이나 latency가 가치 있는 요청만 올립니다."),
    focus: ll(["Address the model exactly as {flagship}; aliases from a vendor UI are not interchangeable with the router catalog.", "Use it for hard architecture, deep review, long-horizon planning and the eval cases where {balanced} misses requirements.", "Cap output, reuse stable context and compare settled usage against {balanced} before making it a global default."], ["Указывайте модель точно как {flagship}: aliases из vendor UI не заменяют router catalog.", "Используйте её для сложной архитектуры, deep review, долгого планирования и eval-кейсов, где {balanced} не проходит требования.", "Ограничьте output, переиспользуйте стабильный контекст и сравните settled usage с {balanced} до global default."], ["模型必须精确写为 {flagship}；厂商 UI 中的别名不能替代路由目录 ID。", "用于复杂架构、深度审查、长期规划，以及 {balanced} 未达到要求的评测案例。", "限制 output、复用稳定上下文，并在设为全局默认前与 {balanced} 的结算 usage 比较。"], ["모델을 정확히 {flagship}으로 지정합니다. vendor UI alias는 router catalog ID와 호환되지 않습니다.", "어려운 architecture, deep review, 장기 planning, {balanced}가 요구사항을 놓치는 eval case에 사용합니다.", "global default 전 output을 제한하고 stable context를 재사용하며 settled usage를 {balanced}와 비교합니다."]),
  },
  {
    id: "balanced-model",
    kind: "model",
    cluster: "free",
    slug: (p) => `${p.id}-${p.models.balanced.replace(/[^a-z0-9]+/gi, "-")}-api`.replace(`${p.id}-${p.id}-`, `${p.id}-`),
    title: l("{balanced} API: The Balanced Production Default", "{balanced} API: сбалансированный production default", "{balanced} API：平衡型生产默认选择", "{balanced} API: 균형형 production 기본값"),
    description: l("Configure {balanced} as the everyday {provider} model for production chat, coding and agents, with explicit routing rules for flagship and fast tiers.", "Настройте {balanced} как повседневную {provider} модель для production chat, coding и agents с явными правилами перехода на flagship и fast tiers.", "将 {balanced} 配置为生产聊天、编程和 agent 的日常 {provider} 模型，并为旗舰与快速层级设置明确路由规则。", "{balanced}를 production chat, coding, agent의 일상 {provider} 모델로 설정하고 flagship·fast tier 전환 규칙을 명시합니다."),
    keywords: ll(["{balanced} api", "best default {provider} model", "{balanced} coding", "{balanced} production"], ["{balanced} api", "лучшая default модель {provider}", "{balanced} для кода", "{balanced} production"], ["{balanced} api", "最佳默认 {provider} 模型", "{balanced} 编程", "{balanced} 生产"], ["{balanced} api", "최고의 기본 {provider} 모델", "{balanced} 코딩", "{balanced} production"]),
    dek: l("A balanced default keeps quality predictable without paying flagship rates for every parser, edit or routine turn. Routing rules matter more than one universal model choice.", "Balanced default сохраняет качество без flagship-цены за каждый parser, edit или обычный turn. Правила routing важнее одной универсальной модели.", "平衡默认可保持质量稳定，又避免每个解析、编辑或常规 turn 都支付旗舰价格；路由规则比单一万能模型更重要。", "balanced default는 모든 parser, edit, routine turn에 flagship 가격을 내지 않고 예측 가능한 품질을 유지합니다. 하나의 universal model보다 routing rule이 중요합니다."),
    focus: ll(["Use {balanced} for normal coding, production assistants and multi-step agents whose evals do not require {flagship}.", "Escalate only failed or high-risk cases to {flagship}; route extraction and classification to the lowest-cost passing tier, {economy}.", "Track quality, latency and settled cost by task class so routing changes are evidence-based."], ["Используйте {balanced} для обычного coding, production assistants и multi-step agents, которым по eval не нужен {flagship}.", "Повышайте до {flagship} только failed/high-risk кейсы, а extraction и classification отправляйте в самый дешёвый passing tier — {economy}.", "Измеряйте quality, latency и settled cost по классам задач, чтобы routing опирался на доказательства."], ["常规编程、生产助手和评测不需要 {flagship} 的多步骤 agent 使用 {balanced}。", "仅将失败或高风险案例升级到 {flagship}，提取和分类任务使用通过评测的最低价层级 {economy}。", "按任务类别跟踪质量、延迟和结算成本，使路由调整有证据。"], ["일반 coding, production assistant, eval상 {flagship}이 필요 없는 multi-step agent에 {balanced}를 사용합니다.", "실패하거나 high-risk인 case만 {flagship}으로 올리고 extraction·classification은 통과한 최저비용 tier인 {economy}로 보냅니다.", "task class별 quality, latency, settled cost를 추적해 routing 변경을 evidence 기반으로 합니다."]),
  },
  {
    id: "fast-model",
    kind: "model",
    cluster: "free",
    slug: (p) => `${p.id}-${p.models.fast.replace(/[^a-z0-9]+/gi, "-")}-api`.replace(`${p.id}-${p.id}-`, `${p.id}-`),
    title: l("{fast} API for Fast, High-Volume Work", "{fast} API для быстрых массовых задач", "{fast} API：快速高并发任务", "{fast} API: 빠른 대량 작업"),
    description: l("Use {fast} for latency-sensitive classification, extraction, routing and agent substeps, then compare its speed premium and validation rate with {balanced} or {flagship}.", "Используйте {fast} для latency-sensitive classification, extraction, routing и agent substeps, сравнивая speed premium и validation rate с {balanced} или {flagship}.", "将 {fast} 用于延迟敏感的分类、提取、路由与 agent 子步骤，并将其速度溢价和验证通过率与 {balanced}/{flagship} 比较。", "{fast}를 latency-sensitive classification, extraction, routing, agent substep에 사용하고 speed premium과 validation rate를 {balanced}/{flagship}과 비교합니다."),
    keywords: ll(["{fast} api", "fast {provider} api", "cheap {provider} model", "{fast} batch tasks"], ["{fast} api", "быстрый {provider} api", "дешёвая модель {provider}", "{fast} массовые задачи"], ["{fast} api", "快速 {provider} api", "低价 {provider} 模型", "{fast} 批量任务"], ["{fast} api", "빠른 {provider} api", "저렴한 {provider} 모델", "{fast} 대량 작업"]),
    dek: l("A fast tier earns its place when lower latency matters enough to offset its settled price. Keep schemas, validation and escalation around it so speed does not become silent quality loss.", "Fast tier оправдан, когда меньшая latency окупает его settled price. Добавьте schema, validation и escalation, чтобы скорость не стала скрытой потерей качества.", "只有较低延迟足以抵消结算价格时，快速层级才值得使用；应配套 schema、验证与升级，避免速度变成隐性质量损失。", "fast tier는 낮은 latency가 settled price를 상쇄할 만큼 중요할 때 가치가 있습니다. schema, validation, escalation으로 속도가 조용한 품질 저하가 되지 않게 합니다."),
    focus: ll(["Send bounded, latency-sensitive classification, extraction, tagging, routing and formatting tasks to {fast}.", "Validate structured results locally and retry a failed validation once on {balanced}, not in an unbounded loop.", "Measure end-to-end latency, validation failures, retries and settled cost; a faster model is not automatically the cheapest."], ["Отправляйте в {fast} ограниченные latency-sensitive classification, extraction, tagging, routing и formatting задачи.", "Проверяйте structured result локально и после failed validation один раз переходите на {balanced}, без бесконечного retry loop.", "Измеряйте end-to-end latency, validation failures, retries и settled cost: более быстрая модель не обязательно дешевле."], ["将有边界且延迟敏感的分类、提取、标注、路由和格式化任务发送给 {fast}。", "在本地验证结构化结果；验证失败后仅升级一次到 {balanced}，不要无限重试。", "测量端到端延迟、验证失败、重试与结算成本；更快的模型并不必然最便宜。"], ["bounded latency-sensitive classification, extraction, tagging, routing, formatting 작업을 {fast}로 보냅니다.", "structured result를 로컬 검증하고 실패하면 무제한 loop 대신 {balanced}로 한 번만 올립니다.", "end-to-end latency, validation failure, retry, settled cost를 측정하며 빠른 모델이 자동으로 가장 싸지는 않습니다."]),
  },
];

function integrationTopic(
  id: string,
  slugSuffix: string,
  toolName: string,
  focus: L10nList,
): TopicSpec {
  return {
    id,
    kind: "tool",
    cluster: "integrate",
    slug: (p) => `${p.id}-api-${slugSuffix}`,
    title: l(`${toolName} with the {provider} API`, `${toolName} с {provider} API`, `${toolName} 接入 {provider} API`, `${toolName}에서 {provider} API 사용`),
    description: l(`Connect ${toolName} to {provider} through apiToken.sale with the correct endpoint, namespaced model ID, authentication and a reproducible verification request.`, `Подключите ${toolName} к {provider} через apiToken.sale: правильный endpoint, namespaced model ID, auth и воспроизводимый verification request.`, `通过 apiToken.sale 将 ${toolName} 连接到 {provider}：正确端点、命名空间 model ID、鉴权与可复现验证请求。`, `올바른 endpoint, namespaced model ID, 인증, 재현 가능한 verification request로 ${toolName}를 apiToken.sale의 {provider}에 연결합니다.`),
    keywords: ll([`${toolName.toLowerCase()} {provider} api`, `{provider} api ${toolName.toLowerCase()}`, `${toolName.toLowerCase()} custom base url`, `${toolName.toLowerCase()} api key`], [`${toolName.toLowerCase()} {provider} api`, `{provider} api ${toolName.toLowerCase()}`, `${toolName.toLowerCase()} custom base url`, `${toolName.toLowerCase()} api ключ`], [`${toolName.toLowerCase()} {provider} api`, `{provider} api ${toolName.toLowerCase()}`, `${toolName.toLowerCase()} 自定义 base url`, `${toolName.toLowerCase()} api 密钥`], [`${toolName.toLowerCase()} {provider} api`, `{provider} api ${toolName.toLowerCase()}`, `${toolName.toLowerCase()} custom base url`, `${toolName.toLowerCase()} api key`]),
    dek: l(`${toolName} works through either the provider-native protocol or the universal OpenAI-compatible lane. The safe setup uses a model returned by the live catalog and fails visibly on unsupported controls.`, `${toolName} работает через provider-native protocol или универсальный OpenAI-compatible lane. Безопасная настройка использует модель из live catalog и явно падает на unsupported controls.`, `${toolName} 可通过提供商原生协议或通用 OpenAI 兼容通道工作。安全配置使用实时目录返回的模型，并让不支持的控制项明确失败。`, `${toolName}는 provider-native protocol 또는 universal OpenAI-compatible lane으로 동작합니다. 안전한 설정은 live catalog 모델을 사용하고 unsupported control을 명확히 실패시킵니다.`),
    focus,
  };
}

const toolTopics: TopicSpec[] = [
  integrationTopic("cursor", "key-for-cursor", "Cursor", ll(
    ["Choose Cursor's OpenAI-compatible/custom provider, set the base URL to https://router.apitoken.sale/v1 and use the sk-pool key as Bearer auth.", "Select the namespaced ID for {provider} from {catalog}; do not enter a marketing model name that discovery did not return.", "Disable or remove provider-specific options the target model rejects with 400 instead of assuming the client silently ignores them."],
    ["Выберите в Cursor OpenAI-compatible/custom provider, задайте base URL https://router.apitoken.sale/v1 и используйте sk-pool key как Bearer auth.", "Выберите namespaced ID {provider} из {catalog}; не вводите marketing name, которого нет в discovery.", "Отключите provider-specific options, которые модель отклоняет с 400, вместо ожидания silent ignore."],
    ["在 Cursor 选择 OpenAI-compatible/custom provider，将 base URL 设为 https://router.apitoken.sale/v1，并用 sk-pool 密钥进行 Bearer 鉴权。", "从 {catalog} 选择 {provider} 的命名空间 ID，不要输入目录未返回的营销名称。", "移除目标模型以 400 拒绝的 provider-specific 选项，不要假设客户端会静默忽略。"],
    ["Cursor에서 OpenAI-compatible/custom provider를 선택하고 base URL을 https://router.apitoken.sale/v1로, sk-pool key를 Bearer auth로 설정합니다.", "{catalog}에서 {provider} namespaced ID를 선택하고 discovery에 없는 marketing name을 입력하지 않습니다.", "client가 무시한다고 가정하지 말고 target model이 400으로 거부하는 provider-specific option을 제거합니다."],
  )),
  integrationTopic("vscode", "for-vs-code", "VS Code (Cline and Continue)", ll(
    ["Use the OpenAI-compatible provider in Cline or Continue with https://router.apitoken.sale/v1; Claude-only native settings are not required for {provider}.", "Pin a namespaced model and keep the API key in the extension secret store or an environment variable, never workspace JSON committed to git.", "Run one small chat, one tool call if needed, and inspect the dashboard model/usage before enabling autonomous mode."],
    ["Используйте OpenAI-compatible provider в Cline или Continue с https://router.apitoken.sale/v1; Claude-only native settings для {provider} не нужны.", "Закрепите namespaced model, а ключ храните в secret store расширения или env, не в workspace JSON в git.", "Запустите маленький chat и при необходимости tool call, затем проверьте model/usage до autonomous mode."],
    ["在 Cline 或 Continue 中使用 OpenAI-compatible provider 与 https://router.apitoken.sale/v1；{provider} 不需要 Claude-only 原生设置。", "固定命名空间模型，把 API 密钥放在扩展 secret store 或环境变量中，不要提交到 workspace JSON。", "先运行小型聊天和必要的 tool call，再检查仪表板 model/usage，之后才启用自主模式。"],
    ["Cline 또는 Continue에서 https://router.apitoken.sale/v1 OpenAI-compatible provider를 사용하며 {provider}에는 Claude-only native 설정이 필요 없습니다.", "namespaced model을 고정하고 API key는 extension secret store나 env에 보관하며 git workspace JSON에 넣지 않습니다.", "autonomous mode 전 작은 chat과 필요 시 tool call을 실행하고 dashboard model/usage를 확인합니다."],
  )),
  integrationTopic("cursor-no-direct", "in-cursor-without-direct-account", "Cursor without a direct vendor account", ll(
    ["Create only an apiToken.sale account and key; Cursor authenticates to the custom endpoint rather than to {direct}.", "Use https://router.apitoken.sale/v1 and the provider namespace returned by {catalog} so Cursor cannot accidentally select its bundled vendor route.", "Cursor product features and model API billing are separate: a custom model key does not unlock unrelated paid editor features."],
    ["Создайте только аккаунт и ключ apiToken.sale: Cursor авторизуется в custom endpoint, а не в {direct}.", "Используйте https://router.apitoken.sale/v1 и namespace из {catalog}, чтобы Cursor не выбрал встроенный vendor route.", "Функции Cursor и billing model API разделены: custom key не открывает другие платные функции редактора."],
    ["只需创建 apiToken.sale 账户和密钥；Cursor 向自定义端点鉴权，而不是登录 {direct}。", "使用 https://router.apitoken.sale/v1 和 {catalog} 返回的 provider namespace，避免 Cursor 意外选择内置厂商路由。", "Cursor 产品功能与模型 API 计费相互独立；自定义模型密钥不会解锁其他付费编辑器功能。"],
    ["apiToken.sale 계정과 key만 만들며 Cursor는 {direct}가 아니라 custom endpoint에 인증합니다.", "https://router.apitoken.sale/v1와 {catalog}의 provider namespace를 사용해 Cursor가 bundled vendor route를 잘못 선택하지 않게 합니다.", "Cursor product feature와 model API billing은 별개이며 custom key가 다른 유료 editor 기능을 열지는 않습니다."],
  )),
  integrationTopic("sdk", "sdk-custom-base-url", "Official SDK / custom base URL", ll(
    ["Use {sdk} when it supports {protocol}; set its base URL to {base} and pass the key through {auth}.", "Keep the SDK's request and response types intact on the native lane; use the universal OpenAI-compatible SDK only when the client has no native adapter.", "Confirm the SDK does not append a duplicate /v1 or /v1beta prefix and does not retry non-idempotent streamed turns after the first byte."],
    ["Используйте {sdk} с {protocol}, задайте base URL {base} и передавайте ключ через {auth}.", "Сохраняйте SDK request/response types на native lane; universal OpenAI SDK нужен только клиентам без native adapter.", "Проверьте, что SDK не дублирует /v1 или /v1beta и не retry streamed turn после первого байта."],
    ["在支持 {protocol} 时使用 {sdk}，base URL 设置为 {base}，并通过 {auth} 传递密钥。", "原生通道保持 SDK 请求与响应类型；仅在客户端没有原生适配器时使用通用 OpenAI 兼容 SDK。", "确认 SDK 不会重复追加 /v1 或 /v1beta，也不会在首字节后重试非幂等流式 turn。"],
    ["{protocol}을 지원할 때 {sdk}를 사용하고 base URL을 {base}로, key 전달을 {auth}로 설정합니다.", "native lane에서는 SDK request/response type을 유지하고 native adapter가 없을 때만 universal OpenAI-compatible SDK를 사용합니다.", "SDK가 /v1 또는 /v1beta를 중복 추가하거나 first byte 이후 non-idempotent streamed turn을 retry하지 않는지 확인합니다."],
  )),
  integrationTopic("langchain", "langchain", "LangChain", ll(
    ["Use the provider-native LangChain integration when it exposes a configurable endpoint; otherwise use ChatOpenAI against the universal /v1/chat/completions lane.", "Address {provider} by its namespaced catalog ID and keep tool/schema controls inside the subset documented for that model.", "Capture terminal usage from callbacks and compare it with the dashboard rather than estimating cost from character counts."],
    ["Используйте provider-native LangChain integration с configurable endpoint; иначе ChatOpenAI через universal /v1/chat/completions.", "Адресуйте {provider} namespaced ID из каталога и оставляйте tool/schema controls в поддерживаемом subset.", "Получайте terminal usage через callbacks и сверяйте с дашбордом вместо оценки по символам."],
    ["若 provider-native LangChain integration 支持可配置端点则优先使用，否则通过通用 /v1/chat/completions 使用 ChatOpenAI。", "使用目录中的 {provider} 命名空间 ID，并把 tool/schema 控制限制在模型文档支持的子集。", "通过 callback 捕获终态 usage 并与仪表板核对，不要按字符数估算成本。"],
    ["configurable endpoint가 있는 provider-native LangChain integration을 우선 사용하고 없으면 universal /v1/chat/completions의 ChatOpenAI를 사용합니다.", "catalog의 {provider} namespaced ID를 사용하고 tool/schema control은 모델이 문서화한 subset으로 제한합니다.", "문자 수로 비용을 추정하지 말고 callback terminal usage를 dashboard와 비교합니다."],
  )),
  integrationTopic("litellm", "litellm", "LiteLLM", ll(
    ["Point LiteLLM's OpenAI-compatible route at https://router.apitoken.sale/v1 and keep the full namespaced model ID instead of applying LiteLLM's vendor prefix twice.", "Disable LiteLLM fallback while validating one model so errors, usage and latency remain attributable.", "If you later add fallback, retry only before delivery and keep one customer billing identity across attempts."],
    ["Направьте OpenAI-compatible route LiteLLM на https://router.apitoken.sale/v1 и не добавляйте vendor prefix дважды к namespaced model ID.", "Отключите fallback LiteLLM при проверке одной модели, чтобы errors, usage и latency были атрибутируемы.", "При последующем fallback retry допустим только до delivery, с одним billing identity для всех attempts."],
    ["将 LiteLLM 的 OpenAI-compatible route 指向 https://router.apitoken.sale/v1，保留完整命名空间 model ID，不要重复添加 vendor prefix。", "验证单模型时关闭 LiteLLM fallback，使错误、usage 与延迟可归因。", "之后如启用 fallback，只能在交付前重试，并让所有 attempt 共用一个客户计费身份。"],
    ["LiteLLM OpenAI-compatible route를 https://router.apitoken.sale/v1로 설정하고 namespaced model ID에 vendor prefix를 두 번 붙이지 않습니다.", "한 모델 검증 중 LiteLLM fallback을 꺼 errors, usage, latency 귀속을 명확히 합니다.", "나중에 fallback을 추가하면 delivery 전만 retry하고 attempt 전체에서 하나의 customer billing identity를 유지합니다."],
  )),
  integrationTopic("aider", "aider", "Aider", ll(
    ["Configure Aider as an OpenAI-compatible client with https://router.apitoken.sale/v1 and a namespaced {provider} model.", "Set separate editor and weak models only after each ID appears in {catalog}; cheap submodels can reduce repository-wide edit cost.", "Start with a clean diff and a lifetime key cap because autonomous edit/test loops can multiply token use quickly."],
    ["Настройте Aider как OpenAI-compatible client на https://router.apitoken.sale/v1 с namespaced моделью {provider}.", "Задавайте editor/weak models только после появления каждого ID в {catalog}; дешёвые submodels снижают цену repo edits.", "Начинайте с clean diff и lifetime cap ключа: autonomous edit/test loops быстро умножают token usage."],
    ["把 Aider 配置为 OpenAI-compatible client，使用 https://router.apitoken.sale/v1 与 {provider} 命名空间模型。", "仅在每个 ID 出现在 {catalog} 后设置 editor/weak model；低价子模型可降低全仓库编辑成本。", "从 clean diff 和密钥终身上限开始，因为自主编辑/测试循环会迅速放大 token usage。"],
    ["Aider를 https://router.apitoken.sale/v1와 namespaced {provider} model을 쓰는 OpenAI-compatible client로 설정합니다.", "각 ID가 {catalog}에 나온 뒤 editor/weak model을 설정하며 저렴한 submodel로 repository edit 비용을 낮춥니다.", "autonomous edit/test loop가 token usage를 빠르게 늘리므로 clean diff와 key lifetime cap으로 시작합니다."],
  )),
  integrationTopic("roo-code", "roo-code", "Roo Code", ll(
    ["Select OpenAI Compatible in Roo Code, set https://router.apitoken.sale/v1 and paste the key into the extension secret field.", "Use the exact namespaced {provider} ID and begin with conservative context/output limits before enabling browser or shell tools.", "A 400 unsupported_parameter is a configuration signal: remove that option instead of switching to a hidden provider preset."],
    ["Выберите OpenAI Compatible в Roo Code, задайте https://router.apitoken.sale/v1 и сохраните ключ в secret field расширения.", "Используйте точный namespaced ID {provider} и начните с консервативных context/output limits до browser/shell tools.", "400 unsupported_parameter — сигнал конфигурации: удалите option, не переходя на скрытый provider preset."],
    ["在 Roo Code 选择 OpenAI Compatible，设置 https://router.apitoken.sale/v1，并把密钥存入扩展 secret 字段。", "使用精确的 {provider} 命名空间 ID，在启用浏览器或 shell tool 前采用保守的 context/output 上限。", "400 unsupported_parameter 是配置提示；应移除该选项，而不是切换到隐藏 provider preset。"],
    ["Roo Code에서 OpenAI Compatible을 선택하고 https://router.apitoken.sale/v1와 extension secret field의 key를 설정합니다.", "정확한 namespaced {provider} ID를 사용하고 browser/shell tool 전 보수적인 context/output limit로 시작합니다.", "400 unsupported_parameter는 설정 신호입니다. 숨은 provider preset으로 바꾸지 말고 해당 option을 제거합니다."],
  )),
  integrationTopic("vscode-agents", "vscode-ai-agents", "Free VS Code AI agents", ll(
    ["Install Cline, Continue or Roo Code and select an OpenAI-compatible provider rather than paying for an editor-bundled model plan.", "Use {economy} for low-cost deterministic steps, {balanced} for daily coding and {flagship} only for hard reviews or architecture.", "Separate extension keys by project or environment so one leaked workspace credential can be revoked without replacing every client."],
    ["Установите Cline, Continue или Roo Code и выберите OpenAI-compatible provider вместо bundled model plan редактора.", "Используйте {economy} для дешёвых шагов, {balanced} для daily coding и {flagship} только для сложного review/architecture.", "Разделяйте extension keys по проектам или environments, чтобы утечка не требовала замены всех клиентов."],
    ["安装 Cline、Continue 或 Roo Code，选择 OpenAI-compatible provider，无需购买编辑器捆绑模型套餐。", "低成本确定性步骤使用 {economy}，日常编程使用 {balanced}，仅复杂审查或架构使用 {flagship}。", "按项目或环境拆分扩展密钥，单个 workspace 凭据泄露时无需替换所有客户端。"],
    ["Cline, Continue, Roo Code를 설치하고 editor bundled model plan 대신 OpenAI-compatible provider를 선택합니다.", "저렴한 deterministic step은 {economy}, daily coding은 {balanced}, 어려운 review/architecture만 {flagship}을 사용합니다.", "project/environment별 extension key를 분리해 workspace credential 하나가 유출돼도 모든 client를 교체하지 않게 합니다."],
  )),
];

function comparisonTopic(
  id: string,
  competitorSlug: string,
  competitor: string,
  focus: L10nList,
): TopicSpec {
  return {
    id,
    kind: "compare",
    cluster: "compare",
    slug: (p) => `${p.id}-api-vs-${competitorSlug}`,
    title: l(`apiToken.sale vs ${competitor} for the {provider} API`, `apiToken.sale и ${competitor}: сравнение для {provider} API`, `apiToken.sale 与 ${competitor}：{provider} API 对比`, `apiToken.sale와 ${competitor}: {provider} API 비교`),
    description: l(`Compare apiToken.sale with ${competitor} for {provider}: account requirements, protocol fidelity, model discovery, billing, operations and the cases where each route fits.`, `Сравните apiToken.sale и ${competitor} для {provider}: аккаунты, точность протокола, каталог моделей, биллинг, эксплуатацию и подходящие сценарии.`, `比较 apiToken.sale 与 ${competitor} 的 {provider} 方案：账户要求、协议保真、模型发现、结算、运维及各自适用场景。`, `{provider} 사용 시 apiToken.sale와 ${competitor}의 계정 요건, protocol fidelity, model discovery, billing, 운영 및 적합한 상황을 비교합니다.`),
    keywords: ll([`apitoken vs ${competitorSlug}`, `{provider} api ${competitorSlug}`, `${competitorSlug} alternative`, `best {provider} api gateway`], [`apitoken или ${competitorSlug}`, `{provider} api ${competitorSlug}`, `альтернатива ${competitorSlug}`, `лучший шлюз {provider} api`], [`apitoken 对比 ${competitorSlug}`, `{provider} api ${competitorSlug}`, `${competitorSlug} 替代方案`, `最佳 {provider} api 网关`], [`apitoken vs ${competitorSlug}`, `{provider} api ${competitorSlug}`, `${competitorSlug} 대안`, `최고의 {provider} api gateway`]),
    dek: l(`The right answer depends on what you need the intermediary to own. Compare the complete path—credentials, wire format, live catalog, settled usage and failure handling—not one headline price.`, `Выбор зависит от того, какую часть пути должен взять на себя посредник. Сравнивайте credentials, wire format, live catalog, settled usage и обработку ошибок, а не одну цену.`, `正确选择取决于你希望中间层负责什么。应比较完整链路——凭据、wire format、实时目录、结算 usage 与故障处理，而非单一报价。`, `올바른 선택은 중간 계층이 무엇을 책임져야 하는지에 달렸습니다. headline 가격 하나가 아니라 credential, wire format, live catalog, settled usage, failure handling 전체를 비교합니다.`),
    focus,
  };
}

const comparisonTopics: TopicSpec[] = [
  comparisonTopic("direct-provider", "direct-provider", "{direct}", ll(
    ["Direct access gives you the vendor relationship and native console; apiToken.sale removes the separate vendor billing account and funds {provider} from one prepaid balance.", "Verify protocol parity on {protocol}, then compare the final dashboard charge with the direct provider's complete rate card.", "Choose direct for vendor enterprise contracts or features outside the live catalog; choose the gateway for prepaid access, one key and simpler regional payment."],
    ["Direct даёт отношения с вендором и нативную консоль; apiToken.sale убирает отдельный billing account и оплачивает {provider} из общего предоплаченного баланса.", "Проверьте protocol parity через {protocol}, затем сравните итоговое списание с полной direct rate card.", "Direct подходит для enterprise-контрактов и функций вне live catalog; gateway — для предоплаты, одного ключа и более простого регионального платежа."],
    ["直连提供厂商关系与原生控制台；apiToken.sale 无需单独厂商结算账户，用一个预付余额支付 {provider}。", "先在 {protocol} 上验证协议一致性，再将仪表板最终扣费与直连完整价目表比较。", "需要厂商企业合同或实时目录之外功能时选直连；需要预付、单密钥与更简单地区支付时选网关。"],
    ["direct access는 vendor 관계와 native console을 제공하고 apiToken.sale는 별도 vendor billing 계정 없이 하나의 선불 잔액으로 {provider}를 결제합니다.", "{protocol}에서 protocol parity를 검증한 뒤 최종 dashboard charge를 direct provider의 전체 rate card와 비교합니다.", "vendor enterprise 계약이나 live catalog 밖 기능은 direct, 선불·하나의 key·간단한 지역 결제는 gateway가 적합합니다."],
  )),
  comparisonTopic("openrouter", "openrouter", "OpenRouter", ll(
    ["OpenRouter optimizes for a broad normalized marketplace; apiToken.sale keeps native provider lanes and also exposes a universal OpenAI-compatible catalog for cross-provider clients.", "Compare the exact model ID, supported controls and terminal usage on the route you will deploy—not just whether both services list the same model family.", "Test streaming, tool calls and errors because abstraction depth matters most at protocol edges."],
    ["OpenRouter ориентирован на широкий нормализованный marketplace; apiToken.sale сохраняет нативные lanes и даёт universal OpenAI-compatible catalog для общих клиентов.", "Сравнивайте точный model ID, controls и terminal usage на реальном route, а не только наличие одной model family.", "Проверьте streaming, tool calls и ошибки: глубина абстракции сильнее всего видна на краях протокола."],
    ["OpenRouter 面向广泛的归一化模型市场；apiToken.sale 保留提供商原生通道，同时为跨提供商客户端提供通用 OpenAI-compatible 目录。", "比较实际部署路由上的精确 model ID、支持控制项和终态 usage，而不只是两边是否列出同一模型系列。", "测试 streaming、tool call 与错误，因为抽象差异最容易在协议边缘暴露。"],
    ["OpenRouter는 넓은 normalized marketplace에 초점을 두고 apiToken.sale는 native provider lane과 cross-provider client용 universal OpenAI-compatible catalog를 함께 제공합니다.", "같은 model family 목록만 보지 말고 실제 배포 route의 정확한 model ID, 지원 control, terminal usage를 비교합니다.", "추상화 깊이가 protocol edge에서 드러나므로 streaming, tool call, error를 테스트합니다."],
  )),
  comparisonTopic("proxyapi", "proxyapi", "ProxyAPI", ll(
    ["Check who owns the key, balance and upstream account, and whether per-request usage is authoritative rather than estimated.", "Compare key-scoped model discovery, native {protocol} behavior and explicit errors for unsupported parameters.", "Evaluate support and refund terms with a small paid request before moving production traffic."],
    ["Проверьте, кто владеет ключом, балансом и upstream account, а usage запроса является authoritative, а не оценочным.", "Сравните key-scoped discovery, нативное поведение {protocol} и явные ошибки unsupported parameters.", "Оцените поддержку и refund terms на маленьком платном запросе до переноса production-трафика."],
    ["确认密钥、余额与 upstream 账户的责任方，并确认每请求 usage 是权威数据而非估算。", "比较密钥范围模型发现、原生 {protocol} 行为及对不支持参数的明确错误。", "迁移生产流量前，用小额付费请求评估支持与退款条款。"],
    ["key, 잔액, upstream account의 소유 주체와 요청별 usage가 추정이 아닌 authoritative 값인지 확인합니다.", "key-scoped model discovery, native {protocol} 동작, unsupported parameter의 명시적 오류를 비교합니다.", "production traffic 이전에 작은 유료 요청으로 support와 refund 조건을 평가합니다."],
  )),
  comparisonTopic("portkey", "portkey", "Portkey", ll(
    ["Portkey is primarily an observability and routing layer for provider credentials you bring; apiToken.sale issues the usable key and prepaid provider balance.", "They can be layered when Portkey can target the relevant custom endpoint—keep one request ID across both systems for reconciliation.", "Avoid duplicate retries and fallback policies; only one layer should own each recovery decision."],
    ["Portkey прежде всего даёт observability и routing для ваших provider credentials; apiToken.sale выпускает рабочий ключ и предоплаченный баланс.", "Их можно сочетать при поддержке custom endpoint; сохраняйте единый request ID для сверки двух систем.", "Не дублируйте retry и fallback policies: каждое решение о восстановлении должно принадлежать одному слою."],
    ["Portkey 主要为自带提供商凭据提供可观测性与路由；apiToken.sale 则签发可用密钥并提供预付余额。", "若 Portkey 支持相应 custom endpoint，可将两者叠加；应在两套系统间保留同一 request ID 便于核对。", "不要重复 retry 与 fallback 策略；每个恢复决策只能由一层负责。"],
    ["Portkey는 주로 사용자가 가져온 provider credential의 observability와 routing을 담당하고 apiToken.sale는 실제 key와 선불 provider 잔액을 제공합니다.", "Portkey가 해당 custom endpoint를 지원하면 함께 사용할 수 있으며 두 시스템에 같은 request ID를 유지합니다.", "retry와 fallback policy를 중복하지 말고 각 recovery 결정은 한 계층만 소유하게 합니다."],
  )),
  comparisonTopic("litellm-proxy", "litellm", "LiteLLM", ll(
    ["LiteLLM is software you operate and connect to separately funded providers; apiToken.sale is a managed endpoint with the key and balance included.", "You can place LiteLLM above the universal lane for internal routing, but must preserve namespaced model IDs and terminal usage.", "Budget for proxy maintenance, secret rotation and retry ownership when comparing total cost."],
    ["LiteLLM — софт, который вы эксплуатируете и подключаете к отдельно оплаченным провайдерам; apiToken.sale — managed endpoint с ключом и балансом.", "LiteLLM можно поставить над universal lane для внутреннего routing, сохранив namespaced IDs и terminal usage.", "При сравнении полной цены учитывайте обслуживание proxy, rotation секретов и владельца retries."],
    ["LiteLLM 是需要自行运维并连接到各自付费提供商的软件；apiToken.sale 是包含密钥与余额的托管端点。", "可将 LiteLLM 放在通用通道之上做内部路由，但必须保留命名空间 model ID 与终态 usage。", "比较总成本时需计入代理维护、secret 轮换与 retry 责任。"],
    ["LiteLLM은 직접 운영하고 별도 결제 provider에 연결하는 software이며 apiToken.sale는 key와 잔액이 포함된 managed endpoint입니다.", "internal routing을 위해 universal lane 위에 LiteLLM을 둘 수 있지만 namespaced model ID와 terminal usage를 보존해야 합니다.", "총비용 비교에는 proxy 유지보수, secret rotation, retry ownership을 포함합니다."],
  )),
];

function operationsTopic(
  id: string,
  cluster: LearnCluster,
  slugSuffix: string,
  title: L10n,
  description: L10n,
  keywords: L10nList,
  dek: L10n,
  focus: L10nList,
  providerExisting?: Partial<Record<ParityProvider, string>>,
): TopicSpec {
  return {
    id,
    kind: "operations",
    cluster,
    slug: (p) => `${p.id}-api-${slugSuffix}`,
    title,
    description,
    keywords,
    dek,
    focus,
    providerExisting,
  };
}

const operationsTopics: TopicSpec[] = [
  operationsTopic("save-tokens", "explain", "save-tokens", l("How to Save Tokens on the {provider} API", "Как экономить токены {provider} API", "如何节省 {provider} API token", "{provider} API token 절약 방법"), l("Reduce {provider} API cost with model routing, bounded context, caching, output caps and terminal-usage measurement instead of fragile character estimates.", "Снижайте цену {provider} API через model routing, ограниченный контекст, caching, output caps и terminal usage вместо оценки по символам.", "通过模型路由、受控上下文、缓存、output 上限和终态 usage 计量降低 {provider} API 成本，而非按字符估算。", "model routing, 제한된 context, caching, output cap, terminal usage 측정으로 {provider} API 비용을 줄이고 문자 수 추정을 피합니다."), ll(["save {provider} api tokens", "reduce {provider} api cost", "{provider} prompt optimization", "{provider} token usage"], ["экономить токены {provider}", "снизить цену {provider} api", "оптимизация prompt {provider}", "usage токенов {provider}"], ["节省 {provider} token", "降低 {provider} api 成本", "{provider} prompt 优化", "{provider} token usage"], ["{provider} token 절약", "{provider} api 비용 절감", "{provider} prompt 최적화", "{provider} token usage"]), l("Token savings come from less unnecessary work, not from hiding usage. Establish a measured baseline, change one lever, and compare task quality with the settled bill.", "Экономия появляется из устранения лишней работы, а не из скрытия usage. Зафиксируйте baseline, меняйте по одному параметру и сравнивайте качество с итоговым списанием.", "节省来自减少无效工作，而不是隐藏 usage。先建立可测基线，每次只改一个变量，并把任务质量与结算账单比较。", "token 절감은 usage를 숨기는 것이 아니라 불필요한 작업을 줄이는 데서 나옵니다. baseline을 측정하고 한 번에 한 lever만 바꿔 품질과 정산 비용을 비교합니다."), ll(["Route deterministic substeps to {fast}, everyday work to {balanced}, and use {flagship} only after an eval shows a quality gain.", "Trim stale conversation turns and repeated tool output before every call; preserve only state needed for the next decision.", "Use {cacheMode}, cap output and inspect terminal usage because input and output have different economic weight."], ["Детерминированные шаги отправляйте в {fast}, обычные — в {balanced}, а {flagship} используйте только после eval с приростом качества.", "Перед каждым вызовом удаляйте устаревшие turns и повторный tool output, оставляя только состояние для следующего решения.", "Используйте {cacheMode}, ограничивайте output и проверяйте terminal usage: input и output имеют разную цену."], ["确定性子步骤使用 {fast}，日常任务使用 {balanced}，只有评测证明质量提升时才用 {flagship}。", "每次调用前删除过期对话 turn 与重复 tool output，只保留下一个决策所需状态。", "使用 {cacheMode}，限制 output 并检查终态 usage，因为 input 与 output 的经济权重不同。"], ["deterministic substep은 {fast}, 일상 작업은 {balanced}, eval로 품질 향상이 확인된 경우만 {flagship}을 사용합니다.", "호출 전 오래된 turn과 반복 tool output을 제거하고 다음 결정에 필요한 상태만 유지합니다.", "{cacheMode}을 사용하고 output을 제한하며 input/output의 경제적 비중이 다르므로 terminal usage를 확인합니다."])),
  operationsTopic("billing", "explain", "billing", l("How {provider} API Billing Works", "Как работает биллинг {provider} API", "{provider} API 如何计费", "{provider} API billing 방식"), l("Understand prepaid {provider} billing from provider usage legs to the discounted dashboard charge, balance checks, failed requests and reconciliation.", "Разберите предоплаченный billing {provider}: usage legs провайдера, скидку, списание в дашборде, balance checks, failed requests и сверку.", "了解 {provider} 预付费结算：提供商 usage 分类、折扣后仪表板扣费、余额检查、失败请求与对账。", "provider usage leg부터 할인된 dashboard charge, balance check, failed request, reconciliation까지 {provider} 선불 billing을 설명합니다."), ll(["{provider} api billing", "{provider} token charges", "prepaid {provider} api", "{provider} usage cost"], ["биллинг {provider} api", "списания {provider} токенов", "предоплаченный {provider} api", "стоимость usage {provider}"], ["{provider} api 计费", "{provider} token 扣费", "预付费 {provider} api", "{provider} usage 成本"], ["{provider} api billing", "{provider} token charge", "선불 {provider} api", "{provider} usage 비용"]), l("A request is funded before it starts and settled from authoritative terminal usage. The useful unit is the complete request ledger, not a rough token estimate made before delivery.", "Запрос получает funding до старта и рассчитывается по authoritative terminal usage. Полезная единица — полный ledger запроса, а не приблизительная оценка до ответа.", "请求开始前完成资金检查，并按权威终态 usage 结算。应以完整请求账本为单位，而不是交付前的粗略 token 估算。", "요청은 시작 전 funding을 확인하고 authoritative terminal usage로 정산됩니다. 유용한 단위는 응답 전 대략적 token 추정이 아니라 전체 request ledger입니다."), ll(["The platform meters fresh input, cached input, output and any model-specific usage legs reported by {provider}.", "Your account discount is applied to settled provider spend and the result is deducted from one prepaid balance shared across providers.", "Reconcile request ID, model ID, usage buckets and final charge; a transport failure with no delivered output must not be inferred from HTTP status alone."], ["Платформа учитывает fresh input, cached input, output и model-specific usage legs, возвращённые {provider}.", "Скидка аккаунта применяется к settled provider spend, результат списывается с общего предоплаченного баланса.", "Сверяйте request ID, model ID, usage buckets и итог: судьбу списания нельзя угадывать только по HTTP status."], ["平台计量 {provider} 报告的 fresh input、cached input、output 及模型特定 usage 分类。", "账户折扣作用于结算后的提供商支出，结果从跨提供商共享的预付余额中扣除。", "核对 request ID、model ID、usage bucket 与最终扣费；不能只凭 HTTP 状态推断传输失败是否计费。"], ["platform은 {provider}가 보고한 fresh input, cached input, output 및 model-specific usage leg를 측정합니다.", "계정 할인은 settled provider spend에 적용되고 결과는 provider 공용 선불 잔액에서 차감됩니다.", "request ID, model ID, usage bucket, final charge를 대조하며 HTTP status만으로 transport failure의 charge를 추정하지 않습니다."])),
  operationsTopic("activation", "explain", "activation-time", l("How Long {provider} API Activation Takes", "Сколько занимает активация {provider} API", "{provider} API 激活需要多久", "{provider} API 활성화 시간"), l("See what is immediate in {provider} API setup, what depends on payment confirmation and how to prove the key, catalog and first request are active.", "Узнайте, что в {provider} API включается сразу, что зависит от подтверждения платежа и как проверить key, catalog и первый запрос.", "了解 {provider} API 设置中哪些步骤立即完成、哪些取决于付款确认，以及如何验证密钥、目录与首个请求已激活。", "{provider} API 설정에서 즉시 되는 것, 결제 확인에 달린 것, key·catalog·첫 요청 활성화를 증명하는 방법을 설명합니다."), ll(["{provider} api activation time", "instant {provider} api key", "activate {provider} api", "{provider} api setup time"], ["время активации {provider} api", "мгновенный ключ {provider}", "активировать {provider} api", "время настройки {provider}"], ["{provider} api 激活时间", "即时 {provider} api 密钥", "激活 {provider} api", "{provider} api 设置时间"], ["{provider} api 활성화 시간", "즉시 {provider} api key", "{provider} api 활성화", "{provider} api 설정 시간"]), l("Account and key issuance are immediate. Paid traffic begins after usable balance appears; the live catalog and a minimal response are the operational proof.", "Аккаунт и ключ выпускаются сразу. Платный трафик начинается после появления баланса; operational proof — live catalog и минимальный ответ.", "账户与密钥立即签发。可用余额到账后才能运行付费流量；实时目录和最小响应才是运行证明。", "계정과 key는 즉시 발급됩니다. 사용 가능한 잔액 반영 후 유료 traffic을 시작하며 live catalog와 최소 응답이 운영 증거입니다."), ll(["Create the account and named key immediately; no separate {direct} organization review is part of this flow.", "Payment confirmation time depends on the selected card or cryptocurrency processor, so wait for dashboard balance rather than a receipt alone.", "Call {catalog} and send one low-cap request through {protocol}; only then configure long-running clients."], ["Аккаунт и named key создаются сразу, без отдельного review организации {direct}.", "Время подтверждения зависит от card/crypto processor: ждите баланс в дашборде, а не только receipt.", "Вызовите {catalog} и один low-cap запрос через {protocol}; только затем настраивайте долгие клиенты."], ["账户与命名密钥立即创建，不需要单独的 {direct} 组织审核。", "确认时间取决于银行卡或加密货币处理商，因此应等待仪表板余额，而不只看收据。", "调用 {catalog} 并通过 {protocol} 发送一个低上限请求，之后再配置长时间运行客户端。"], ["별도 {direct} 조직 review 없이 계정과 named key가 즉시 생성됩니다.", "결제 확인 시간은 card/crypto processor에 따라 다르므로 영수증만 보지 말고 dashboard 잔액을 기다립니다.", "{catalog}와 {protocol} low-cap 요청 하나를 실행한 후 장기 client를 설정합니다."])),
  operationsTopic("countries", "explain", "supported-countries", l("{provider} API Supported Countries and Access", "Поддерживаемые страны и доступ к {provider} API", "{provider} API 支持国家与访问方式", "{provider} API 지원 국가 및 접근"), l("Understand the difference between apiToken.sale account availability, payment methods, lawful network access and the direct vendor's country list for {provider}.", "Различайте доступность аккаунта apiToken.sale, способы оплаты, законный network access и список стран direct-вендора {provider}.", "区分 apiToken.sale 账户可用性、付款方式、合法网络访问与 {provider} 直连厂商国家列表。", "apiToken.sale 계정 가용성, 결제 방식, 합법적 network access, {provider} direct vendor 국가 목록의 차이를 설명합니다."), ll(["{provider} api supported countries", "{provider} api country availability", "international {provider} api", "{provider} api regional access"], ["страны {provider} api", "доступность {provider} по странам", "международный {provider} api", "региональный доступ {provider}"], ["{provider} api 支持国家", "{provider} api 国家可用性", "国际 {provider} api", "{provider} api 地区访问"], ["{provider} api 지원 국가", "{provider} api 국가 가용성", "국제 {provider} api", "{provider} api 지역 접근"]), l("A gateway changes account and payment dependencies; it does not erase law, sanctions, network policy or the live availability of a provider pool.", "Gateway меняет зависимости аккаунта и оплаты, но не отменяет законы, санкции, network policy и live-доступность provider pool.", "网关改变账户与付款依赖，但不会消除法律、制裁、网络政策或提供商池的实时可用性。", "gateway는 계정과 결제 의존성을 바꾸지만 법률, 제재, network policy, provider pool의 live 가용성을 없애지 않습니다."), ll(["Check whether you can lawfully reach apiToken.sale and use an offered payment method from your location.", "Do not copy the direct {company} country list onto the independent gateway; the account and checkout boundaries are different.", "Confirm {catalog} with your own key immediately before deployment because operational model availability can change."], ["Проверьте законность доступа к apiToken.sale и доступный способ оплаты из вашей локации.", "Не переносите список стран {company} на независимый gateway: границы аккаунта и checkout различаются.", "Перед deployment проверьте {catalog} своим ключом: operational availability моделей может меняться."], ["确认所在位置可合法访问 apiToken.sale 并能使用其提供的付款方式。", "不要把 {company} 直连国家列表复制到独立网关；账户与 checkout 边界不同。", "部署前立即用自己的密钥确认 {catalog}，因为模型运行可用性可能变化。"], ["현재 위치에서 apiToken.sale에 합법적으로 접근하고 제공 결제 방식을 사용할 수 있는지 확인합니다.", "계정과 checkout 경계가 다르므로 direct {company} 국가 목록을 독립 gateway에 그대로 적용하지 않습니다.", "모델 운영 가용성이 바뀔 수 있으므로 배포 직전 자신의 key로 {catalog}를 확인합니다."])),
  operationsTopic("refund", "explain", "refund-policy", l("{provider} API Refund Policy and Balance Questions", "Возвраты и баланс {provider} API", "{provider} API 退款政策与余额问题", "{provider} API 환불 정책 및 잔액"), l("Learn how to document a {provider} API refund request, distinguish unused balance from consumed usage and provide support with a traceable payment and request history.", "Узнайте, как оформить возврат по {provider} API, отличить неиспользованный баланс от consumed usage и передать support проверяемую историю платежа и запросов.", "了解如何提交 {provider} API 退款请求、区分未用余额与已消费 usage，并向支持提供可追踪付款和请求记录。", "{provider} API 환불 요청을 문서화하고 미사용 잔액과 소비 usage를 구분하며 support에 추적 가능한 결제·요청 이력을 제공하는 방법입니다."), ll(["{provider} api refund", "refund {provider} api balance", "unused {provider} api credit", "{provider} api payment support"], ["возврат {provider} api", "вернуть баланс {provider}", "неиспользованный баланс {provider}", "поддержка платежа {provider}"], ["{provider} api 退款", "退还 {provider} api 余额", "未使用 {provider} api 余额", "{provider} api 付款支持"], ["{provider} api 환불", "{provider} api 잔액 환불", "미사용 {provider} api credit", "{provider} api 결제 지원"]), l("Refund review needs evidence. Preserve the payment reference, account identity, request IDs and ledger entries; never send a full API key to support.", "Для возврата нужны доказательства. Сохраните payment reference, account identity, request IDs и ledger; никогда не отправляйте support полный API-ключ.", "退款审核需要证据。保留付款参考、账户身份、request ID 与账本记录；绝不要把完整 API 密钥发送给支持。", "환불 검토에는 증거가 필요합니다. payment reference, account identity, request ID, ledger를 보존하고 support에 전체 API key를 보내지 않습니다."), ll(["Separate unused prepaid balance from requests already settled against authoritative {provider} usage.", "Contact support from the account email with the payment reference, amount, date and the relevant masked key or request IDs.", "The original checkout provider and payment rail can determine timing and reversibility; do not create a chargeback before support can reconcile the ledger."], ["Отделите неиспользованный prepaid balance от запросов, уже рассчитанных по authoritative usage {provider}.", "Пишите с email аккаунта, приложив payment reference, сумму, дату и masked key или request IDs.", "Срок и обратимость зависят от checkout provider и payment rail; не открывайте chargeback до сверки ledger поддержкой."], ["区分未使用预付余额与已按 {provider} 权威 usage 结算的请求。", "使用账户邮箱联系支持，并提供付款参考、金额、日期及相关 masked key 或 request ID。", "退款时间与可逆性取决于原 checkout provider 和付款通道；支持完成账本核对前不要发起拒付。"], ["미사용 선불 잔액과 authoritative {provider} usage로 이미 정산된 요청을 구분합니다.", "계정 email로 payment reference, 금액, 날짜, 관련 masked key 또는 request ID를 제공해 support에 문의합니다.", "original checkout provider와 payment rail이 시기와 가역성을 결정하므로 support ledger 대조 전 chargeback을 만들지 않습니다."])),
];

// Additional operational topics are appended separately so the content matrix
// remains reviewable and no Claude intent collapses into a generic catch-all.
const advancedTopics: TopicSpec[] = [
  operationsTopic("best-coding-model", "compare", "best-model-for-coding", l("Best {provider} Model for Coding", "Лучшая модель {provider} для программирования", "最适合编程的 {provider} 模型", "코딩에 가장 적합한 {provider} 모델"), l("Choose among {flagship}, {balanced} and {fast} for coding by measuring repository-level quality, latency, retries and settled cost instead of relying on one benchmark.", "Выберите {flagship}, {balanced} или {fast} для кода по качеству на репозитории, latency, retries и settled cost, а не по одному benchmark.", "通过仓库级质量、延迟、重试与结算成本在 {flagship}、{balanced} 和 {fast} 中选择，而非依赖单一 benchmark。", "단일 benchmark 대신 repository 수준 품질, latency, retry, settled cost를 측정해 {flagship}, {balanced}, {fast} 중 코딩 모델을 선택합니다."), ll(["best {provider} model for coding", "{provider} coding model comparison", "{flagship} vs {balanced} coding", "cheap {provider} coding api"], ["лучшая модель {provider} для кода", "сравнение coding моделей {provider}", "{flagship} или {balanced} для кода", "дешёвый coding api {provider}"], ["最佳 {provider} 编程模型", "{provider} 编程模型对比", "{flagship} 与 {balanced} 编程", "低价 {provider} 编程 api"], ["최고의 {provider} 코딩 모델", "{provider} 코딩 모델 비교", "{flagship} vs {balanced} 코딩", "저렴한 {provider} 코딩 api"]), l("Coding quality is a workflow property. A model that writes a good patch but needs repeated repair, excessive context or slow tool turns can lose to a cheaper tier on total task cost.", "Качество coding определяется workflow. Модель с хорошим patch, но частыми repairs, лишним context или медленными tools может проиграть дешёвому tier по полной цене задачи.", "编程质量是工作流属性。即使补丁不错，若需要反复修复、过多上下文或缓慢 tool turn，总任务成本也可能输给低价层级。", "코딩 품질은 workflow 속성입니다. 좋은 patch를 써도 반복 repair, 과도한 context, 느린 tool turn이 필요하면 총 작업 비용에서 저렴한 tier보다 불리할 수 있습니다."), ll(["Start with {balanced} for everyday repository work and score compile/test success, review defects and total tokens.", "Use {fast} for search summaries, classification and bounded edits with local validation; escalate failures once.", "Reserve {flagship} for architecture, difficult debugging and high-risk review where your own eval shows a material gain."], ["Начните с {balanced} и измеряйте compile/test success, defects на review и все токены задачи.", "Используйте {fast} для search summaries, classification и ограниченных edits с локальной проверкой; failed case повышайте один раз.", "Оставьте {flagship} для архитектуры, сложной отладки и high-risk review с доказанным eval-приростом."], ["日常仓库任务从 {balanced} 开始，记录编译/测试成功率、审查缺陷和总 token。", "{fast} 用于搜索摘要、分类及可本地验证的有界编辑；失败后只升级一次。", "{flagship} 保留给架构、困难调试与高风险审查，并要求自有评测证明显著提升。"], ["일상 repository 작업은 {balanced}로 시작해 compile/test 성공, review defect, 총 token을 측정합니다.", "{fast}는 search summary, classification, local validation 가능한 bounded edit에 쓰고 실패 시 한 번만 올립니다.", "자체 eval에서 유의미한 향상이 있는 architecture, 어려운 debugging, high-risk review에만 {flagship}을 사용합니다."])),
  operationsTopic("subscription-vs-api", "compare", "subscription-vs-api", l("{provider} Subscription vs API", "Подписка {provider} или API", "{provider} 订阅与 API 对比", "{provider} 구독과 API 비교"), l("Compare a fixed {provider} consumer or coding subscription with prepaid API usage for automation, integrations, team attribution and cost control.", "Сравните фиксированную подписку {provider} с предоплаченным API для automation, интеграций, командной атрибуции и контроля расходов.", "比较固定 {provider} 消费者/编程订阅与预付 API 在自动化、集成、团队归因和成本控制方面的差异。", "고정 {provider} consumer/coding 구독과 선불 API를 automation, integration, team attribution, 비용 control 관점에서 비교합니다."), ll(["{provider} subscription vs api", "{provider} plan or api", "{provider} api pay as you go", "{provider} coding subscription"], ["подписка {provider} или api", "тариф {provider} и api", "{provider} api по факту", "coding подписка {provider}"], ["{provider} 订阅 对比 api", "{provider} 套餐还是 api", "{provider} api 按量付费", "{provider} 编程订阅"], ["{provider} 구독 vs api", "{provider} plan 또는 api", "{provider} api 종량제", "{provider} 코딩 구독"]), l("Subscriptions optimize an interactive product for one user; APIs expose metered calls to software. Choose by workload shape and governance, not by dividing a monthly price by imagined tokens.", "Подписка оптимизирует интерактивный продукт для одного пользователя; API даёт metered calls программам. Выбирайте по workload и governance, а не по выдуманному пересчёту monthly price в токены.", "订阅优化面向单用户的交互产品；API 向软件提供可计量调用。应按工作负载形态与治理选择，而不是把月费除以想象中的 token。", "구독은 한 사용자의 interactive product에 최적화되고 API는 software에 metered call을 제공합니다. 월 구독료를 가상 token으로 나누지 말고 workload 형태와 governance로 선택합니다."), ll(["Choose a subscription for regular human interaction inside {direct} when its product limits and account policy fit.", "Choose the API for backend jobs, agents, SDKs, per-project keys, request logs and deterministic spending caps.", "Do not share consumer credentials with a team service; use named API keys and a prepaid balance for auditable automation."], ["Подписка подходит для регулярной ручной работы в {direct}, если устраивают product limits и account policy.", "API выбирайте для backend jobs, agents, SDK, отдельных project keys, request logs и детерминированных spending caps.", "Не делитесь consumer credentials с team service: для automation нужны named API keys и проверяемый prepaid balance."], ["若 {direct} 的产品限制与账户政策合适，订阅适用于固定的人机交互。", "后端任务、agent、SDK、项目独立密钥、请求日志与确定性消费上限应选择 API。", "不要让团队服务共享消费者凭据；可审计自动化应使用命名 API 密钥与预付余额。"], ["{direct}의 product limit와 account policy가 맞는 정기적 사람 상호작용에는 구독을 선택합니다.", "backend job, agent, SDK, project별 key, request log, deterministic spending cap에는 API를 선택합니다.", "consumer credential을 team service와 공유하지 말고 auditable automation에는 named API key와 선불 잔액을 사용합니다."])),
  operationsTopic("generation-comparison", "compare", "previous-vs-current", l("{previous} vs {current} API", "{previous} и {current} API", "{previous} 与 {current} API 对比", "{previous}와 {current} API 비교"), l("Compare {previous} and {current} on your own {provider} workload: model IDs, quality, latency, controls, context behavior and total settled cost.", "Сравните {previous} и {current} на своём workload {provider}: model IDs, quality, latency, controls, context и полную settled cost.", "在自己的 {provider} 工作负载上比较 {previous} 与 {current}：model ID、质量、延迟、控制项、上下文行为和总结算成本。", "자신의 {provider} workload에서 {previous}와 {current}의 model ID, 품질, latency, control, context 동작, 총 settled cost를 비교합니다."), ll(["{previous} vs {current}", "{provider} old vs new model", "upgrade {provider} api model", "{current} migration"], ["{previous} или {current}", "старая и новая модель {provider}", "обновить модель {provider}", "миграция {current}"], ["{previous} 对比 {current}", "{provider} 新旧模型", "升级 {provider} api 模型", "{current} 迁移"], ["{previous} vs {current}", "{provider} 이전 최신 모델", "{provider} api 모델 업그레이드", "{current} migration"]), l("A newer generation is not a drop-in win for every task. Keep the old baseline, pin explicit IDs, replay representative cases and promote only after quality and cost gates pass.", "Новое поколение не всегда drop-in лучше. Сохраните старый baseline, закрепите IDs, повторите representative cases и повышайте только после quality/cost gates.", "新一代并非对所有任务都可直接胜出。保留旧基线、固定明确 ID、重放代表性案例，并在质量与成本门槛通过后再推广。", "새 세대가 모든 작업에서 drop-in으로 더 낫지는 않습니다. 이전 baseline, 명시적 ID, representative case replay를 유지하고 quality/cost gate 통과 후 승격합니다."), ll(["Fetch {catalog} and pin the exact old and new IDs; marketing family names are not reproducible deployment inputs.", "Replay the same prompts, tools, schemas and output caps, then score correctness, latency, retries and terminal usage.", "Canary {current} by task class and retain a fast rollback to {previous} until the production distribution is understood."], ["Получите точные old/new IDs из {catalog}: marketing family names не воспроизводимы в deployment.", "Повторите одинаковые prompts, tools, schemas и output caps; измерьте correctness, latency, retries и terminal usage.", "Включайте {current} canary по классу задач и сохраняйте быстрый rollback на {previous} до понимания production distribution."], ["从 {catalog} 获取并固定新旧精确 ID；营销系列名称不能作为可复现部署输入。", "重放相同 prompt、tool、schema 与 output 上限，记录正确性、延迟、重试和终态 usage。", "按任务类别对 {current} 做 canary，在理解生产分布前保留快速回滚到 {previous} 的能力。"], ["{catalog}에서 정확한 old/new ID를 가져와 고정하며 marketing family name을 재현 가능한 deployment input으로 쓰지 않습니다.", "같은 prompt, tool, schema, output cap을 replay하고 correctness, latency, retry, terminal usage를 측정합니다.", "task class별로 {current}를 canary하고 production distribution을 이해할 때까지 {previous}로 빠른 rollback을 유지합니다."])),
  operationsTopic("why-apitoken", "compare", "why-apitoken", l("Why Use apiToken.sale for the {provider} API", "Почему apiToken.sale для {provider} API", "为什么用 apiToken.sale 访问 {provider} API", "{provider} API에 apiToken.sale를 쓰는 이유"), l("Evaluate apiToken.sale for {provider} on concrete capabilities: one prepaid key, native or compatible protocols, live discovery, exact usage and account-level cost controls.", "Оцените apiToken.sale для {provider} по фактам: один prepaid key, native/compatible protocols, live discovery, exact usage и account-level cost controls.", "从具体能力评估 apiToken.sale 的 {provider} 方案：单个预付密钥、原生/兼容协议、实时发现、精确 usage 与账户级成本控制。", "하나의 선불 key, native/compatible protocol, live discovery, exact usage, account-level 비용 control로 apiToken.sale의 {provider} 가치를 평가합니다."), ll(["why apitoken {provider}", "apitoken {provider} api", "{provider} api gateway benefits", "one key {provider} api"], ["зачем apitoken для {provider}", "apitoken {provider} api", "плюсы gateway {provider}", "один ключ {provider} api"], ["为什么 apitoken {provider}", "apitoken {provider} api", "{provider} api 网关优势", "单密钥 {provider} api"], ["왜 apitoken {provider}", "apitoken {provider} api", "{provider} api gateway 장점", "하나의 key {provider} api"]), l("The value is operational consolidation without pretending every provider has the same wire protocol. One balance can fund multiple families while {provider} keeps its documented request path.", "Ценность — в operational consolidation без притворства, что у всех провайдеров один wire protocol. Один баланс покрывает семейства, а {provider} сохраняет свой request path.", "价值在于运行整合，同时不假装所有提供商共享同一 wire protocol。一个余额可支付多个系列，而 {provider} 保留其文档化请求路径。", "모든 provider가 같은 wire protocol인 척하지 않고 운영을 통합하는 것이 가치입니다. 하나의 잔액이 여러 family를 지원하면서 {provider}는 문서화된 request path를 유지합니다."), ll(["One sk-pool key and prepaid balance cover supported Claude, GPT, Gemini and Kimi models.", "{provider} uses {protocol}, {auth} and the live {catalog} rather than an undocumented translated surface.", "The dashboard records model, usage legs and charge; named keys support expiration and a lifetime spending limit."], ["Один sk-pool key и prepaid balance покрывают поддерживаемые Claude, GPT, Gemini и Kimi.", "{provider} использует {protocol}, {auth} и live {catalog}, а не недокументированный translated surface.", "Дашборд фиксирует model, usage legs и charge; named keys поддерживают expiration и lifetime spending limit."], ["一个 sk-pool 密钥和预付余额覆盖受支持的 Claude、GPT、Gemini 与 Kimi 模型。", "{provider} 使用 {protocol}、{auth} 与实时 {catalog}，而不是未文档化的转换表面。", "仪表板记录模型、usage 分类与扣费；命名密钥支持到期日期和终身消费上限。"], ["하나의 sk-pool key와 선불 잔액이 지원 Claude, GPT, Gemini, Kimi 모델을 포함합니다.", "{provider}는 문서화되지 않은 translated surface 대신 {protocol}, {auth}, live {catalog}를 사용합니다.", "dashboard는 model, usage leg, charge를 기록하고 named key는 expiration과 lifetime spending limit를 지원합니다."])),
  operationsTopic("gateway", "explain", "gateway", l("What Is a {provider} API Gateway?", "Что такое gateway для {provider} API", "什么是 {provider} API 网关？", "{provider} API gateway란?"), l("Learn what a {provider} API gateway changes—and what it must preserve—across credentials, protocol, routing, billing, observability and failures.", "Разберите, что gateway {provider} меняет и что обязан сохранять в credentials, protocol, routing, billing, observability и errors.", "了解 {provider} API 网关在凭据、协议、路由、结算、可观测性和故障方面改变什么、又必须保留什么。", "{provider} API gateway가 credential, protocol, routing, billing, observability, failure에서 바꾸는 것과 보존해야 할 것을 설명합니다."), ll(["{provider} api gateway", "what is {provider} gateway", "{provider} api proxy", "managed {provider} endpoint"], ["gateway {provider} api", "что такое шлюз {provider}", "proxy {provider} api", "managed endpoint {provider}"], ["{provider} api 网关", "什么是 {provider} 网关", "{provider} api 代理", "托管 {provider} endpoint"], ["{provider} api gateway", "{provider} gateway란", "{provider} api proxy", "managed {provider} endpoint"]), l("A trustworthy gateway is explicit at the account boundary and boring at the protocol boundary. It may own funding and routing, but clients should still see documented models, responses and errors.", "Надёжный gateway заметен на account boundary и скучен на protocol boundary. Он может владеть funding/routing, но клиент видит документированные models, responses и errors.", "可信网关在账户边界上明确，在协议边界上应当平淡。它可以负责资金与路由，但客户端仍应看到文档化的模型、响应和错误。", "신뢰할 gateway는 account boundary에서는 명시적이고 protocol boundary에서는 평범해야 합니다. funding과 routing을 소유해도 client는 문서화된 model, response, error를 봐야 합니다."), ll(["The gateway authenticates your platform key, checks prepaid funding and selects an eligible upstream provider pool.", "It preserves {protocol} on the native route and offers a universal OpenAI-compatible lane only where cross-provider clients need it.", "It must expose terminal usage and explicit failures so billing and retries remain auditable."], ["Gateway аутентифицирует platform key, проверяет prepaid funding и выбирает eligible upstream pool.", "Он сохраняет {protocol} на native route и даёт universal OpenAI-compatible lane для общих клиентов.", "Он обязан отдавать terminal usage и явные failures, чтобы billing и retries оставались проверяемыми."], ["网关验证平台密钥、检查预付资金并选择合格 upstream provider pool。", "原生路由保留 {protocol}，仅在跨提供商客户端需要时提供通用 OpenAI-compatible 通道。", "必须暴露终态 usage 与明确故障，确保结算与 retry 可审计。"], ["gateway는 platform key를 인증하고 prepaid funding을 확인해 eligible upstream provider pool을 선택합니다.", "native route에서 {protocol}을 보존하고 cross-provider client가 필요할 때만 universal OpenAI-compatible lane을 제공합니다.", "billing과 retry를 감사할 수 있도록 terminal usage와 명시적 failure를 노출해야 합니다."])),
  operationsTopic("rate-limits", "explain", "rate-limits", l("{provider} API Rate Limits and Capacity", "Rate limits и capacity {provider} API", "{provider} API 速率限制与容量", "{provider} API rate limit 및 capacity"), l("Handle {provider} API capacity with bounded concurrency, authoritative errors, jittered backoff and key spending controls without inventing unsupported daily quotas.", "Управляйте capacity {provider} через bounded concurrency, authoritative errors, jittered backoff и key spending controls без выдуманных daily quotas.", "用有界并发、权威错误、带抖动退避和密钥消费控制处理 {provider} API 容量，不虚构每日配额。", "지원되지 않는 daily quota를 만들지 않고 bounded concurrency, authoritative error, jittered backoff, key spending control로 {provider} API capacity를 관리합니다."), ll(["{provider} api rate limits", "{provider} api 429", "{provider} api concurrency", "{provider} capacity errors"], ["rate limits {provider} api", "{provider} api 429", "concurrency {provider} api", "capacity errors {provider}"], ["{provider} api 速率限制", "{provider} api 429", "{provider} api 并发", "{provider} 容量错误"], ["{provider} api rate limit", "{provider} api 429", "{provider} api concurrency", "{provider} capacity error"]), l("Rate limiting is dynamic capacity management, not a promise that one static requests-per-minute number applies to every model and account. Treat the actual response as authority.", "Rate limiting — динамическое управление capacity, а не обещание одного RPM для всех моделей и аккаунтов. Авторитетен фактический response.", "速率限制是动态容量管理，不是承诺一个静态 RPM 适用于所有模型与账户；实际响应才是权威。", "rate limit은 동적 capacity 관리이며 하나의 고정 RPM이 모든 model/account에 적용된다는 약속이 아닙니다. 실제 response가 권위입니다."), ll(["Bound concurrency per workload and queue excess work before it reaches {base}.", "On a retryable 429 or transient 5xx, use exponential backoff with jitter and a strict attempt/deadline budget; never retry after delivered streamed bytes.", "Use a separate named key per service with expiration and a lifetime spending limit; these are cost guardrails, not provider RPM controls."], ["Ограничьте concurrency по workload и ставьте лишнюю работу в очередь до {base}.", "Для retryable 429/5xx используйте exponential backoff с jitter и строгим attempt/deadline budget; не retry после streamed bytes.", "Выдайте сервисам named keys с expiration и lifetime spending limit: это cost guardrails, а не provider RPM controls."], ["按工作负载限制并发，在请求到达 {base} 前排队超额任务。", "遇到可重试 429/瞬时 5xx 时使用带抖动的指数退避和严格 attempt/deadline 预算；流式字节已交付后不要重试。", "每个服务使用独立命名密钥，设置到期日期与终身消费上限；这是成本护栏，不是提供商 RPM 控制。"], ["workload별 concurrency를 제한하고 초과 작업은 {base} 도달 전에 queue합니다.", "retry 가능한 429/transient 5xx에 jitter exponential backoff와 엄격한 attempt/deadline budget을 쓰고 streamed byte 전달 후 retry하지 않습니다.", "service별 named key에 expiration과 lifetime spending limit를 두며 이는 cost guardrail이지 provider RPM control이 아닙니다."])),
  operationsTopic("streaming", "explain", "streaming", l("{provider} API Streaming Guide", "Гайд по streaming {provider} API", "{provider} API 流式响应指南", "{provider} API streaming 가이드"), l("Implement {provider} streaming with the correct protocol, event parser, cancellation rules, terminal usage and an evidence-bounded view of provider capability.", "Реализуйте streaming {provider} с правильным protocol, event parser, cancellation, terminal usage и проверенными заявлениями о capability.", "使用正确协议、事件解析、取消规则、终态 usage，并基于证据描述提供商能力，实现 {provider} 流式响应。", "정확한 protocol, event parser, cancellation rule, terminal usage, evidence 기반 provider capability로 {provider} streaming을 구현합니다."), ll(["{provider} api streaming", "{provider} sse", "stream {provider} response", "{provider} terminal usage"], ["streaming {provider} api", "{provider} sse", "потоковый ответ {provider}", "terminal usage {provider}"], ["{provider} api 流式", "{provider} sse", "流式 {provider} 响应", "{provider} 终态 usage"], ["{provider} api streaming", "{provider} sse", "{provider} response stream", "{provider} terminal usage"]), l("Streaming is correct only when content arrives according to the documented event grammar, cancellation is safe and the final usage record remains authoritative. Capability claims must match live evidence.", "Streaming корректен, когда content приходит по event grammar, cancellation безопасен, а final usage остаётся authoritative. Заявления должны соответствовать live evidence.", "只有内容按文档事件语法到达、取消安全且最终 usage 仍权威时，流式才算正确；能力声明必须符合实时证据。", "content가 문서화된 event grammar로 도착하고 cancellation이 안전하며 final usage가 authoritative할 때만 streaming이 올바릅니다. capability claim은 live evidence와 일치해야 합니다."), ll(["Use the streaming form of {protocol} and parse events incrementally instead of splitting arbitrary network chunks as JSON.", "Treat the terminal event as the source for finish reason and usage; do not estimate the final bill from emitted text.", "Current evidence: {streamingEvidence}. Build a non-streaming fallback where incrementality is not part of the verified contract."], ["Используйте streaming-форму {protocol} и разбирайте events, не пытайтесь парсить произвольный network chunk как JSON.", "Terminal event — источник finish reason и usage; не оценивайте счёт по выданному тексту.", "Текущий evidence: {streamingEvidence}. Добавьте non-streaming fallback, где incrementality не входит в проверенный contract."], ["使用 {protocol} 的流式形式并按事件增量解析，不要把任意网络 chunk 当作 JSON。", "终态事件是 finish reason 与 usage 的来源；不要按已输出文本估算最终账单。", "当前证据：{streamingEvidence}。若增量性不属于已验证合同，应提供非流式 fallback。"], ["{protocol}의 streaming 형식을 사용하고 임의 network chunk를 JSON으로 나누지 말고 event를 증분 parsing합니다.", "terminal event를 finish reason과 usage의 source로 사용하고 출력 text로 final bill을 추정하지 않습니다.", "현재 evidence: {streamingEvidence}. incrementality가 검증 contract가 아닌 곳에는 non-streaming fallback을 둡니다."])),
  operationsTopic("prompt-caching", "explain", "prompt-caching", l("{provider} API Prompt Caching", "Prompt caching в {provider} API", "{provider} API Prompt 缓存", "{provider} API prompt caching"), l("Use {provider} caching without double-counting tokens: design stable prefixes, understand cache mode, read terminal usage and compare end-to-end cost and latency.", "Используйте caching {provider} без двойного счёта: стабильные prefixes, cache mode, terminal usage и полная цена/latency.", "使用 {provider} 缓存而不重复计算 token：设计稳定前缀、理解缓存模式、读取终态 usage，并比较端到端成本与延迟。", "token을 이중 계산하지 않고 stable prefix, cache mode, terminal usage, end-to-end 비용·latency로 {provider} caching을 사용합니다."), ll(["{provider} api prompt caching", "{provider} cached input", "reduce {provider} context cost", "{provider} cache usage"], ["prompt caching {provider}", "cached input {provider}", "снизить цену контекста {provider}", "cache usage {provider}"], ["{provider} api prompt 缓存", "{provider} cached input", "降低 {provider} 上下文成本", "{provider} cache usage"], ["{provider} api prompt caching", "{provider} cached input", "{provider} context 비용 절감", "{provider} cache usage"]), l("Caching rewards stable repeated context. It does not make every token free, and the only reliable accounting is the provider's terminal usage separated into the supported cache legs.", "Caching выгоден для стабильного повторного контекста. Он не делает все токены бесплатными; надёжен только terminal usage с поддерживаемыми cache legs.", "缓存奖励稳定重复上下文，但不会让所有 token 免费；可靠核算只能依赖提供商终态 usage 中支持的缓存分类。", "caching은 안정적으로 반복되는 context에 유리하지만 모든 token을 무료로 만들지 않습니다. 신뢰할 accounting은 지원 cache leg로 나뉜 provider terminal usage뿐입니다."), ll(["Place stable system instructions, schemas and tool definitions before volatile user data so the reusable prefix stays identical.", "For {provider}, the implemented mode is {cacheMode}; do not invent a cache-write price or TTL not present in the live contract.", "Run a cold request and a warm repeat, then compare cache usage, latency, output quality and final charge in the dashboard."], ["Ставьте стабильные system instructions, schemas и tools перед изменяемыми user data, чтобы reusable prefix оставался идентичным.", "Для {provider} реализован режим {cacheMode}; не придумывайте cache-write price или TTL вне live contract.", "Выполните cold request и warm repeat, затем сравните cache usage, latency, quality и итоговый charge."], ["将稳定 system instruction、schema 与 tool 定义放在易变用户数据之前，保持可复用前缀完全一致。", "{provider} 已实现模式为 {cacheMode}；不要虚构实时合同之外的 cache-write 价格或 TTL。", "运行一次 cold request 和一次 warm repeat，再比较缓存 usage、延迟、输出质量与最终扣费。"], ["stable system instruction, schema, tool definition을 volatile user data 앞에 두어 reusable prefix를 동일하게 유지합니다.", "{provider} 구현 mode는 {cacheMode}이며 live contract에 없는 cache-write 가격이나 TTL을 만들지 않습니다.", "cold request와 warm repeat를 실행한 뒤 cache usage, latency, output 품질, final charge를 비교합니다."])),
  operationsTopic("best-practices", "explain", "best-practices", l("{provider} API Best Practices", "Лучшие практики {provider} API", "{provider} API 最佳实践", "{provider} API best practice"), l("Production checklist for {provider}: explicit model discovery, secret handling, timeouts, bounded retries, usage reconciliation, eval-based routing and safe rollout.", "Production checklist {provider}: model discovery, secrets, timeouts, bounded retries, usage reconciliation, eval routing и безопасный rollout.", "{provider} 生产清单：明确模型发现、secret 管理、超时、有界重试、usage 对账、评测路由与安全发布。", "{provider} production checklist: 명시적 model discovery, secret handling, timeout, bounded retry, usage reconciliation, eval routing, safe rollout."), ll(["{provider} api best practices", "production {provider} api", "reliable {provider} integration", "{provider} api checklist"], ["лучшие практики {provider} api", "production {provider} api", "надёжная интеграция {provider}", "чеклист {provider} api"], ["{provider} api 最佳实践", "生产 {provider} api", "可靠 {provider} 集成", "{provider} api 清单"], ["{provider} api best practice", "production {provider} api", "신뢰할 {provider} integration", "{provider} api checklist"]), l("Reliable integrations make model choice, protocol assumptions and recovery policy explicit. They fail within a budget, record terminal truth and roll out changes through canaries.", "Надёжные интеграции явно задают model choice, protocol assumptions и recovery policy. Они падают в пределах budget, записывают terminal truth и выкатываются через canary.", "可靠集成会明确模型选择、协议假设与恢复策略，在预算内失败、记录终态事实，并通过 canary 发布变更。", "신뢰할 integration은 model choice, protocol assumption, recovery policy를 명시하고 budget 안에서 실패하며 terminal truth를 기록하고 canary로 변경을 배포합니다."), ll(["Discover models with {catalog}, pin an exact returned ID and reject configuration drift at startup.", "Keep {auth} in a server-side secret, set connect/first-byte/total deadlines and retry only safe transient failures.", "Record request ID, model, terminal usage, latency and charge; canary model or prompt changes against a fixed eval before broad rollout."], ["Получайте модели через {catalog}, закрепляйте точный ID и отклоняйте config drift при старте.", "Храните {auth} в server-side secret, задайте connect/first-byte/total deadlines и retry только безопасные transient failures.", "Пишите request ID, model, terminal usage, latency и charge; model/prompt changes катите canary через фиксированный eval."], ["通过 {catalog} 发现模型，固定实际返回的 ID，并在启动时拒绝配置漂移。", "把 {auth} 放在服务端 secret，设置连接/首字节/总 deadline，只重试安全的瞬时故障。", "记录 request ID、模型、终态 usage、延迟与扣费；模型或 prompt 变更先通过固定评测做 canary。"], ["{catalog}로 모델을 발견하고 반환된 정확한 ID를 고정하며 시작 시 config drift를 거부합니다.", "{auth}를 server-side secret에 두고 connect/first-byte/total deadline을 설정하며 안전한 transient failure만 retry합니다.", "request ID, model, terminal usage, latency, charge를 기록하고 model/prompt 변경은 고정 eval로 canary합니다."])),
  operationsTopic("cli-setup", "integrate", "cli-api-key", l("{cli} API Key Setup", "Настройка API-ключа для {cli}", "{cli} API 密钥设置", "{cli} API key 설정"), l("Configure {cli} with an apiToken.sale key, explicit endpoint and model, then verify the active payer, catalog and first repository request.", "Настройте {cli} с ключом apiToken.sale, явными endpoint/model и проверьте active payer, catalog и первый repository request.", "使用 apiToken.sale 密钥、明确 endpoint 与模型配置 {cli}，再验证当前付款方、目录及首个仓库请求。", "apiToken.sale key, 명시적 endpoint/model로 {cli}를 설정하고 active payer, catalog, 첫 repository request를 검증합니다."), ll(["{cli} api key", "{cli} custom endpoint", "{provider} cli setup", "{cli} apitoken"], ["api ключ {cli}", "custom endpoint {cli}", "настройка cli {provider}", "{cli} apitoken"], ["{cli} api 密钥", "{cli} 自定义 endpoint", "{provider} cli 设置", "{cli} apitoken"], ["{cli} api key", "{cli} custom endpoint", "{provider} cli 설정", "{cli} apitoken"]), l("A CLI profile must make the provider, base URL, key source and model unambiguous. Separate it from vendor login state so every session has one payer and one audit trail.", "CLI profile должен однозначно задавать provider, base URL, key source и model. Отделите его от vendor login, чтобы у сессии был один payer и audit trail.", "CLI profile 必须明确提供商、base URL、密钥来源与模型。应与厂商登录状态分离，使每次会话只有一个付款方和一条审计链。", "CLI profile은 provider, base URL, key source, model을 명확히 해야 합니다. vendor login 상태와 분리해 각 session에 payer와 audit trail이 하나만 있도록 합니다."), ll(["Create a named apiToken.sale profile pointing to {base} and read the key from the CLI's supported secret mechanism.", "Call {catalog} with the same key, pin an explicit model and confirm the CLI does not silently replace it with a vendor default.", "Run a minimal repository prompt, inspect {cli} status and match its model/usage with the dashboard before autonomous work."], ["Создайте named profile apiToken.sale на {base} и читайте ключ из поддерживаемого CLI secret mechanism.", "Вызовите {catalog} тем же ключом, закрепите model и проверьте, что CLI не заменяет её vendor default.", "Запустите минимальный repository prompt, проверьте status {cli} и сопоставьте model/usage с дашбордом до autonomous work."], ["创建指向 {base} 的 apiToken.sale 命名 profile，并从 CLI 支持的 secret 机制读取密钥。", "用同一密钥调用 {catalog}，固定明确模型，并确认 CLI 不会静默替换成厂商默认。", "运行最小仓库 prompt，查看 {cli} 状态，并在自主工作前将 model/usage 与仪表板核对。"], ["{base}를 가리키는 named apiToken.sale profile을 만들고 CLI 지원 secret mechanism에서 key를 읽습니다.", "같은 key로 {catalog}를 호출해 model을 고정하고 CLI가 vendor default로 조용히 바꾸지 않는지 확인합니다.", "최소 repository prompt를 실행하고 {cli} status와 dashboard model/usage를 대조한 후 autonomous 작업을 시작합니다."]), { gpt: "codex-cli-setup", kimi: "kimi-api-for-kimi-code" }),
  operationsTopic("key-security", "explain", "key-security", l("{provider} API Key Security", "Безопасность API-ключа {provider}", "{provider} API 密钥安全", "{provider} API key 보안"), l("Protect {provider} API keys with server-side storage, least privilege by service, expiration, lifetime spending limits, redacted logs and evidence-led incident response.", "Защитите ключи {provider}: server-side storage, отдельные service keys, expiration, lifetime spending limits, redacted logs и incident response по фактам.", "通过服务端存储、按服务最小权限、到期日期、终身消费上限、日志脱敏与基于证据的事件响应保护 {provider} API 密钥。", "server-side storage, service별 최소 권한, expiration, lifetime spending limit, redacted log, evidence 기반 incident response로 {provider} API key를 보호합니다."), ll(["{provider} api key security", "protect {provider} api key", "rotate {provider} api key", "{provider} key spending limit"], ["безопасность ключа {provider}", "защитить api ключ {provider}", "rotation ключа {provider}", "лимит расходов ключа {provider}"], ["{provider} api 密钥安全", "保护 {provider} api 密钥", "轮换 {provider} api 密钥", "{provider} 密钥消费上限"], ["{provider} api key 보안", "{provider} api key 보호", "{provider} api key rotation", "{provider} key 지출 한도"]), l("An API key is a bearer credential: whoever has it can spend its allowed balance. Minimize where it exists, bound its lifetime impact and make revocation a rehearsed operation.", "API-ключ — bearer credential: владелец может тратить доступный баланс. Сведите копии к минимуму, ограничьте lifetime impact и отрепетируйте revoke.", "API 密钥是 bearer credential：持有者可消费其允许余额。应减少副本、限制生命周期影响，并演练撤销流程。", "API key는 bearer credential로 가진 사람이 허용 잔액을 사용할 수 있습니다. 존재 위치를 최소화하고 lifetime impact를 제한하며 revoke를 연습합니다."), ll(["Store the sk-pool key only in a server-side secret manager or the client's protected secret field; never commit it or expose it to browser code.", "Issue a separate named key per service/environment with an expiration date and lifetime spending limit so one leak has a bounded blast radius.", "On suspected exposure, revoke the key, replace the consuming service secret, inspect request IDs/usage and redact logs; never send the raw key to support."], ["Храните sk-pool key только в server-side secret manager или protected secret field; не commit и не отдавайте browser code.", "Выдавайте отдельный named key на service/environment с expiration date и lifetime spending limit для ограничения blast radius.", "При утечке revoke ключ, замените secret сервиса, проверьте request IDs/usage и очистите логи; не отправляйте raw key support."], ["sk-pool 密钥只存于服务端 secret manager 或客户端受保护 secret 字段；不要提交到仓库或暴露给浏览器代码。", "每个服务/环境签发独立命名密钥，并设置到期日期与终身消费上限，限制单次泄露影响范围。", "疑似泄露时撤销密钥、替换服务 secret、检查 request ID/usage 并清理日志；不要向支持发送原始密钥。"], ["sk-pool key는 server-side secret manager나 client protected secret field에만 보관하고 commit하거나 browser code에 노출하지 않습니다.", "service/environment별 named key에 expiration date와 lifetime spending limit를 두어 leak blast radius를 제한합니다.", "노출 의심 시 key를 revoke하고 service secret을 교체하며 request ID/usage와 log redaction을 확인하고 raw key를 support에 보내지 않습니다."])),
  operationsTopic("ai-agents", "explain", "for-ai-agents", l("{provider} API for AI Agents", "{provider} API для AI-агентов", "用于 AI Agent 的 {provider} API", "AI agent용 {provider} API"), l("Build cost-aware AI agents on {provider} with explicit model routing, bounded tool loops, context management, terminal usage and per-agent key controls.", "Стройте cost-aware AI agents на {provider}: model routing, bounded tool loops, context management, terminal usage и отдельные key controls.", "在 {provider} 上构建成本可控 AI agent：明确模型路由、有界 tool loop、上下文管理、终态 usage 与每 agent 密钥控制。", "명시적 model routing, bounded tool loop, context management, terminal usage, agent별 key control로 {provider} AI agent를 구축합니다."), ll(["{provider} api ai agents", "{provider} agent api", "{provider} tool calling", "cost control ai agent {provider}"], ["{provider} api для ai agents", "agent api {provider}", "tool calling {provider}", "контроль цены ai agent {provider}"], ["{provider} api ai agent", "{provider} agent api", "{provider} tool calling", "{provider} ai agent 成本控制"], ["{provider} api ai agent", "{provider} agent api", "{provider} tool calling", "{provider} ai agent 비용 control"]), l("Agents amplify both capability and spend because one user goal can trigger many model calls and tools. Reliability comes from explicit state, budgets, validation and a terminal ledger for every turn.", "Agents усиливают и возможности, и расход: одна цель запускает много model calls/tools. Надёжность дают явное state, budgets, validation и terminal ledger каждого turn.", "Agent 会同时放大能力与消费，因为一个用户目标可能触发多次模型调用和 tool。可靠性来自明确状态、预算、验证及每个 turn 的终态账本。", "agent는 한 사용자 목표가 여러 model call과 tool을 만들기 때문에 capability와 spend를 모두 증폭합니다. 명시적 state, budget, validation, turn별 terminal ledger가 신뢰성을 만듭니다."), ll(["Use {balanced} for ordinary planning and coding, {fast} for bounded deterministic substeps and {flagship} only for eval-proven hard decisions.", "Set maximum turns, tool calls, wall time and output per run; validate tool arguments before execution and never use an unbounded retry loop.", "Give each agent a named expiring key with a lifetime spending limit, then record model, terminal usage, tool outcome and charge per turn."], ["Используйте {balanced} для обычного planning/coding, {fast} для bounded substeps и {flagship} только для сложных решений с eval proof.", "Задайте max turns, tool calls, wall time и output; проверяйте tool arguments и не используйте unbounded retry loop.", "Дайте agent отдельный expiring key с lifetime spending limit и записывайте model, terminal usage, tool outcome и charge каждого turn."], ["常规规划与编程使用 {balanced}，有界确定性子步骤使用 {fast}，只有评测证明的困难决策才使用 {flagship}。", "设置每次运行的最大 turn、tool call、wall time 与 output；执行前验证 tool 参数，禁止无界 retry loop。", "为每个 agent 配置带到期日期与终身消费上限的命名密钥，并逐 turn 记录模型、终态 usage、tool 结果与扣费。"], ["일반 planning/coding은 {balanced}, bounded deterministic substep은 {fast}, eval로 입증된 어려운 결정만 {flagship}을 사용합니다.", "run별 최대 turn, tool call, wall time, output을 설정하고 실행 전 tool argument를 검증하며 unbounded retry loop를 금지합니다.", "agent별 expiring named key와 lifetime spending limit를 두고 turn마다 model, terminal usage, tool outcome, charge를 기록합니다."])),
];

const depthSectionCopy = {
  decision: l("Decision matrix: {title}", "Матрица решений: {title}", "决策矩阵：{title}", "의사결정 매트릭스: {title}"),
  protocol: l("Exact {provider} request path", "Точный request path {provider}", "准确的 {provider} 请求路径", "정확한 {provider} request path"),
  verification: l("Validation plan for {title}", "Проверка перед rollout: {title}", "{title} 验证计划", "{title} 검증 계획"),
  operations: l("Troubleshooting and the go/no-go rule", "Troubleshooting и критерий go/no-go", "故障排查与上线判定", "troubleshooting과 go/no-go 기준"),
} satisfies Record<string, L10n>;

const tableCopy = {
  priority: l("Priority", "Приоритет", "优先级", "우선순위"),
  action: l("Recommended action", "Рекомендуемое действие", "建议操作", "권장 작업"),
  stop: l("Failure signal", "Стоп-сигнал", "失败信号", "중단 신호"),
} satisfies Record<string, L10n>;

const protocolCopy: Record<TopicKind, L10n> = {
  access: l(
    "After the account or payment decision, {provider} traffic still follows {protocol}: use {base}, send {auth}, and select an exact model returned by {catalog}.",
    "После решения по аккаунту или оплате трафик {provider} всё равно идёт через {protocol}: используйте {base}, {auth} и точную модель из {catalog}.",
    "完成账户或付款决策后，{provider} 流量仍使用 {protocol}：访问 {base}、发送 {auth}，并选择 {catalog} 返回的准确模型。",
    "계정 또는 결제 결정 후에도 {provider} traffic은 {protocol}을 따릅니다. {base}, {auth}, {catalog}가 반환한 정확한 model을 사용합니다.",
  ),
  model: l(
    "Model tier changes the ID, not the wire contract. Call {provider} through {protocol} at {base}, authenticate with {auth}, and pin the evaluated ID from {catalog}.",
    "Model tier меняет ID, а не wire contract. Вызывайте {provider} через {protocol} на {base}, используйте {auth} и закрепляйте проверенный ID из {catalog}.",
    "模型层级改变的是 ID，而不是 wire contract。通过 {base} 的 {protocol} 调用 {provider}，使用 {auth}，并固定 {catalog} 中已评测的 ID。",
    "model tier는 ID를 바꾸지만 wire contract는 바꾸지 않습니다. {base}의 {protocol}, {auth}, {catalog}에서 평가한 ID를 사용합니다.",
  ),
  tool: l(
    "Keep the client configuration and the provider probe separate. The underlying {provider} proof uses {protocol} at {base} with {auth}; the tool must preserve the selected catalog model and errors.",
    "Разделяйте client config и provider probe. Базовая проверка {provider} использует {protocol} на {base} с {auth}; tool должен сохранять catalog model и ошибки.",
    "将客户端配置与提供商探针分开。{provider} 基础验证使用 {base} 的 {protocol} 与 {auth}；工具必须保留所选目录模型和错误。",
    "client 설정과 provider probe를 분리합니다. {provider} 기본 검증은 {base}의 {protocol}과 {auth}를 사용하며 tool은 선택한 catalog model과 error를 보존해야 합니다.",
  ),
  compare: l(
    "Run an attributable {provider} control request before comparing routes. Use {protocol} at {base}, {auth}, and the same exact catalog model so protocol and billing differences are measurable.",
    "До сравнения маршрутов выполните атрибутируемый control request {provider}: {protocol} на {base}, {auth} и одна точная catalog model для измеримых различий.",
    "比较路由前先运行可归因的 {provider} 对照请求：使用 {base} 的 {protocol}、{auth} 与同一个准确目录模型，以便测量协议和结算差异。",
    "route 비교 전에 귀속 가능한 {provider} control request를 실행합니다. {base}의 {protocol}, {auth}, 동일한 exact catalog model로 protocol과 billing 차이를 측정합니다.",
  ),
  operations: l(
    "Measure the operating claim on a real {provider} request. Use {protocol} at {base}, authenticate with {auth}, and keep the exact {catalog} model, request ID and terminal usage together.",
    "Проверяйте operational claim на реальном запросе {provider}: {protocol} на {base}, {auth}, точная модель из {catalog}, request ID и terminal usage вместе.",
    "在真实 {provider} 请求上测量运维结论：使用 {base} 的 {protocol} 与 {auth}，并把准确 {catalog} 模型、request ID 和终态 usage 放在一起。",
    "실제 {provider} 요청에서 운영 주장을 측정합니다. {base}의 {protocol}, {auth}를 사용하고 정확한 {catalog} model, request ID, terminal usage를 함께 보존합니다.",
  ),
};

const verificationCopy: Record<TopicKind, L10nList> = {
  access: ll(
    ["Confirm the account, named key and usable dashboard balance are three distinct completed states.", "Call {catalog} with that key and pin an exact returned model before spending balance.", "Send one low-cap request through {protocol}; require output, terminal usage and a matching ledger entry."],
    ["Подтвердите три отдельных состояния: account, named key и usable balance в дашборде.", "Вызовите {catalog} этим ключом и закрепите returned model до расхода баланса.", "Отправьте low-cap запрос через {protocol}; потребуйте output, terminal usage и matching ledger entry."],
    ["分别确认账户、命名密钥与仪表板可用余额三种状态均已完成。", "使用该密钥调用 {catalog}，并在消费余额前固定准确返回模型。", "通过 {protocol} 发送一次低上限请求；要求有输出、终态 usage 与匹配账本记录。"],
    ["account, named key, dashboard usable balance가 각각 완료됐는지 확인합니다.", "같은 key로 {catalog}를 호출하고 잔액 사용 전에 반환된 정확한 model을 고정합니다.", "{protocol} low-cap 요청에서 output, terminal usage, 일치하는 ledger entry를 요구합니다."],
  ),
  model: ll(
    ["Build a fixed eval with quality thresholds for the task classes named in this guide.", "Run the same prompts with {fast}, {balanced} and {flagship}; record latency, retries and terminal usage.", "Pin the cheapest tier that passes, then canary its exact catalog ID with an explicit escalation rule."],
    ["Соберите fixed eval с quality thresholds для классов задач из статьи.", "Запустите одинаковые prompts на {fast}, {balanced} и {flagship}; запишите latency, retries и terminal usage.", "Закрепите самый дешёвый passing tier и canary exact catalog ID с explicit escalation rule."],
    ["为本文任务类别建立带质量阈值的固定评测。", "在 {fast}、{balanced} 与 {flagship} 上运行相同 prompt，记录延迟、重试与终态 usage。", "固定能通过评测的最低价层级，再以明确升级规则 canary 其准确目录 ID。"],
    ["글의 task class에 quality threshold가 있는 fixed eval을 만듭니다.", "같은 prompt를 {fast}, {balanced}, {flagship}에서 실행하고 latency, retry, terminal usage를 기록합니다.", "통과한 가장 싼 tier의 exact catalog ID를 명시적 escalation rule과 함께 canary합니다."],
  ),
  tool: ll(
    ["Capture the configured base URL, key source and exact model without exposing the credential.", "Run one bounded client turn and, when relevant, one tool call with fallback disabled.", "Match the client's model, error behavior and terminal usage to the dashboard before autonomous work."],
    ["Зафиксируйте base URL, key source и exact model без раскрытия credential.", "Выполните один bounded client turn и при необходимости tool call с отключённым fallback.", "Сверьте model, error behavior и terminal usage клиента с дашбордом до autonomous work."],
    ["记录已配置的 base URL、密钥来源与准确模型，但不暴露凭据。", "关闭 fallback，运行一个有界客户端 turn，并在需要时执行一次 tool call。", "自主工作前，将客户端模型、错误行为和终态 usage 与仪表板核对。"],
    ["credential 노출 없이 설정된 base URL, key source, exact model을 기록합니다.", "fallback을 끄고 bounded client turn 하나와 필요 시 tool call 하나를 실행합니다.", "autonomous 작업 전 client model, error 동작, terminal usage를 dashboard와 대조합니다."],
  ),
  compare: ll(
    ["Write the decision criteria before testing: protocol fidelity, model identity, settled cost and operational ownership.", "Send the same bounded prompt through both routes and capture output, errors, request IDs and terminal usage.", "Choose only after documenting feature gaps, support boundaries and the total operating cost of the winning route."],
    ["До теста задайте criteria: protocol fidelity, model identity, settled cost и operational ownership.", "Отправьте один bounded prompt по обоим routes и сохраните output, errors, request IDs и terminal usage.", "Выберите route после фиксации feature gaps, support boundaries и total operating cost."],
    ["测试前写明决策标准：协议保真、模型身份、结算成本与运维责任。", "通过两条路由发送同一个有界 prompt，记录输出、错误、request ID 与终态 usage。", "记录功能差距、支持边界和胜出路由的总运维成本后再做选择。"],
    ["테스트 전에 protocol fidelity, model identity, settled cost, operational ownership 기준을 작성합니다.", "두 route에 같은 bounded prompt를 보내 output, error, request ID, terminal usage를 수집합니다.", "feature gap, support boundary, winning route의 총운영비를 문서화한 뒤 선택합니다."],
  ),
  operations: ll(
    ["Record a baseline request with its exact model, configuration, latency, terminal usage and charge.", "Change one operating variable from this guide and repeat the same bounded workload.", "Ship only when the target metric improves without triggering a listed failure signal; otherwise roll back."],
    ["Запишите baseline request: exact model, config, latency, terminal usage и charge.", "Измените одну operating variable из статьи и повторите тот же bounded workload.", "Выкатывайте только при улучшении target metric без стоп-сигналов; иначе rollback."],
    ["记录基线请求的准确模型、配置、延迟、终态 usage 与扣费。", "只更改本文中的一个运维变量，再重复相同有界工作负载。", "仅当目标指标改善且未触发失败信号时发布，否则回滚。"],
    ["baseline request의 exact model, config, latency, terminal usage, charge를 기록합니다.", "이 글의 operating variable 하나만 바꾸고 같은 bounded workload를 반복합니다.", "target metric이 개선되고 failure signal이 없을 때만 배포하며 아니면 rollback합니다."],
  ),
};

const probeCopy = l(
  "Keep this {balanced} smoke probe separate from “{title}” application traffic so endpoint, authentication and model availability remain independently diagnosable.",
  "Держите smoke probe {balanced} отдельно от application traffic «{title}», чтобы независимо диагностировать endpoint, auth и model availability.",
  "将这个 {balanced} smoke probe 与“{title}”应用流量分开，以便独立诊断 endpoint、鉴权与模型可用性。",
  "endpoint, 인증, model availability를 독립 진단할 수 있도록 {balanced} smoke probe를 “{title}” application traffic과 분리합니다.",
);

const goNoGoCopy = l(
  "Do not ship “{title}” while any failure signal in the matrix is true. Preserve the request ID and terminal usage, correct the named boundary, repeat the matching validation step, and never switch provider, model or payer silently.",
  "Не выкатывайте «{title}», пока активен любой стоп-сигнал матрицы. Сохраните request ID и terminal usage, исправьте указанную границу, повторите соответствующий шаг и не меняйте скрытно provider, model или payer.",
  "矩阵中任一失败信号仍成立时，不要发布“{title}”。保留 request ID 与终态 usage，修正对应边界，重复相关验证步骤，绝不静默切换提供商、模型或付款方。",
  "매트릭스의 failure signal이 하나라도 참이면 “{title}”를 배포하지 않습니다. request ID와 terminal usage를 보존하고 해당 경계를 수정해 검증을 반복하며 provider, model, payer를 조용히 바꾸지 않습니다.",
);

function protocolSnippet(facts: ProviderFacts): string {
  if (facts.id === "gpt") {
    return `curl ${facts.baseUrl}/responses \\\n+  -H "Authorization: Bearer $APITOKEN_API_KEY" \\\n+  -H "Content-Type: application/json" \\\n+  -d '{"model":"${facts.models.balanced}","input":"Reply with exactly: connected"}'`;
  }
  if (facts.id === "gemini") {
    return `curl ${facts.baseUrl}/v1beta/models/${facts.models.balanced}:generateContent \\\n+  -H "x-goog-api-key: $APITOKEN_API_KEY" \\\n+  -H "Content-Type: application/json" \\\n+  -d '{"contents":[{"parts":[{"text":"Reply with exactly: connected"}]}]}'`;
  }
  return `curl ${facts.baseUrl}/v1/messages \\\n+  -H "x-api-key: $APITOKEN_API_KEY" \\\n+  -H "anthropic-version: 2023-06-01" \\\n+  -H "Content-Type: application/json" \\\n+  -d '{"model":"${facts.models.balanced}","max_tokens":64,"messages":[{"role":"user","content":"Reply with exactly: connected"}]}'`;
}

function articleSlug(topic: TopicSpec, facts: ProviderFacts): string {
  return topic.providerExisting?.[facts.id] ?? topic.slug(facts);
}

function relatedSlugs(current: string, facts: ProviderFacts): string[] {
  return [facts.buySlug, facts.quickstartSlug, facts.pricingSlug, facts.compareSlug, facts.cliSlug]
    .filter((slug, index, entries) => slug !== current && entries.indexOf(slug) === index)
    .slice(0, 4);
}

const ECONOMY_FOCUS_INDEXES: Record<string, readonly number[]> = {
  pricing: [1],
  "save-tokens": [0],
  "best-coding-model": [1],
  "ai-agents": [0],
};

function applyTopicModelEconomics(topicId: string, focus: string[], facts: ProviderFacts): string[] {
  const economyIndexes = ECONOMY_FOCUS_INDEXES[topicId] ?? [];
  return focus.map((item, index) => economyIndexes.includes(index)
    ? item.replaceAll(facts.models.fast, facts.models.economy)
    : item);
}

function topicRiskItems(topicId: string, locale: Locale, facts: ProviderFacts): string[] {
  const risks = PROVIDER_TOPIC_RISKS[topicId];
  if (!risks) throw new Error(`Missing provider editorial risks for ${topicId}`);
  return localizedList(risks, locale, facts);
}

function localizedTemplate(value: L10n, locale: Locale, facts: ProviderFacts, title: string): string {
  return localized(value, locale, facts).replaceAll("{title}", title);
}

function depthSections(
  title: string,
  topicId: string,
  kind: TopicKind,
  focus: string[],
  facts: ProviderFacts,
  locale: Locale,
  includeProtocol: boolean,
): LearnSection[] {
  const risks = topicRiskItems(topicId, locale, facts);
  const proof = localizedList(verificationCopy[kind], locale, facts);
  const sections: LearnSection[] = [{
    h2: localizedTemplate(depthSectionCopy.decision, locale, facts, title),
    blocks: [{
      type: "table",
      headers: [
        localized(tableCopy.priority, locale, facts),
        localized(tableCopy.action, locale, facts),
        localized(tableCopy.stop, locale, facts),
      ],
      rows: focus.map((action, index) => [`${index + 1}`, action, risks[index]!]),
    }],
  }];

  if (includeProtocol) {
    sections.push({
      h2: localized(depthSectionCopy.protocol, locale, facts),
      blocks: [
        { type: "p", text: localized(protocolCopy[kind], locale, facts) },
        { type: "code", code: protocolSnippet(facts).replaceAll("\n+", "\n") },
        { type: "note", text: localizedTemplate(probeCopy, locale, facts, title) },
      ],
    });
  }

  sections.push(
    {
      h2: localizedTemplate(depthSectionCopy.verification, locale, facts, title),
      blocks: [{ type: "steps", items: proof }],
    },
    {
      h2: localized(depthSectionCopy.operations, locale, facts),
      blocks: [{ type: "p", text: localizedTemplate(goNoGoCopy, locale, facts, title) }],
    },
  );
  return sections;
}

function localizedContent(topic: TopicSpec, facts: ProviderFacts, locale: Locale): LocalizedContent {
  const title = localized(topic.title, locale, facts);
  const focus = applyTopicModelEconomics(topic.id, localizedList(topic.focus, locale, facts), facts);
  const risks = topicRiskItems(topic.id, locale, facts);
  const proof = localizedList(verificationCopy[topic.kind], locale, facts);
  const questions = {
    decision: l(`What is the first decision for “${title}”?`, `С какого решения начать «${title}»?`, `“${title}”的首要决策是什么？`, `“${title}”의 첫 결정은 무엇인가요?`),
    failure: l(`What invalidates “${title}”?`, `Что делает «${title}» непригодным к rollout?`, `什么情况会使“${title}”无法上线？`, `무엇이 “${title}” rollout을 무효화하나요?`),
    proof: l(`What proves that “${title}” is ready?`, `Что доказывает готовность «${title}»?`, `什么能证明“${title}”已准备就绪？`, `“${title}” 준비를 무엇으로 증명하나요?`),
    protocol: l(`Which protocol does “${title}” use?`, `Какой протокол использует «${title}»?`, `“${title}”使用什么协议？`, `“${title}”는 어떤 protocol을 사용하나요?`),
  };

  return {
    title,
    h1: title,
    description: localized(topic.description, locale, facts),
    keywords: localizedList(topic.keywords, locale, facts),
    dek: localized(topic.dek, locale, facts),
    sections: [
      {
        h2: localizedTemplate(depthSectionCopy.decision, locale, facts, title),
        blocks: [
          { type: "p", text: localized(topic.dek, locale, facts) },
          {
            type: "table",
            headers: [
              localized(tableCopy.priority, locale, facts),
              localized(tableCopy.action, locale, facts),
              localized(tableCopy.stop, locale, facts),
            ],
            rows: focus.map((action, index) => [`${index + 1}`, action, risks[index]!]),
          },
        ],
      },
      ...depthSections(title, topic.id, topic.kind, focus, facts, locale, true).slice(1),
    ],
    faq: [
      {
        q: localized(questions.decision, locale, facts),
        a: `${focus[0]!} ${focus[1]!}`,
      },
      {
        q: localized(questions.failure, locale, facts),
        a: risks.join(" "),
      },
      {
        q: localized(questions.proof, locale, facts),
        a: `${focus[2]!} ${proof[2]!}`,
      },
      {
        q: localized(questions.protocol, locale, facts),
        a: localized(protocolCopy[topic.kind], locale, facts),
      },
    ],
  };
}

const generatedTopics = [
  ...accessTopics,
  ...modelTopics,
  ...toolTopics,
  ...comparisonTopics,
  ...operationsTopics,
  ...advancedTopics,
];

export const PROVIDER_PARITY_TOPIC_IDS: readonly string[] = generatedTopics.map((topic) => topic.id);

const topicsById = new Map(generatedTopics.map((topic) => [topic.id, topic]));

type GeneratedEntry = {
  provider: ParityProvider;
  topicId: string;
  article: LearnArticle;
  translations: Record<Exclude<Locale, "en">, LocalizedContent>;
};

const generatedEntries: GeneratedEntry[] = generatedTopics.flatMap((topic) =>
  PARITY_PROVIDERS.flatMap((provider) => {
    const facts = PROVIDERS[provider];
    if (topic.providerExisting?.[provider]) return [];
    const slug = articleSlug(topic, facts);
    const en = localizedContent(topic, facts, "en");
    return [{
      provider,
      topicId: topic.id,
      article: {
        slug,
        cluster: topic.cluster,
        ...en,
        related: relatedSlugs(slug, facts),
        published: "2026-08-09",
        updated: "2026-08-09",
      },
      translations: {
        ru: localizedContent(topic, facts, "ru"),
        zh: localizedContent(topic, facts, "zh"),
        ko: localizedContent(topic, facts, "ko"),
      },
    }];
  }),
);

export const learnProviderParityEn: LearnArticle[] = generatedEntries.map((entry) => entry.article);

export const learnProviderParityRu: Record<string, LocalizedContent> = Object.fromEntries(
  generatedEntries.map((entry) => [entry.article.slug, entry.translations.ru]),
);

export const learnProviderParityZh: Record<string, LocalizedContent> = Object.fromEntries(
  generatedEntries.map((entry) => [entry.article.slug, entry.translations.zh]),
);

export const learnProviderParityKo: Record<string, LocalizedContent> = Object.fromEntries(
  generatedEntries.map((entry) => [entry.article.slug, entry.translations.ko]),
);

const existingTopicPages: Record<string, Record<ParityProvider, string>> = {
  buy: {
    gpt: "how-to-buy-gpt-api-key",
    gemini: "how-to-buy-gemini-api-key",
    kimi: "how-to-buy-kimi-api-key",
  },
  quickstart: {
    gpt: "openai-api-quickstart",
    gemini: "gemini-api-quickstart",
    kimi: "kimi-api-quickstart",
  },
  pricing: {
    gpt: "gpt-api-pricing",
    gemini: "gemini-api-pricing",
    kimi: "kimi-api-pricing",
  },
  "model-comparison": {
    gpt: "gpt-5-6-sol-vs-terra-vs-luna",
    gemini: "gemini-pro-vs-flash-vs-flash-lite",
    kimi: "kimi-k3-vs-kimi-for-coding",
  },
};

function topicPages(topicId: string): Record<ParityProvider, string> {
  const existing = existingTopicPages[topicId];
  if (existing) return existing;
  const topic = topicsById.get(topicId);
  if (!topic) throw new Error(`Unknown provider parity topic: ${topicId}`);
  return Object.fromEntries(PARITY_PROVIDERS.map((provider) => {
    const facts = PROVIDERS[provider];
    return [provider, articleSlug(topic, facts)];
  })) as Record<ParityProvider, string>;
}

/**
 * Explicit semantic contract: every article in the original 47-page catalog
 * has a provider-specific GPT, Gemini and Kimi destination.
 */
export const CLAUDE_PROVIDER_PARITY: Record<string, Record<ParityProvider, string>> = {
  "how-to-buy-claude-api-key": topicPages("buy"),
  "cheapest-claude-api": topicPages("cheapest"),
  "claude-api-for-russia": topicPages("restricted-regions"),
  "claude-api-crypto-payment": topicPages("crypto-payment"),
  "claude-api-without-waitlist": topicPages("no-waitlist"),
  "claude-api-quick-setup": topicPages("quickstart"),
  "free-claude-api-key": topicPages("free-key"),
  "claude-api-free-trial": topicPages("free-trial"),
  "claude-code-without-subscription": topicPages("cli-without-subscription"),
  "claude-opus-api": topicPages("flagship-model"),
  "claude-sonnet-api": topicPages("balanced-model"),
  "claude-haiku-api": topicPages("fast-model"),
  "claude-api-key-for-cursor": topicPages("cursor"),
  "claude-api-for-vs-code": topicPages("vscode"),
  "cursor-without-anthropic-account": topicPages("cursor-no-direct"),
  "anthropic-sdk-base-url": topicPages("sdk"),
  "claude-api-langchain": topicPages("langchain"),
  "claude-api-litellm": topicPages("litellm"),
  "claude-api-aider": topicPages("aider"),
  "claude-api-roo-code": topicPages("roo-code"),
  "apitoken-vs-anthropic-direct": topicPages("direct-provider"),
  "apitoken-vs-openrouter": topicPages("openrouter"),
  "claude-opus-vs-sonnet": topicPages("model-comparison"),
  "claude-api-pricing-explained": topicPages("pricing"),
  "save-tokens-on-claude-api": topicPages("save-tokens"),
  "how-billing-works": topicPages("billing"),
  "claude-api-activation-time": topicPages("activation"),
  "claude-api-supported-countries": topicPages("countries"),
  "claude-api-refund-policy": topicPages("refund"),
  "apitoken-vs-proxyapi": topicPages("proxyapi"),
  "apitoken-vs-portkey": topicPages("portkey"),
  "apitoken-vs-litellm": topicPages("litellm-proxy"),
  "best-claude-model-for-coding": topicPages("best-coding-model"),
  "claude-max-plan-vs-api": topicPages("subscription-vs-api"),
  "claude-3-5-vs-claude-4": topicPages("generation-comparison"),
  "why-choose-apitoken": topicPages("why-apitoken"),
  "claude-api-gateway": topicPages("gateway"),
  "claude-api-rate-limits": topicPages("rate-limits"),
  "claude-api-streaming": topicPages("streaming"),
  "claude-api-prompt-caching": topicPages("prompt-caching"),
  "claude-api-best-practices": topicPages("best-practices"),
  "claude-code-api-key": topicPages("cli-setup"),
  "openai-api-quickstart": topicPages("quickstart"),
  "codex-cli-setup": topicPages("cli-setup"),
  "vscode-ai-agents-one-prompt": topicPages("vscode-agents"),
  "claude-api-key-security": topicPages("key-security"),
  "claude-api-for-ai-agents": topicPages("ai-agents"),
};

type ExistingDepthBinding = {
  provider: ParityProvider;
  topicId: "buy" | "quickstart" | "pricing" | "model-comparison" | "cli-setup";
  kind: TopicKind;
};

const EXISTING_DEPTH_BINDINGS: Record<string, ExistingDepthBinding> = {
  "how-to-buy-gpt-api-key": { provider: "gpt", topicId: "buy", kind: "access" },
  "how-to-buy-gemini-api-key": { provider: "gemini", topicId: "buy", kind: "access" },
  "how-to-buy-kimi-api-key": { provider: "kimi", topicId: "buy", kind: "access" },
  "openai-api-quickstart": { provider: "gpt", topicId: "quickstart", kind: "tool" },
  "gemini-api-quickstart": { provider: "gemini", topicId: "quickstart", kind: "tool" },
  "kimi-api-quickstart": { provider: "kimi", topicId: "quickstart", kind: "tool" },
  "gpt-api-pricing": { provider: "gpt", topicId: "pricing", kind: "operations" },
  "gemini-api-pricing": { provider: "gemini", topicId: "pricing", kind: "operations" },
  "kimi-api-pricing": { provider: "kimi", topicId: "pricing", kind: "operations" },
  "gpt-5-6-sol-vs-terra-vs-luna": { provider: "gpt", topicId: "model-comparison", kind: "model" },
  "gemini-pro-vs-flash-vs-flash-lite": { provider: "gemini", topicId: "model-comparison", kind: "model" },
  "kimi-k3-vs-kimi-for-coding": { provider: "kimi", topicId: "model-comparison", kind: "model" },
  "codex-cli-setup": { provider: "gpt", topicId: "cli-setup", kind: "tool" },
  "kimi-api-for-kimi-code": { provider: "kimi", topicId: "cli-setup", kind: "tool" },
};

/** Add the same decision/risk/verification depth to manually authored parity entry pages. */
export function enrichExistingProviderParityContent(
  slug: string,
  locale: Locale,
  content: LocalizedContent,
): LocalizedContent {
  const binding = EXISTING_DEPTH_BINDINGS[slug];
  if (!binding) return content;

  const facts = PROVIDERS[binding.provider];
  const existingFocus = PROVIDER_EXISTING_FOCUS[binding.topicId];
  const generatedFocus = topicsById.get(binding.topicId)?.focus;
  const focusSource = existingFocus ?? generatedFocus;
  if (!focusSource) throw new Error(`Missing provider editorial focus for ${binding.topicId}`);
  const focus = applyTopicModelEconomics(binding.topicId, localizedList(focusSource, locale, facts), facts);
  const proof = localizedList(verificationCopy[binding.kind], locale, facts);
  const title = content.title;
  const faqQuestion = localized(l(
    `What is the rollout gate for “${title}”?`,
    `Какой rollout gate у «${title}»?`,
    `“${title}”的上线门槛是什么？`,
    `“${title}”의 rollout gate는 무엇인가요?`,
  ), locale, facts);

  return {
    ...content,
    sections: [
      ...content.sections,
      ...depthSections(title, binding.topicId, binding.kind, focus, facts, locale, false),
    ],
    faq: [
      ...content.faq,
      { q: faqQuestion, a: `${focus[2]!} ${proof[2]!}` },
    ],
  };
}
