import type { ImageSeoSpec } from "./shared";
import { faq, list, note, paragraph, section, sharedCode, steps, table, tr } from "./shared";

export const spec: ImageSeoSpec = {
    slug: "batch-image-generation-api",
    cluster: "integrate",
    related: ["image-generation-api-pricing", "cheapest-image-generation-api", "nano-banana-2-api-cost", "gpt-image-2-api-cost"],
    title: tr(
      "Batch Image Generation API: Budget-Safe Pipeline",
      "Batch API генерации изображений: контроль бюджета",
      "批量图像生成 API：成本可控的生产工作流",
      "Batch 이미지 생성 API: 비용 통제 production workflow",
    ),
    h1: tr(
      "Build a batch image generation pipeline that cannot overrun its budget",
      "Batch pipeline генерации изображений, который не выйдет за бюджет",
      "构建不会超支的批量图像生成流水线",
      "예산을 초과하지 않는 batch 이미지 생성 pipeline 만들기",
    ),
    description: tr(
      "Batch image generation API done safely: durable queues, idempotency, bounded concurrency, 500-SKU cost math and key-level spending limits.",
      "Batch API генерации изображений без перерасхода: durable queues, idempotency, ограниченная concurrency, расчёт прогона на 500 SKU и лимиты ключа.",
      "使用 Nano Banana 2 或 GPT Image 2 设计批量 AI 图像流水线：一次调用一个候选项、持久队列、幂等、有界并发、成本演算与密钥级消费上限。",
      "Nano Banana 2 또는 GPT Image 2 기반 batch AI image pipeline: call당 candidate 하나, durable queue, idempotency, bounded concurrency, 비용 계산, key-level spending limit.",
    ),
    keywords: tr(
      ["batch image generation api", "bulk ai image api", "image generation pipeline", "automated image generation api", "batch image api cost", "high volume image generation"],
      ["batch api генерации изображений", "массовая генерация картинок api", "image generation pipeline", "автоматическая генерация изображений api", "стоимость batch image api", "high volume image generation"],
      ["批量图像生成 api", "批量 ai 图像 api", "图像生成流水线", "自动图像生成 api", "批量图像 api 成本", "大规模图像生成"],
      ["batch 이미지 생성 api", "대량 ai image api", "image generation pipeline", "자동 이미지 생성 api", "batch image api 비용", "대규모 이미지 생성"],
    ),
    dek: tr(
      "There is no batch endpoint to call. On both published image routes, one admitted request returns exactly one candidate image, so a production batch pipeline is your own durable queue in front of Nano Banana 2 (gemini-3.1-flash-image) or GPT Image 2 — with bounded workers, per-asset attempt budgets, terminal-usage reconciliation and a key-level lifetime spending limit as the final monetary boundary.",
      "Batch-эндпоинта не существует. На обоих public image routes один admitted request возвращает ровно один candidate image, поэтому production batch pipeline — это ваша собственная durable queue перед Nano Banana 2 (gemini-3.1-flash-image) или GPT Image 2: bounded workers, per-asset attempt budget, сверка terminal usage и lifetime spending limit ключа как последняя денежная граница.",
      "不存在所谓的批量端点。在两条公开图像路由上，一次准入请求只返回一张候选图，因此生产级批量流水线本质上是你在 Nano Banana 2（gemini-3.1-flash-image）或 GPT Image 2 之前自建的持久队列——配有有界 worker、每资产尝试预算、终态 usage 核对，以及作为最终资金边界的密钥终身消费上限。",
      "호출할 수 있는 batch endpoint는 없습니다. 두 public image route 모두 admitted request 하나가 정확히 candidate image 하나를 반환하므로, production batch pipeline은 Nano Banana 2(gemini-3.1-flash-image) 또는 GPT Image 2 앞에 두는 자체 durable queue입니다. bounded worker, asset당 attempt budget, terminal usage 대조, 그리고 최종 금액 경계인 key 평생 누적 지출 한도를 갖춰야 합니다.",
    ),
    sections: [
      section(
        tr("One admitted call returns one candidate", "Один admitted call — один candidate", "一次准入调用只返回一个候选项", "admitted call 하나, candidate 하나"),
        [
          paragraph(
            "Neither public image route accepts a count, a manifest or a folder of prompts. A Gemini generateContent call on gemini-3.1-flash-image returns one candidate whose image travels as base64 in inlineData; an OpenAI-compatible POST /v1/images/generations on gpt-image-2 returns a single-element data array with b64_json. The model never sees \"make 500 variations\" — batch size is a property of your queue, not of the request.",
            "Ни один public image route не принимает count, манифест или папку промптов. Вызов generateContent для gemini-3.1-flash-image возвращает один candidate, чьё изображение передаётся как base64 в inlineData; OpenAI-compatible POST /v1/images/generations для gpt-image-2 возвращает массив data из одного элемента с b64_json. Модель никогда не видит «сделай 500 вариаций» — размер batch определяется вашей queue, а не запросом.",
            "两条公开图像路由都不接受数量、清单或提示词文件夹。对 gemini-3.1-flash-image 的 generateContent 调用返回一个候选项，图像以 base64 形式放在 inlineData 中；OpenAI 兼容的 POST /v1/images/generations（gpt-image-2）返回单元素 data 数组，内含 b64_json。模型永远看不到“生成 500 个变体”——批量大小是队列的属性，不是请求的属性。",
            "어떤 public image route도 count, manifest, prompt 폴더를 받지 않습니다. gemini-3.1-flash-image의 generateContent call은 이미지가 inlineData에 base64로 담긴 candidate 하나를 반환하고, OpenAI 호환 POST /v1/images/generations(gpt-image-2)는 b64_json을 담은 단일 원소 data 배열을 반환합니다. 모델은 \"500개 변형을 만들어 달라\"를 보지 않습니다. batch 크기는 request가 아니라 queue의 속성입니다.",
          ),
          sharedCode(`// gemini-3.1-flash-image → one candidate, image as base64 inlineData
{
  "candidates": [
    { "content": { "parts": [ { "inlineData": { "mimeType": "image/png", "data": "<base64>" } } ] } }
  ],
  "usageMetadata": { "promptTokenCount": 24, "candidatesTokenCount": 1120, "totalTokenCount": 1144 }
}

// gpt-image-2 → single-element data array, image as b64_json
{
  "created": 1754800000,
  "data": [ { "b64_json": "<base64>" } ],
  "usage": { "input_tokens": 38, "output_tokens": 4096, "total_tokens": 4134 }
}`),
          paragraph(
            "Plan storage for both shapes at design time: decode the base64 payload once, checksum the bytes and persist them beside the job. A URL-shaped output, when a route returns one, is a fetch task with its own expiry — not a durable asset.",
            "Спроектируйте хранение обоих форматов заранее: декодируйте base64 один раз, посчитайте checksum байтов и сохраните их рядом с job. URL-формат ответа, если route его возвращает, — это отдельная fetch-задача со своим сроком жизни, а не durable asset.",
            "设计阶段就要为两种形态规划存储：base64 只解码一次，对字节计算 checksum 并与任务一起持久化。若路由返回 URL 形态，那是一项有独立过期时间的抓取任务，而不是持久资产。",
            "두 형태의 저장은 설계 단계에서 계획하세요. base64는 한 번만 decode해 byte checksum을 계산하고 job과 함께 보관합니다. route가 URL 형태를 반환한다면 그것은 자체 만료가 있는 fetch 작업이지 durable asset이 아닙니다.",
          ),
        ],
      ),
      section(
        tr("One durable job per asset, not per prompt", "Один durable job на ассет, а не на prompt", "每个资产一个持久任务，而不是每个提示词", "prompt가 아니라 asset당 durable job 하나"),
        [
          steps(
            ["Create a stable asset ID and an immutable brief — prompt, references, protected traits — before anything is enqueued.", "Resolve the model, protocol, output size and maximum attempts into the job payload, so a retry replays the same decision rather than making a new one.", "Give each worker a bounded slice of the queue and reserve one provider request per candidate; never ask the model for an internal batch.", "Persist the request ID, terminal usage, output checksum and validation verdict atomically with the asset version.", "Mark the job complete only after storage and downstream publication both confirm the same asset version."],
            ["Создайте stable asset ID и immutable brief — prompt, references, protected traits — до того, как что-либо попадёт в queue.", "Зафиксируйте в job payload model, protocol, output size и maximum attempts, чтобы retry воспроизводил то же решение, а не принимал новое.", "Дайте каждому worker ограниченный срез queue и резервируйте один provider request на один candidate; не просите модель о внутреннем batch.", "Атомарно сохраняйте request ID, terminal usage, output checksum и validation verdict вместе с версией ассета.", "Завершайте job только после того, как storage и downstream publication подтвердили одну и ту же версию ассета."],
            ["入队前先创建稳定 asset ID 与不可变 brief——提示词、参考图、受保护特征。", "在任务 payload 中固定模型、协议、输出尺寸与最大尝试次数，让重试复现同一决策而不是新决策。", "给每个 worker 分配有界的队列切片，一次提供商请求只对应一个候选项；绝不要求模型内部批量。", "把 request ID、终态 usage、输出 checksum 与验证结论随资产版本原子保存。", "只有存储与下游发布确认同一资产版本后，任务才算完成。"],
            ["무엇이든 enqueue하기 전에 stable asset ID와 immutable brief(prompt, references, protected traits)를 만듭니다.", "job payload에 model, protocol, output size, maximum attempts를 고정해 retry가 새 결정이 아니라 같은 결정을 재현하게 합니다.", "각 worker에 queue의 bounded slice를 주고 provider request 하나당 candidate 하나를 예약합니다. 모델에 internal batch를 요구하지 않습니다.", "request ID, terminal usage, output checksum, validation verdict를 asset version과 함께 원자적으로 저장합니다.", "storage와 downstream publication이 같은 asset version을 확인한 뒤에만 job을 완료합니다."],
          ),
          sharedCode(`{
  "asset_id": "catalog/sku-1042/hero-v3",
  "idempotency_key": "sku-1042:hero-v3:attempt-1",
  "model": "gemini-3.1-flash-image",
  "size": "1K",
  "max_attempts": 2,
  "spending_key": "image-production"
}`),
        ],
      ),
      section(
        tr("Cost math for a 500-SKU run", "Математика цены для прогона на 500 SKU", "500 个 SKU 批次的成本演算", "SKU 500개 run의 비용 계산"),
        [
          paragraph(
            "Nano Banana 2 publishes fixed image-output legs: 1K is 1,120 image tokens, $0.0672 officially and $0.0336 for a regular B2C account; after the same 50% policy, 2K is $0.0504 and 4K is $0.0756. GPT Image 2 has no honest per-picture constant: its image output bills at $15 per 1M tokens for regular B2C, and the settled total follows terminal usage. Work the example with the fixed leg, because that is the only part you can know before the run.",
            "У Nano Banana 2 опубликованы фиксированные image-output составляющие: 1K — это 1 120 image tokens, официально $0.0672 и $0.0336 для обычного B2C; после той же политики 50% 2K стоит $0.0504, а 4K — $0.0756. У GPT Image 2 нет честной константы за картинку: image output тарифицируется по $15 за 1M tokens для обычного B2C, а итог определяет terminal usage. Считайте пример по фиксированной составляющей — это единственная часть, известная до прогона.",
            "Nano Banana 2 公布了固定的图像输出计费项：1K 为 1,120 个 image token，官方 $0.0672，普通 B2C 为 $0.0336；同样五折后 2K 为 $0.0504，4K 为 $0.0756。GPT Image 2 没有诚实的单张常数：普通 B2C 的 image output 按每 1M token $15 计费，结算总额以终态 usage 为准。演算应按固定计费项进行，因为这是运行前唯一可知的部分。",
            "Nano Banana 2는 고정 image-output leg를 공개합니다. 1K는 image token 1,120개로 공식 $0.0672, 일반 B2C는 $0.0336이며 같은 50% 정책으로 2K는 $0.0504, 4K는 $0.0756입니다. GPT Image 2에는 정직한 이미지당 상수가 없습니다. 일반 B2C image output은 1M token당 $15이고 정산 총액은 terminal usage를 따릅니다. run 전에 알 수 있는 부분은 고정 leg뿐이므로 이 값으로 계산하세요.",
          ),
          table(
            { headers: ["Line", "Value"], rows: [["Assets in the campaign", "500 SKUs"], ["Candidates per asset", "2"], ["Admitted calls", "1,000"], ["Image output per 1K candidate (regular B2C)", "$0.0336"], ["Base image-output spend", "1,000 × $0.0336 = $33.60"], ["Retry budget: 10% of assets, one extra attempt", "+$3.36"], ["Worst-case image-output spend", "$36.96"]] },
            { headers: ["Строка", "Значение"], rows: [["Ассеты в кампании", "500 SKU"], ["Candidates на ассет", "2"], ["Admitted calls", "1 000"], ["Image output за 1K candidate (обычный B2C)", "$0.0336"], ["Базовый image-output spend", "1 000 × $0.0336 = $33.60"], ["Бюджет retry: 10% ассетов, одна extra attempt", "+$3.36"], ["Худший случай image-output spend", "$36.96"]] },
            { headers: ["项目", "数值"], rows: [["campaign 中的资产", "500 个 SKU"], ["每资产候选数", "2"], ["准入调用数", "1,000"], ["每个 1K 候选项的图像输出（普通 B2C）", "$0.0336"], ["基础图像输出支出", "1,000 × $0.0336 = $33.60"], ["重试预算：10% 资产各增加一次尝试", "+$3.36"], ["最坏情况图像输出支出", "$36.96"]] },
            { headers: ["항목", "값"], rows: [["campaign의 asset", "SKU 500개"], ["asset당 candidate", "2"], ["admitted call", "1,000"], ["1K candidate당 image output(일반 B2C)", "$0.0336"], ["기본 image-output 지출", "1,000 × $0.0336 = $33.60"], ["retry 예산: asset 10%에 attempt 1회 추가", "+$3.36"], ["최악의 image-output 지출", "$36.96"]] },
          ),
          note(
            "This is the image-output leg only. Text and image input, optional text or thinking output, and grounding are added from terminal usage. The 50% B2C discount halves official usage; it does not cap the product of assets × candidates × retries × resolution — that product is yours to bound.",
            "Это только image-output составляющая. Text/image input, возможный text/thinking output и grounding добавляются из terminal usage. Скидка 50% для B2C делит official usage пополам, но не ограничивает произведение assets × candidates × retries × resolution — его границы задаёте вы.",
            "以上仅为图像输出项；text/image input、可选文本或思考输出以及 grounding 需按终态 usage 另计。B2C 五折会把官方 usage 减半，但不会限制 资产 × 候选 × 重试 × 分辨率 的乘积——它的边界由你来设定。",
            "이것은 image-output leg뿐입니다. text/image input, 선택적 text/thinking output, grounding은 terminal usage에서 더해집니다. B2C 50% 할인은 official usage를 절반으로 줄이지만 asset × candidate × retry × resolution의 곱을 제한하지 않습니다. 그 경계는 직접 정해야 합니다.",
          ),
          tr(
            { type: "link", text: "The per-leg token rates behind this batch math", href: "/docs/learn/image-generation-api-pricing" },
            { type: "link", text: "Ставки за tokens, на которых строится этот расчёт batch", href: "/docs/learn/image-generation-api-pricing" },
            { type: "link", text: "支撑该批量演算的各项 token 费率", href: "/docs/learn/image-generation-api-pricing" },
            { type: "link", text: "이 batch 계산의 근거가 되는 token 요금", href: "/docs/learn/image-generation-api-pricing" },
          ),
        ],
      ),
      section(
        tr("Bound every multiplier, not just the price", "Ограничьте каждый множитель, а не только цену", "限制每一个倍增因素，而不只是单价", "단가뿐 아니라 모든 배수를 제한"),
        [
          table(
            { headers: ["Multiplier", "Guardrail"], rows: [["Assets", "Explicit queue length and campaign budget"], ["Variants", "Maximum candidates per asset"], ["Retries", "Only proven not-started attempts; total deadline"], ["Resolution", "Default 1K; promote by delivery rule"], ["References", "Only files required by the brief"], ["Concurrency", "Small worker ceiling with 429 cooling"]] },
            { headers: ["Множитель", "Guardrail"], rows: [["Assets", "Явная queue length и campaign budget"], ["Variants", "Maximum candidates per asset"], ["Retries", "Только proven not-started; total deadline"], ["Resolution", "Default 1K; повышение по delivery rule"], ["References", "Только нужные brief файлы"], ["Concurrency", "Небольшой worker ceiling с 429 cooling"]] },
            { headers: ["倍增因素", "保护措施"], rows: [["资产数", "明确队列长度与 campaign 预算"], ["变体", "每个资产最大候选数"], ["重试", "仅明确未开始的尝试；总 deadline"], ["分辨率", "默认 1K；按交付规则升级"], ["参考图", "只发送 brief 必需文件"], ["并发", "小型 worker 上限与 429 cooling"]] },
            { headers: ["배수", "Guardrail"], rows: [["Assets", "명시적 queue 길이와 campaign budget"], ["Variants", "asset당 maximum candidate"], ["Retries", "proven not-started만; total deadline"], ["Resolution", "기본 1K; delivery rule로 승격"], ["References", "brief에 필요한 파일만"], ["Concurrency", "작은 worker ceiling과 429 cooling"]] },
          ),
          note(
            "A key's lifetime spending limit is the last monetary boundary when every application-level guardrail fails. Set it to the campaign budget plus a measured safety margin — not to the account balance.",
            "Lifetime spending limit ключа — последняя денежная граница, если откажут все application-level guardrails. Ставьте его равным бюджету кампании плюс измеренный запас, а не балансу аккаунта.",
            "密钥的终身消费上限是所有应用层保护措施失效后的最后一道资金边界。把它设为 campaign 预算加实测安全余量，而不是账户余额。",
            "key의 평생 누적 지출 한도는 모든 application-level guardrail이 실패했을 때의 마지막 금액 경계입니다. 계정 잔액이 아니라 campaign 예산에 측정된 안전 마진을 더한 값으로 설정하세요.",
          ),
        ],
      ),
      section(
        tr("Retry, cooling and observability rules", "Правила retry, cooling и observability", "重试、冷却与可观测性规则", "retry, cooling, observability 규칙"),
        [
          list(
            ["Never retry after image bytes or a complete provider response were delivered — that retry is a second paid candidate, not a recovery.", "Treat an ambiguous timeout as reconciliation work: match the request ID against the dashboard charge before concluding that no billable generation happened.", "Honor 429 responses with Retry-After, provider cooling and jitter inside a total deadline; immediate fan-out amplifies a capacity event.", "Track attempts, accepted assets, settled nanoUSD and validation failure reasons; keep prompts and keys out of metrics.", "Alert on cost per accepted asset and failure share, not only on HTTP success rate — a fully green batch can still be an expensive one."],
            ["Никогда не делайте retry после доставки image bytes или complete provider response — такой retry оплачивает второй candidate, а не восстановление.", "Ambiguous timeout — это reconciliation: сверьте request ID с charge в дашборде, прежде чем решить, что billable generation не было.", "На 429 соблюдайте Retry-After, provider cooling и jitter в пределах total deadline; немедленный fan-out усиливает capacity event.", "Считайте attempts, accepted assets, settled nanoUSD и причины validation failures; не допускайте prompts и keys в metrics.", "Алертьте на cost per accepted asset и failure share, а не только на HTTP success rate — полностью зелёный batch может оказаться дорогим."],
            ["图像字节或完整提供商响应交付后绝不重试——这种重试是第二次付费候选项，而不是恢复。", "歧义超时属于核对工作：先按 request ID 对照仪表板扣费，再判断没有发生可计费生成。", "遇到 429 时遵守 Retry-After、provider cooling 与 jitter，并受总 deadline 约束；立即扇出会放大容量事件。", "统计尝试次数、验收资产、结算 nanoUSD 与验证失败原因；提示词和密钥不进入 metrics。", "对每个验收资产成本与失败占比告警，而不只看 HTTP 成功率——全绿的批次也可能很昂贵。"],
            ["image bytes나 complete provider response가 전달된 후에는 절대 retry하지 않습니다. 그 retry는 복구가 아니라 두 번째 유료 candidate입니다.", "ambiguous timeout은 reconciliation 작업입니다. billable generation이 없었다고 결론 내리기 전에 request ID를 dashboard charge와 대조하세요.", "429에는 Retry-After, provider cooling, jitter를 total deadline 안에서 적용합니다. 즉시 fan-out은 capacity event를 키웁니다.", "attempt, accepted asset, settled nanoUSD, validation 실패 사유를 추적하고 prompt와 key는 metric에 넣지 않습니다.", "HTTP success rate뿐 아니라 accepted asset당 비용과 failure share에 alert하세요. 전부 초록인 batch도 비쌀 수 있습니다."],
          ),
          note(
            "Settled cost per accepted asset is the batch's real unit economics: it folds token price, resolution, retries and quality rejects into the single number the business actually buys.",
            "Settled cost per accepted asset — реальная unit economics batch: она складывает token price, resolution, retries и quality rejects в единственное число, которое бизнес действительно покупает.",
            "每个验收资产的结算成本才是批次真实的单位经济性：它把 token 价格、分辨率、重试与质量拒绝折进业务真正购买的那个数字。",
            "accepted asset당 settled cost가 batch의 실제 unit economics입니다. token 가격, resolution, retry, quality reject를 비즈니스가 실제로 사는 숫자 하나로 합칩니다.",
          ),
        ],
      ),
      section(
        tr("Isolate the batch lane with its own key and budget", "Изолируйте batch lane отдельным ключом и бюджетом", "用独立密钥与预算隔离批量通道", "전용 key와 예산으로 batch lane 격리"),
        [
          paragraph(
            "Run the batch worker on a dedicated key with a lifetime spending limit and an expiration date, so a queue bug can exhaust only the campaign budget, never the account. A new account created with Google or GitHub starts with $5 of platform bonus credit — enough to validate the pipeline end to end before the first top-up; after that, fund any whole-dollar amount by bank card or cryptocurrency such as USDT or BTC. The prepaid balance never expires, and there is no subscription to size.",
            "Запускайте batch worker на dedicated key с lifetime spending limit и expiration date: тогда баг в queue потратит максимум бюджет кампании, но не аккаунт. Новый аккаунт через Google или GitHub получает $5 platform bonus credit — этого хватит, чтобы проверить pipeline end-to-end до первого пополнения; дальше баланс пополняется на любую целую сумму в долларах банковской картой или криптовалютой, например USDT или BTC. Prepaid balance не сгорает, а подписки, которую нужно подбирать по объёму, нет.",
            "让批量 worker 使用带终身消费上限和过期时间的独立密钥：队列出 bug 时最多耗尽 campaign 预算，而不会动到账户。通过 Google 或 GitHub 创建的新账户自带 $5 平台欢迎奖励余额，足够在首次充值前把流水线端到端跑通；之后可按任意整数美元金额用银行卡或加密货币（如 USDT、BTC）充值。预付余额永不过期，也没有需要按量选择的订阅。",
            "batch worker는 평생 누적 지출 한도와 expiration date가 있는 전용 key로 실행하세요. queue 버그가 터져도 campaign 예산만 소진하고 계정은 건드리지 않습니다. Google이나 GitHub로 만든 새 계정에는 $5 플랫폼 웰컴 보너스 크레딧이 포함되어 첫 충전 전에 pipeline을 end-to-end로 검증할 수 있고, 이후에는 은행 카드나 USDT, BTC 같은 암호화폐로 원하는 정수 달러 금액을 충전합니다. prepaid 잔액은 만료되지 않고 규모를 맞춰야 하는 구독도 없습니다.",
          ),
          list(
            ["One key per workload: batch generation never shares a key with interactive or editing traffic.", "Reconcile every settled charge against its request ID before the campaign is closed.", "Re-check the lifetime spending limit before each new campaign, not once at setup."],
            ["Один ключ на workload: batch generation никогда не делит ключ с interactive или editing трафиком.", "Сверяйте каждый settled charge с его request ID до закрытия кампании.", "Перепроверяйте lifetime spending limit перед каждой новой кампанией, а не один раз при настройке."],
            ["每个工作负载一把密钥：批量生成绝不与交互或编辑流量共用密钥。", "campaign 结束前，把每笔结算扣费与其 request ID 逐一对账。", "每个新 campaign 开始前重新检查终身消费上限，而不是只在初始化时设置一次。"],
            ["workload당 key 하나: batch generation은 interactive나 editing 트래픽과 key를 공유하지 않습니다.", "campaign을 닫기 전에 모든 settled charge를 request ID와 대조합니다.", "초기 설정 때 한 번이 아니라 새 campaign마다 평생 누적 지출 한도를 다시 확인합니다."],
          ),
          tr(
            { type: "link", text: "Prepaid plans that never expire: fund exactly the campaign budget", href: "/plans" },
            { type: "link", text: "Prepaid-тарифы без сгорания: вносите ровно бюджет кампании", href: "/plans" },
            { type: "link", text: "永不过期的预付方案：正好充入活动预算", href: "/plans" },
            { type: "link", text: "만료 없는 prepaid 요금제: campaign 예산만큼 정확히 충전", href: "/plans" },
          ),
        ],
      ),
    ],
    faq: [
      faq(
        tr("Can one API request generate a whole batch of images?", "Можно ли сгенерировать весь batch одним API request?", "一个 API 请求能生成整个批次吗？", "API request 하나로 전체 batch를 생성할 수 있나요?"),
        tr("No, on the published routes. One admitted call returns one candidate image; there is no count parameter that multiplies it. Batch size, ordering and concurrency belong to your durable queue — which is also where budgets are enforced before any money moves.", "Нет, не на published routes. Один admitted call возвращает один candidate image; параметра count, который его умножает, нет. Размер, порядок и concurrency batch принадлежат вашей durable queue — именно там бюджет проверяется до того, как потрачены деньги.", "在已发布路由上不能。一次准入调用返回一张候选图，没有可以放大它的 count 参数。批次大小、顺序与并发属于持久队列——预算也正是在那里、在资金发生变动之前被执行。", "공개 route에서는 불가능합니다. admitted call 하나가 candidate image 하나를 반환하며 이를 늘리는 count 파라미터는 없습니다. batch 크기, 순서, concurrency는 durable queue의 영역이며, 예산도 돈이 움직이기 전에 바로 그곳에서 집행됩니다."),
      ),
      faq(
        tr("How much would a 1,000-image batch cost?", "Сколько стоит batch из 1 000 изображений?", "1,000 张图的批次要多少钱？", "이미지 1,000장 batch 비용은 얼마인가요?"),
        tr("On Nano Banana 2 at 1K, the fixed image-output leg is 1,120 tokens per candidate — $0.0336 for regular B2C — so 1,000 candidates cost $33.60 of image output, plus input and optional grounding legs from terminal usage. GPT Image 2 bills image output at $15 per 1M tokens for regular B2C with no fixed per-image constant, so its batch total is known only from settled usage.", "Для Nano Banana 2 в 1K фиксированная image-output составляющая — 1 120 tokens на candidate, то есть $0.0336 для обычного B2C: 1 000 candidates стоят $33.60 image output плюс input и возможный grounding из terminal usage. GPT Image 2 тарифицирует image output по $15 за 1M tokens для обычного B2C без фиксированной цены картинки, поэтому итог batch известен только по settled usage.", "Nano Banana 2 在 1K 下每个候选项的固定图像输出项为 1,120 token，普通 B2C 即 $0.0336：1,000 个候选项的图像输出为 $33.60，另加终态 usage 中的输入与可选 grounding。GPT Image 2 普通 B2C 的 image output 为每 1M token $15，没有固定单张价格，批次总额只能按结算 usage 得出。", "Nano Banana 2 1K 기준 candidate당 고정 image-output leg는 1,120 token, 일반 B2C로 $0.0336이므로 candidate 1,000개의 image output은 $33.60이고 terminal usage의 input과 선택적 grounding이 추가됩니다. GPT Image 2는 일반 B2C image output이 1M token당 $15로 이미지당 고정 가격이 없어 batch 총액은 settled usage로만 알 수 있습니다."),
      ),
      faq(
        tr("How do I stop a runaway image batch?", "Как остановить runaway image batch?", "如何阻止失控的图像批处理？", "runaway image batch를 어떻게 막나요?"),
        tr("Combine a dedicated key carrying a lifetime spending limit, an explicit queue length, bounded workers, a per-asset maximum attempt count and a total campaign budget. Each layer fails independently, and the key-level limit is the boundary that holds even if all the others misbehave.", "Скомбинируйте dedicated key с lifetime spending limit, явную queue length, bounded workers, per-asset maximum attempts и total campaign budget. Каждый слой отказывает независимо, а key-level limit — граница, которая держится, даже если все остальные сломались.", "组合使用：带终身消费上限的独立密钥、明确队列长度、有界 worker、每资产最大尝试次数与总 campaign 预算。各层独立失效，而密钥级上限是即使其他全部失灵仍然成立的边界。", "평생 누적 지출 한도가 있는 전용 key, 명시적 queue 길이, bounded worker, asset당 maximum attempts, total campaign budget을 조합하세요. 각 계층은 독립적으로 실패하고, key-level 한도는 나머지가 모두 고장 나도 유지되는 경계입니다."),
      ),
      faq(
        tr("Should 429 responses be retried immediately?", "Нужно ли немедленно retry 429?", "429 应该立即重试吗？", "429를 즉시 retry해야 하나요?"),
        tr("No. Respect Retry-After and provider cooling, add jitter and keep a total deadline per job. Immediate fan-out during a capacity event converts a slowdown into an outage you paid to create.", "Нет. Соблюдайте Retry-After и provider cooling, добавляйте jitter и держите total deadline на job. Немедленный fan-out во время capacity event превращает замедление в outage, который вы сами оплатили.", "不应该。遵守 Retry-After 与 provider cooling，加入 jitter，并为每个任务保留总 deadline。容量事件中的立即扇出会把减速变成你自己花钱造成的故障。", "아닙니다. Retry-After와 provider cooling을 지키고 jitter를 더하며 job마다 total deadline을 유지하세요. capacity event 중 즉시 fan-out은 속도 저하를 직접 돈 내고 만든 장애로 바꿉니다."),
      ),
      faq(
        tr("Should the pipeline store the base64 payload or a URL?", "Хранить base64 payload или URL?", "流水线应存 base64 还是 URL？", "base64 payload와 URL 중 무엇을 저장해야 하나요?"),
        tr("Treat the API response as transport, not storage. Decode base64 once, checksum and persist the bytes in your own object storage, then publish optimized WebP/AVIF derivatives; never serve the raw API payload as a storefront asset.", "Ответ API — это транспорт, а не хранилище. Декодируйте base64 один раз, посчитайте checksum и сохраните байты в собственном object storage, затем публикуйте оптимизированные WebP/AVIF derivatives; сырый payload API нельзя отдавать как storefront asset.", "API 响应是传输层而不是存储层。base64 只解码一次，计算 checksum 后把字节存入自有对象存储，再发布优化过的 WebP/AVIF 衍生品；绝不能把原始 API payload 直接当店面资产。", "API 응답은 저장소가 아니라 전송 수단입니다. base64는 한 번만 decode해 checksum을 계산하고 byte를 자체 object storage에 보관한 뒤 최적화된 WebP/AVIF derivative를 게시하세요. 날것의 API payload를 storefront asset으로 제공하면 안 됩니다."),
      ),
      faq(
        tr("Does the 50% discount apply to every call in a batch?", "Скидка 50% действует на каждый call в batch?", "五折适用于批次中的每次调用吗？", "50% 할인이 batch의 모든 call에 적용되나요?"),
        tr("For regular B2C accounts it applies to the official usage of every admitted call, generation or edit. B2B accounts follow their negotiated policy, and OpenKeys bill 1:1 at official prices. A discount never proves that a model is currently available to a particular key.", "Для обычных B2C — да, на official usage каждого admitted call, generation или edit. У B2B действует согласованная политика, а OpenKeys тарифицируются 1:1 по официальной цене. Скидка никогда не доказывает, что модель сейчас доступна конкретному ключу.", "普通 B2C 适用：每次准入调用的官方 usage（生成或编辑）都按五折计。B2B 按协商策略，OpenKeys 按官方价格 1:1。折扣从不证明某个模型当前对特定密钥可用。", "일반 B2C에는 generation과 edit을 포함한 모든 admitted call의 official usage에 적용됩니다. B2B는 협상 정책, OpenKeys는 공식 가격 1:1입니다. 할인은 특정 key에 모델이 현재 사용 가능함을 증명하지 않습니다."),
      ),
    ],
  };
