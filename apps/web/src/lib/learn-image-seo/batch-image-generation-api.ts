import type { ImageSeoSpec } from "./shared";
import { faq, list, note, section, sharedCode, steps, table, tr } from "./shared";

export const spec: ImageSeoSpec = {
    slug: "batch-image-generation-api",
    cluster: "integrate",
    related: ["image-generation-api-pricing", "cheapest-image-generation-api", "nano-banana-2-api-cost", "gpt-image-2-api-cost"],
    title: tr(
      "Batch Image Generation API: Cost-Controlled Production Workflow",
      "Batch API генерации изображений: production workflow с контролем цены",
      "批量图像生成 API：成本可控的生产工作流",
      "Batch 이미지 생성 API: 비용 통제 production workflow",
    ),
    h1: tr(
      "Build a safe batch image generation API pipeline",
      "Безопасный batch pipeline для генерации изображений",
      "构建安全的批量图像生成 API 流水线",
      "안전한 batch 이미지 생성 API pipeline 만들기",
    ),
    description: tr(
      "Design a batch AI image pipeline with Nano Banana 2 or GPT Image 2: queues, idempotency, bounded concurrency, spending limits, validation, retries and cost per accepted asset.",
      "Спроектируйте batch AI image pipeline на Nano Banana 2 или GPT Image 2: queues, idempotency, bounded concurrency, spending limits, validation, retries и cost per accepted asset.",
      "使用 Nano Banana 2 或 GPT Image 2 设计批量 AI 图像流水线：队列、幂等、有界并发、消费上限、验证、重试与每个验收资产成本。",
      "Nano Banana 2 또는 GPT Image 2로 queue, idempotency, bounded concurrency, spending limit, validation, retry, accepted asset당 비용을 갖춘 batch pipeline을 설계하세요.",
    ),
    keywords: tr(
      ["batch image generation api", "bulk ai image api", "image generation pipeline", "automated image generation api", "batch image api cost", "high volume image generation"],
      ["batch api генерации изображений", "массовая генерация картинок api", "image generation pipeline", "автоматическая генерация изображений api", "стоимость batch image api", "high volume image generation"],
      ["批量图像生成 api", "批量 ai 图像 api", "图像生成流水线", "自动图像生成 api", "批量图像 api 成本", "大规模图像生成"],
      ["batch 이미지 생성 api", "대량 ai image api", "image generation pipeline", "자동 이미지 생성 api", "batch image api 비용", "대규모 이미지 생성"],
    ),
    dek: tr(
      "Neither public image route is a request for an unbounded number of pictures: one admitted call yields one candidate. A reliable batch system owns the queue, attempt budget, validation and durable attribution outside the model request.",
      "Ни один public image route не принимает запрос на безлимитное число картинок: один admitted call даёт один candidate. Надёжная batch-система сама владеет queue, attempt budget, validation и durable attribution.",
      "两个公开图像路由都不是无限图片请求：一次准入调用只产生一个候选项。可靠批量系统应在模型请求之外管理队列、尝试预算、验证与持久归因。",
      "두 public image route 모두 무제한 이미지 요청이 아니며 admitted call 하나가 candidate 하나를 만듭니다. 신뢰할 batch 시스템은 model request 밖에서 queue, attempt budget, validation, durable attribution을 소유합니다.",
    ),
    sections: [
      section(
        tr("One durable job per asset", "Один durable job на ассет", "每个资产一个持久任务", "asset당 durable job 하나"),
        [
          steps(
            ["Create a stable asset ID and immutable brief before enqueueing.", "Resolve the model, protocol, references, output size and maximum attempts into the job payload.", "Use bounded workers; reserve one provider request for one candidate rather than asking the model for an internal batch.", "Persist request ID, terminal usage, output checksum and validation verdict atomically.", "Mark the job complete only after storage and downstream publication both confirm the same asset version."],
            ["Создайте stable asset ID и immutable brief до enqueue.", "Зафиксируйте в job payload model, protocol, references, output size и maximum attempts.", "Используйте bounded workers: один provider request на один candidate вместо internal batch модели.", "Атомарно сохраните request ID, terminal usage, output checksum и validation verdict.", "Завершайте job только после подтверждения storage и downstream publication одной версии ассета."],
            ["入队前创建稳定 asset ID 与不可变 brief。", "在任务 payload 中固定模型、协议、参考图、输出尺寸与最大尝试次数。", "使用有界 worker；一次提供商请求对应一个候选项，不要求模型内部批量。", "原子保存 request ID、终态 usage、输出 checksum 与验证结论。", "存储与下游发布确认同一资产版本后才完成任务。"],
            ["enqueue 전에 stable asset ID와 immutable brief를 만듭니다.", "job payload에 model, protocol, references, output size, maximum attempts를 고정합니다.", "bounded worker를 쓰고 model internal batch 대신 provider request 하나당 candidate 하나를 예약합니다.", "request ID, terminal usage, output checksum, validation verdict를 원자적으로 저장합니다.", "storage와 downstream publication이 같은 asset version을 확인한 뒤 job을 완료합니다."],
          ),
          sharedCode(`{
  "asset_id": "catalog/sku-1042/hero-v3",
  "model": "gemini-3.1-flash-image",
  "size": "1K",
  "max_attempts": 2,
  "spending_key": "image-production"
}`),
        ],
      ),
      section(
        tr("Bound the multiplication factors", "Ограничьте множители цены", "限制成本倍增因素", "비용 배수 제한"),
        [
          table(
            { headers: ["Multiplier", "Guardrail"], rows: [["Assets", "Explicit queue length and campaign budget"], ["Variants", "Maximum candidates per asset"], ["Retries", "Only proven not-started attempts; total deadline"], ["Resolution", "Default 1K; promote by delivery rule"], ["References", "Only files required by the brief"], ["Concurrency", "Small worker ceiling with 429 cooling"]] },
            { headers: ["Множитель", "Guardrail"], rows: [["Assets", "Явная queue length и campaign budget"], ["Variants", "Maximum candidates per asset"], ["Retries", "Только proven not-started; total deadline"], ["Resolution", "Default 1K; повышение по delivery rule"], ["References", "Только нужные brief файлы"], ["Concurrency", "Небольшой worker ceiling с 429 cooling"]] },
            { headers: ["倍增因素", "保护措施"], rows: [["资产数", "明确队列长度与 campaign 预算"], ["变体", "每个资产最大候选数"], ["重试", "仅明确未开始的尝试；总 deadline"], ["分辨率", "默认 1K；按交付规则升级"], ["参考图", "只发送 brief 必需文件"], ["并发", "小型 worker 上限与 429 cooling"]] },
            { headers: ["배수", "Guardrail"], rows: [["Assets", "명시적 queue 길이와 campaign budget"], ["Variants", "asset당 maximum candidate"], ["Retries", "proven not-started만; total deadline"], ["Resolution", "기본 1K; delivery rule로 승격"], ["References", "brief에 필요한 파일만"], ["Concurrency", "작은 worker ceiling과 429 cooling"]] },
          ),
          note(
            "A 50% B2C discount halves official usage; it does not cap the product of assets × variants × retries × resolution. The key's lifetime spending limit is the final monetary boundary.",
            "B2C-скидка 50% делит official usage пополам, но не ограничивает assets × variants × retries × resolution. Финансовая граница — lifetime spending limit ключа.",
            "B2C 五折会减半官方 usage，但不会限制资产 × 变体 × 重试 × 分辨率的乘积；密钥终身消费上限才是最终资金边界。",
            "B2C 50% 할인은 official usage를 절반으로 줄이지만 assets × variants × retries × resolution 곱을 제한하지 않습니다. key 평생 누적 지출 한도가 최종 금액 경계입니다.",
          ),
        ],
      ),
      section(
        tr("Retry and observability contract", "Контракт retries и observability", "重试与可观测性契约", "retry와 observability 계약"),
        [
          list(
            ["Never retry after image bytes or a complete provider response were delivered.", "Treat an ambiguous timeout as reconciliation work, not proof that no billable generation happened.", "Honor provider cooling and Retry-After instead of widening concurrency during a capacity event.", "Track attempts, accepted assets, settled nanoUSD and validation reasons without putting prompts or keys in metrics.", "Alert on cost per accepted asset and failure share, not only HTTP success rate."],
            ["Не retry после image bytes или complete provider response.", "Ambiguous timeout означает reconciliation, а не доказательство отсутствия billable generation.", "Соблюдайте provider cooling и Retry-After вместо роста concurrency при capacity event.", "Считайте attempts, accepted assets, settled nanoUSD и validation reasons без prompts/keys в metrics.", "Alert по cost per accepted asset и failure share, не только HTTP success rate."],
            ["图像字节或完整提供商响应已交付后绝不重试。", "歧义超时意味着需要核对，不代表没有发生可计费生成。", "容量事件中遵守 provider cooling 与 Retry-After，不要扩大并发。", "统计尝试、验收资产、结算 nanoUSD 与验证原因，metrics 不放提示词或密钥。", "对每个验收资产成本与失败占比告警，而不只看 HTTP 成功率。"],
            ["image bytes나 complete provider response 전달 후에는 retry하지 않습니다.", "ambiguous timeout은 reconciliation 대상이지 billable generation이 없었다는 증거가 아닙니다.", "capacity event에 concurrency를 늘리지 말고 provider cooling과 Retry-After를 따릅니다.", "prompt/key를 metric에 넣지 않고 attempt, accepted asset, settled nanoUSD, validation reason을 추적합니다.", "HTTP success rate뿐 아니라 accepted asset당 비용과 failure share에 alert합니다."],
          ),
        ],
      ),
    ],
    faq: [
      faq(tr("Can one API request generate a whole batch?", "Можно ли сгенерировать весь batch одним API request?", "一个 API 请求能生成整个批次吗？", "API request 하나로 전체 batch를 생성할 수 있나요?"), tr("No on the published routes. One call returns one candidate image; your durable queue should own batch size and concurrency.", "Не на published routes. Один call возвращает один candidate image; размер batch и concurrency принадлежат вашей durable queue.", "已发布路由不支持。一次调用返回一张候选图；批量大小与并发应由持久队列管理。", "공개 route에서는 아닙니다. call 하나가 candidate image 하나를 반환하며 batch 크기와 concurrency는 durable queue가 소유해야 합니다.")),
      faq(tr("How do I stop a runaway image batch?", "Как остановить runaway image batch?", "如何阻止失控的图像批处理？", "runaway image batch를 어떻게 막나요?"), tr("Use a dedicated key with a lifetime spending limit, an explicit queue size, bounded workers, per-asset attempt limits and a total campaign budget.", "Используйте dedicated key с lifetime spending limit, явную queue size, bounded workers, per-asset attempt limits и total campaign budget.", "使用带终身消费上限的独立密钥、明确队列大小、有界 worker、每资产尝试上限与总 campaign 预算。", "평생 누적 지출 한도가 있는 dedicated key, 명시적 queue size, bounded worker, asset당 attempt 제한, total campaign budget을 사용합니다.")),
      faq(tr("What metric matters most for batch economics?", "Какая метрика главная для batch economics?", "批量经济性最重要的指标是什么？", "batch economics에서 가장 중요한 metric은?"), tr("Settled cost per accepted asset. It combines token price, resolution, retries and quality rejection into the unit the business actually needs.", "Settled cost per accepted asset: она объединяет token price, resolution, retries и quality rejects в реальную business unit.", "每个验收资产的结算成本；它把 token 价格、分辨率、重试与质量拒绝合并为业务真正需要的单位。", "accepted asset당 settled cost입니다. token 가격, resolution, retry, quality reject를 실제 business unit으로 합칩니다.")),
      faq(tr("Should 429 responses be retried immediately?", "Нужно ли сразу retry 429?", "429 是否应立即重试？", "429를 즉시 retry해야 하나요?"), tr("No. Respect Retry-After and provider cooling with jitter and a total deadline. Immediate fan-out amplifies capacity exhaustion.", "Нет. Соблюдайте Retry-After/provider cooling, jitter и total deadline. Immediate fan-out усиливает capacity exhaustion.", "不要。应遵守 Retry-After 与 provider cooling，并使用 jitter 和总 deadline；立即扇出会放大容量耗尽。", "아닙니다. Retry-After와 provider cooling을 지키고 jitter와 total deadline을 사용하세요. 즉시 fan-out은 capacity exhaustion을 키웁니다.")),
    ],
  };
