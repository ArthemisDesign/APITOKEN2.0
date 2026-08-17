import type { ImageSeoSpec } from "./shared";
import { faq, list, note, paragraph, section, sharedCode, steps, table, tr, ROUTER, OPENAI } from "./shared";

export const spec: ImageSeoSpec = {
    slug: "cheapest-image-generation-api",
    cluster: "free",
    related: ["image-generation-api-pricing", "nano-banana-2-vs-gpt-image-2", "nano-banana-2-api-cost", "gpt-image-2-api-cost"],
    title: tr(
      "Cheapest Image Generation API: Compare Real Workflow Cost",
      "Самый дешёвый API генерации изображений: сравнение реальной цены workflow",
      "最便宜的图像生成 API：比较真实工作流成本",
      "가장 저렴한 이미지 생성 API: 실제 workflow 비용 비교",
    ),
    h1: tr(
      "How to find the cheapest image generation API",
      "Как найти самый дешёвый API генерации изображений",
      "如何找到最便宜的图像生成 API",
      "가장 저렴한 이미지 생성 API 찾기",
    ),
    description: tr(
      "Find the cheapest AI image API for your workload by comparing Nano Banana 2 and GPT Image 2 on real request shapes, settled usage, acceptance rate and apiToken.sale's 50% B2C discount.",
      "Найдите самый дешёвый AI image API для своей задачи: сравните Nano Banana 2 и GPT Image 2 по реальным request shapes, settled usage, acceptance rate и B2C-скидке apiToken.sale 50%.",
      "按真实请求形态、结算 usage、验收率与 apiToken.sale B2C 五折比较 Nano Banana 2 和 GPT Image 2，找到适合你工作负载的最低成本图像 API。",
      "실제 request shape, settled usage, acceptance rate와 apiToken.sale B2C 50% 할인으로 Nano Banana 2와 GPT Image 2를 비교해 workload에 가장 저렴한 API를 찾으세요.",
    ),
    keywords: tr(
      ["cheapest image generation api", "cheap ai image api", "lowest cost image generator api", "nano banana vs gpt image price", "affordable image api", "image api discount"],
      ["дешевый api генерации изображений", "самый дешевый ai image api", "недорогой image generator api", "nano banana vs gpt image цена", "доступный image api", "скидка image api"],
      ["最便宜图像生成 api", "低成本 ai 图像 api", "最低成本图像生成器 api", "nano banana vs gpt image 价格", "实惠图像 api", "图像 api 折扣"],
      ["가장 저렴한 이미지 생성 api", "저렴한 ai image api", "최저 비용 image generator api", "nano banana vs gpt image 가격", "합리적인 image api", "image api 할인"],
    ),
    dek: tr(
      "The cheapest image API is the one that delivers an accepted asset for the least settled money, not the one with the lowest headline price. On apiToken.sale both candidates — Nano Banana 2 and GPT Image 2 — already carry the same flat 50% B2C discount, so the decision comes from request shape, retries and acceptance rate.",
      "Самый дешёвый image API — тот, что выдаёт принятый ассет за наименьшую settled-сумму, а не тот, у кого ниже headline-цена. На apiToken.sale оба кандидата — Nano Banana 2 и GPT Image 2 — уже имеют одинаковую B2C-скидку 50%, поэтому решение определяют request shape, retries и acceptance rate.",
      "最便宜的图像 API，是以最低结算金额产出可验收资产的 API，而不是标价最低的 API。在 apiToken.sale 上，两个候选——Nano Banana 2 与 GPT Image 2——都享受同样的 B2C 五折，因此决策取决于请求形态、重试次数与验收率。",
      "가장 저렴한 이미지 API는 headline 가격이 아니라 가장 적은 settled 금액으로 accepted asset을 전달하는 API입니다. apiToken.sale에서 두 후보 — Nano Banana 2와 GPT Image 2 — 모두 동일한 B2C 50% 할인이 적용되므로 request shape, retry, acceptance rate가 결정을 좌우합니다.",
    ),
    sections: [
      section(
        tr("The cheapest API is the cheapest accepted asset", "Самый дешёвый API — это самый дешёвый принятый ассет", "最便宜的 API = 每个验收资产成本最低的 API", "가장 저렴한 API는 accepted asset이 가장 저렴한 API"),
        [
          paragraph(
            "Direct answer: for predictable 1K assets, Nano Banana 2 (gemini-3.1-flash-image) is usually the cheapest starting point because its image-output leg is fixed by size — 1,120 tokens for 1K, $0.0672 official, $0.0336 for a regular B2C account after the 50% discount. For workflows already built on the OpenAI Images client, GPT Image 2 (gpt-image-2) can end up cheaper despite variable token billing, because higher acceptance and zero migration work beat a lower token rate. Anything beyond that requires a benchmark on your own briefs — and this guide shows how to run one honestly.",
            "Короткий ответ: для предсказуемых 1K-ассетов самая дешёвая стартовая точка — обычно Nano Banana 2 (gemini-3.1-flash-image), потому что её image-output leg фиксирован по размеру: 1 120 tokens для 1K, $0.0672 official, $0.0336 для обычного B2C после скидки 50%. Для workflow, уже построенного на OpenAI Images client, GPT Image 2 (gpt-image-2) может оказаться дешевле даже с variable token billing: более высокий acceptance и нулевая миграция перевешивают низкий token rate. Всё остальное требует benchmark на ваших briefs — ниже показано, как провести его честно.",
            "直接回答：对于可预测的 1K 资产，Nano Banana 2（gemini-3.1-flash-image）通常是成本最低的起点，因为其图像输出项按尺寸固定——1K 为 1,120 token，官方 $0.0672，普通 B2C 五折后 $0.0336。而已基于 OpenAI Images 客户端的工作流中，GPT Image 2（gpt-image-2）即使按可变 token 计费也可能更便宜，因为更高的验收率与零迁移成本胜过更低的 token 单价。除此之外都需要用你自己的 brief 做基准测试，本文将说明如何诚实地做。",
            "직접적인 답: 예측 가능한 1K asset에는 보통 Nano Banana 2(gemini-3.1-flash-image)가 가장 저렴한 출발점입니다. image-output leg가 크기별로 고정되어 1K는 1,120 token, 공식 $0.0672, 일반 B2C는 50% 할인 후 $0.0336이기 때문입니다. 이미 OpenAI Images client 위에 구축된 workflow에서는 GPT Image 2(gpt-image-2)가 가변 token billing에도 더 저렴할 수 있습니다. 더 높은 acceptance와 migration 비용 0이 낮은 token rate를 이기기 때문입니다. 그 외에는 실제 brief로 benchmark가 필요하며, 이 가이드가 정직한 측정 방법을 보여줍니다.",
          ),
          paragraph(
            "Run the arithmetic before choosing. One thousand accepted 1K assets on Nano Banana 2 at an 80% acceptance rate means 1,250 paid generations: 1,250 × $0.0336 = $42 of image-output spend, plus text input at $0.50/M official and any grounding reported in terminal usage. The same brief on GPT Image 2 bills image output at $30/M official ($15/M after the discount), text input at $5/M and reference images at $8/M — if it passes more often, the variable bill still wins. Acceptance rate, not the price list, decides.",
            "Посчитайте до выбора. Тысяча принятых 1K-ассетов на Nano Banana 2 при acceptance rate 80% — это 1 250 платных генераций: 1 250 × $0.0336 = $42 image-output spend, плюс text input по $0.50/M official и возможный grounding из terminal usage. Тот же brief на GPT Image 2 тарифицирует image output по $30/M official ($15/M после скидки), text input по $5/M и reference images по $8/M — если он проходит чаще, variable bill всё равно выигрывает. Решает acceptance rate, а не прайс-лист.",
            "选择前先算一笔账。在 80% 验收率下用 Nano Banana 2 产出 1,000 张已验收 1K 资产，意味着 1,250 次付费生成：1,250 × $0.0336 = $42 的图像输出费用，再加上官方 $0.50/M 的文本输入与终态 usage 中报告的 grounding。同一 brief 在 GPT Image 2 上，图像输出按官方 $30/M（折后 $15/M）、文本输入 $5/M、参考图 $8/M 计费——若通过率更高，可变账单仍会胜出。决定结果的是验收率，而非价目表。",
            "선택 전에 계산부터 하세요. acceptance rate 80%에서 Nano Banana 2로 accepted 1K asset 1,000장을 만들면 1,250번의 paid generation이 필요합니다. 1,250 × $0.0336 = $42의 image-output 지출에 공식 $0.50/M의 text input과 terminal usage의 grounding이 더해집니다. 같은 brief를 GPT Image 2로 처리하면 image output 공식 $30/M(할인 후 $15/M), text input $5/M, reference image $8/M인데, 통과율이 더 높으면 가변 bill도 이깁니다. 가격표가 아니라 acceptance rate가 결정합니다.",
          ),
          table(
            { headers: ["Cost question", "Nano Banana 2 (gemini-3.1-flash-image)", "GPT Image 2 (gpt-image-2)"], rows: [["Image output", "1K $0.0672, 2K $0.1008, 4K $0.1512 official — fixed tokens by size", "$30/M actual image-output tokens; read terminal usage"], ["Text input", "$0.50/M official", "$5/M official; cached input at 25% of fresh"], ["Reference images", "input tokens at the model rate; up to 14 supported inputs", "$8/M official image input; 1–5 strict PNG on the edits route"], ["Regular B2C price", "50% off the exact official total", "50% off the exact official total"], ["What the response contains", "inlineData image part at the requested 1K/2K/4K size and aspect ratio", "one non-streaming base64 PNG; exact dimensions not promised"]] },
            { headers: ["Вопрос цены", "Nano Banana 2 (gemini-3.1-flash-image)", "GPT Image 2 (gpt-image-2)"], rows: [["Image output", "1K $0.0672, 2K $0.1008, 4K $0.1512 official — fixed tokens по размеру", "$30/M actual image-output tokens; смотрите terminal usage"], ["Text input", "$0.50/M official", "$5/M official; cached input = 25% от fresh"], ["Reference images", "input tokens по ставке модели; до 14 supported inputs", "$8/M official image input; 1–5 строгих PNG на edits route"], ["Цена для обычного B2C", "50% от exact official total", "50% от exact official total"], ["Что в response", "inlineData image part в запрошенном size 1K/2K/4K и aspect ratio", "один non-streaming base64 PNG; exact dimensions не обещаны"]] },
            { headers: ["成本问题", "Nano Banana 2（gemini-3.1-flash-image）", "GPT Image 2（gpt-image-2）"], rows: [["图像输出", "1K $0.0672、2K $0.1008、4K $0.1512 官方——按尺寸固定 token", "$30/M 实际 image-output token；以终态 usage 为准"], ["文本输入", "$0.50/M 官方", "$5/M 官方；cached input 为 fresh 的 25%"], ["参考图", "按模型费率计 input token；最多 14 个受支持输入", "$8/M 官方 image input；编辑路由 1–5 张严格 PNG"], ["普通 B2C 价格", "准确官方总额五折", "准确官方总额五折"], ["响应内容", "按请求的 1K/2K/4K 尺寸与宽高比返回 inlineData 图像 part", "单张非流式 base64 PNG；不承诺准确尺寸"]] },
            { headers: ["비용 질문", "Nano Banana 2 (gemini-3.1-flash-image)", "GPT Image 2 (gpt-image-2)"], rows: [["Image output", "1K $0.0672, 2K $0.1008, 4K $0.1512 공식 — 크기별 고정 token", "$30/M 실제 image-output token; terminal usage 확인"], ["Text input", "$0.50/M 공식", "$5/M 공식; cached input은 fresh의 25%"], ["Reference image", "모델 요금의 input token; 최대 14개 지원 input", "$8/M 공식 image input; edits route에서 1~5 strict PNG"], ["일반 B2C 가격", "exact official total의 50%", "exact official total의 50%"], ["Response 내용", "요청한 1K/2K/4K 크기와 aspect ratio의 inlineData image part", "non-streaming base64 PNG 한 장; 정확한 dimensions 미보장"]] },
          ),
        ],
      ),
      section(
        tr("Match the model to the workload's cost driver", "Подберите модель под cost driver задачи", "按工作负载的成本驱动因素匹配模型", "workload의 cost driver에 모델 맞추기"),
        [
          table(
            { headers: ["Workload", "Start with", "Reason to benchmark anyway"], rows: [["Predictable 1K social or catalog assets", "Nano Banana 2", "Fixed 1K image leg and explicit aspect ratios make spend forecastable"], ["Existing OpenAI Images client", "GPT Image 2", "No protocol migration; integration hours are real cost too"], ["Many visual references per brief", "Nano Banana 2", "Up to 14 supported image inputs on the native Gemini shape"], ["Strict PNG edit pipeline", "GPT Image 2", "Native /v1/images/edits route for one to five PNG references"], ["Mixed portfolio across asset classes", "Evaluate both", "Acceptance rate per asset class can outweigh every token price"]] },
            { headers: ["Задача", "Начните с", "Зачем всё равно нужен benchmark"], rows: [["Предсказуемые 1K social/catalog assets", "Nano Banana 2", "Fixed 1K image leg и явные aspect ratios делают spend прогнозируемым"], ["Готовый OpenAI Images client", "GPT Image 2", "Нет protocol migration; часы интеграции — тоже реальная цена"], ["Много visual references на brief", "Nano Banana 2", "До 14 supported image inputs в native Gemini shape"], ["Strict PNG edit pipeline", "GPT Image 2", "Native /v1/images/edits route для 1–5 PNG references"], ["Mixed portfolio по классам ассетов", "Обе модели", "Acceptance rate по классу ассетов может перевесить любой token price"]] },
            { headers: ["工作负载", "优先测试", "仍需基准验证的原因"], rows: [["可预测的 1K 社交/目录资产", "Nano Banana 2", "固定 1K 图像项与明确宽高比让支出可预测"], ["已有 OpenAI Images 客户端", "GPT Image 2", "无需协议迁移；集成工时也是真实成本"], ["每个 brief 需要大量参考图", "Nano Banana 2", "原生 Gemini 结构最多支持 14 个图像输入"], ["严格 PNG 编辑流程", "GPT Image 2", "原生 /v1/images/edits 路由支持 1–5 张 PNG 参考图"], ["跨资产类别的混合组合", "两者都评测", "每个资产类别的验收率可能超过任何 token 价格的影响"]] },
            { headers: ["Workload", "시작 모델", "그래도 benchmark가 필요한 이유"], rows: [["예측 가능한 1K social/catalog asset", "Nano Banana 2", "고정 1K image leg와 명시적 aspect ratio로 spend 예측 가능"], ["기존 OpenAI Images client", "GPT Image 2", "protocol migration 불필요; 통합 시간도 실제 비용"], ["brief당 많은 visual reference", "Nano Banana 2", "native Gemini 형식에서 최대 14개 지원 image input"], ["Strict PNG edit pipeline", "GPT Image 2", "1~5개 PNG reference용 native /v1/images/edits route"], ["asset class가 섞인 portfolio", "둘 다 평가", "asset class별 acceptance rate가 어떤 token price보다 클 수 있음"]] },
          ),
          note(
            "The 50% B2C discount applies to both models after exact official usage is calculated, so it never breaks a tie by itself. The gap comes from usage shape, retries and how much client code you must write and maintain for each protocol.",
            "B2C-скидка 50% действует на обе модели после расчёта exact official usage и сама по себе не выбирает победителя. Разница рождается из usage shape, retries и объёма client code под каждый protocol.",
            "B2C 五折（50%）在准确官方 usage 计算后对两款模型同样适用，因此折扣本身不能分出胜负。差距来自 usage 形态、重试次数以及你为每种协议编写和维护的客户端代码量。",
            "B2C 50% 할인은 exact official usage 계산 후 두 모델 모두에 적용되므로 그 자체로 승부를 가르지 않습니다. usage shape, retry, 각 protocol용 client code 작성·유지 비용이 차이를 만듭니다.",
          ),
        ],
      ),
      section(
        tr("Price the real request shapes, not the landing page", "Считайте по реальным request shapes, а не по лендингу", "按真实请求形态计价，而非落地页宣传", "랜딩 페이지가 아닌 실제 request shape로 계산"),
        [
          paragraph(
            "Both models are reachable with the same prepaid key, but they speak different protocols, and the protocol decides what you can control. Nano Banana 2 uses the native Gemini generateContent shape with the x-goog-api-key header: you set responseModalities to TEXT and IMAGE, pick imageSize 1K/2K/4K and an aspectRatio, and receive the picture as a base64 inlineData part. GPT Image 2 uses the OpenAI Images routes with Authorization: Bearer: the published controls are background opaque, quality low and size auto, the response is one non-streaming base64 PNG, and edits on /v1/images/edits accept one to five strict PNG references.",
            "Обе модели доступны одним prepaid-ключом, но говорят на разных protocols, и именно protocol определяет, чем вы управляете. Nano Banana 2 использует native Gemini generateContent с заголовком x-goog-api-key: вы задаёте responseModalities TEXT и IMAGE, выбираете imageSize 1K/2K/4K и aspectRatio и получаете картинку как base64 inlineData part. GPT Image 2 работает через OpenAI Images routes с Authorization: Bearer: подтверждённые controls — background opaque, quality low, size auto; response — один non-streaming base64 PNG, а edits на /v1/images/edits принимают 1–5 строгих PNG references.",
            "两款模型可用同一个预付密钥调用，但协议不同，而协议决定了你能控制什么。Nano Banana 2 使用原生 Gemini generateContent 结构与 x-goog-api-key 头：将 responseModalities 设为 TEXT 与 IMAGE，选择 imageSize 1K/2K/4K 和 aspectRatio，图像以 base64 inlineData part 返回。GPT Image 2 使用 OpenAI Images 路由与 Authorization: Bearer：已发布控制项为 background opaque、quality low、size auto，响应为单张非流式 base64 PNG，/v1/images/edits 编辑接受 1–5 张严格 PNG 参考图。",
            "두 모델 모두 같은 prepaid key로 호출되지만 protocol이 다르고, protocol이 제어 가능한 범위를 정합니다. Nano Banana 2는 x-goog-api-key 헤더의 native Gemini generateContent 형식을 사용합니다. responseModalities를 TEXT와 IMAGE로 설정하고 imageSize 1K/2K/4K와 aspectRatio를 고륩며 그림은 base64 inlineData part로 옵니다. GPT Image 2는 Authorization: Bearer의 OpenAI Images route를 쓰고 공개 control은 background opaque, quality low, size auto이며 response는 non-streaming base64 PNG 한 장, /v1/images/edits는 1~5개 strict PNG reference를 받습니다.",
          ),
          sharedCode(`# Nano Banana 2 — Gemini-native route
curl ${ROUTER}/v1beta/models/gemini-3.1-flash-image:generateContent \\
  -H "x-goog-api-key: $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{"contents":[{"parts":[{"text":"Create a square product illustration"}]}],"generationConfig":{"responseModalities":["TEXT","IMAGE"],"imageConfig":{"imageSize":"1K","aspectRatio":"1:1"}}}'

# GPT Image 2 — OpenAI Images route
curl ${OPENAI}/images/generations \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{"model":"gpt-image-2","prompt":"A clean studio product photograph","background":"opaque","quality":"low","size":"auto"}'`),
          note(
            "Do not estimate the bill from the PNG file size or pixel dimensions. For GPT Image 2 the billing authority is the terminal usage event; for Nano Banana 2 the fixed image-output leg must still be added to input, optional text or thinking output and grounding.",
            "Не оценивайте счёт по размеру PNG-файла или pixel dimensions. Для GPT Image 2 billing authority — terminal usage event; для Nano Banana 2 к fixed image-output leg нужно добавить input, возможный text/thinking output и grounding.",
            "不要按 PNG 文件大小或像素尺寸估算账单。GPT Image 2 的结算权威是终态 usage event；Nano Banana 2 的固定图像输出项之外，还要加上输入、可选文本/思考输出与 grounding。",
            "PNG 파일 크기나 pixel dimensions로 요금을 추정하지 마세요. GPT Image 2의 billing authority는 terminal usage event이고, Nano Banana 2는 고정 image-output leg에 input, 선택적 text/thinking output, grounding을 더해야 합니다.",
          ),
        ],
      ),
      section(
        tr("Run a fair per-asset cost benchmark", "Проведите честный benchmark цены ассета", "运行公平的单资产成本基准测试", "공정한 asset당 비용 benchmark 실행"),
        [
          steps(
            ["Pick 20–50 representative briefs from the real workload and write pass/fail criteria before any generation runs.", "Give both routes equivalent inputs: same resolution class, same references, same maximum number of attempts per brief.", "For every paid attempt record the request ID, terminal usage, latency and your pass/fail verdict next to the returned asset.", "Divide total settled charge — including rejected outputs — by accepted assets, and compare the distributions, not the single best image.", "Repeat the benchmark whenever prompts, target resolution, reference count or the provider catalog changes materially."],
            ["Возьмите 20–50 representative briefs из реальной задачи и запишите критерии pass/fail до любой генерации.", "Дайте обоим routes эквивалентные inputs: тот же класс resolution, те же references, тот же лимит attempts на brief.", "Для каждого paid attempt сохраняйте request ID, terminal usage, latency и ваш verdict pass/fail рядом с полученным ассетом.", "Разделите total settled charge — включая rejected outputs — на число принятых ассетов и сравнивайте распределения, а не лучший единичный image.", "Повторяйте benchmark при существенной смене prompts, целевого resolution, числа references или provider catalog."],
            ["从真实工作负载中选取 20–50 个代表性 brief，并在任何生成之前写下通过/失败标准。", "两条路由使用等价输入：相同分辨率等级、相同参考图、每个 brief 相同的最大尝试次数。", "为每次付费尝试记录 request ID、终态 usage、延迟与你的通过/失败判定，并与返回的资产一起保存。", "用总结算费用（含被拒输出）除以验收资产数，比较分布而非单张最佳图像。", "当提示词、目标分辨率、参考图数量或提供商目录显著变化时，重新运行基准测试。"],
            ["실제 workload에서 representative brief 20~50개를 고르고 generation 전에 pass/fail 기준을 적습니다.", "두 route에 동등한 input을 줍니다: 같은 resolution 등급, 같은 references, brief당 같은 최대 attempt 횟수.", "모든 paid attempt의 request ID, terminal usage, latency와 pass/fail 판정을 반환된 asset과 함께 기록합니다.", "rejected output을 포함한 total settled charge를 accepted asset 수로 나누고 최고 한 장이 아닌 분포를 비교합니다.", "prompt, 목표 resolution, reference 수, provider catalog가 크게 바뀌면 benchmark를 반복합니다."],
          ),
        ],
      ),
      section(
        tr("Savings tactics that quietly raise the bill", "«Экономия», которая незаметно раздувает счёт", "暗中抬高账单的“省钱”做法", "조용히 bill을 키우는 절감 전술"),
        [
          list(
            ["Generating at 4K and downscaling every accepted asset to 1K: you paid the 4K image leg ($0.1512 official) for a 1K deliverable ($0.0672 official).", "Counting only successful outputs while hiding paid rejects, moderation failures and timed-out retries from the cost per accepted asset.", "Burning image-model tokens on text-only planning or prompt rewriting that a cheaper text model handles before the image call.", "Retrying automatically after a delivered response, or retrying without a fixed total attempt budget per brief.", "Publishing one permanent 'cheapest' verdict while live catalog availability and per-workload quality keep moving."],
            ["Генерировать 4K и уменьшать каждый принятый ассет до 1K: вы заплатили image leg 4K ($0.1512 official) за результат уровня 1K ($0.0672 official).", "Считать только successful outputs, пряча paid rejects, moderation failures и timed-out retries из цены принятого ассета.", "Тратить токены image-модели на text-only planning и переписывание prompt, которые дешевле выполнить text-моделью до image-вызова.", "Автоматически retry после delivered response или retry без фиксированного total attempt budget на brief.", "Публиковать вечный verdict «самый дешёвый», хотя live catalog availability и quality по workload постоянно меняются."],
            ["生成 4K 再把每张验收资产缩到 1K：你为 1K 交付物付了 4K 图像项（官方 $0.1512 对 $0.0672）。", "只统计成功输出，把付费拒绝、moderation 失败与超时重试排除在每个验收资产成本之外。", "把图像模型 token 花在纯文本规划或提示词改写上，而这些在图像调用前交给更便宜的文本模型即可。", "响应已交付后仍自动重试，或在没有固定总尝试预算的情况下重试。", "实时目录可用性与各工作负载质量一直在变化，却发布永久“最便宜”结论。"],
            ["4K로 생성하고 모든 accepted asset을 1K로 축소: 1K 결과물($0.0672 공식)에 4K image leg($0.1512 공식)를 지불한 셈입니다.", "paid reject, moderation 실패, timed-out retry를 숨기고 successful output만 accepted asset당 비용에 반영합니다.", "image 호출 전에 더 저렴한 text model이 처리할 text-only planning이나 prompt 재작성에 image-model token을 씁니다.", "delivered response 이후에 자동 retry하거나 brief당 고정 total attempt budget 없이 retry합니다.", "live catalog availability와 workload별 quality가 계속 변하는데 영구적인 'cheapest' verdict를 게시합니다."],
          ),
        ],
      ),
      section(
        tr("Validate the winner with the $5 bonus before you top up", "Проверьте победителя на $5 бонуса до пополнения", "充值前用 $5 赠金验证胜出模型", "충전 전 $5 별도 크레딧으로 승자 검증"),
        [
          paragraph(
            "You do not need to spend money to find your cheapest route. New accounts created with Google or GitHub receive a one-time $5 platform bonus credit — at $0.0336 per 1K Nano Banana 2 image-output leg, that covers the entire benchmark above with room to spare. Accounts registered with email and password are fully usable but not eligible for the grant. The bonus is always spent before paid balance, so the evaluation never touches your top-up.",
            "Не нужно тратить деньги, чтобы найти самый дешёвый route. Новые аккаунты, созданные через Google или GitHub, получают one-time $5 platform bonus credit — при $0.0336 за 1K image-output leg Nano Banana 2 этого хватит на весь benchmark выше с запасом. Аккаунты с email и паролем полностью работают, но бонуса не дают. Bonus всегда расходуется раньше paid balance, поэтому оценка не трогает ваше пополнение.",
            "找到最便宜路由不需要先花钱。通过 Google 或 GitHub 创建的新账户可获得一次性 $5 平台赠金——按 Nano Banana 2 每张 1K 图像输出 $0.0336 计算，足以完成上述整套基准测试还有富余。用邮箱和密码注册的账户功能完整，但不享受该赠金。赠金总是先于付费余额消耗，因此评测不会动用你的充值。",
            "가장 저렴한 route를 찾는 데 돈을 쓸 필요가 없습니다. Google 또는 GitHub로 만든 새 계정은 one-time $5 platform bonus credit을 받으며, Nano Banana 2 1K image-output leg당 $0.0336 기준으로 위 benchmark 전체를 넉넉히 수행할 수 있습니다. 이메일·비밀번호 가입 계정은 정상 사용 가능하지만 별도 크레딧 대상이 아닙니다. bonus는 항상 paid balance보다 먼저 소진되므로 평가가 충전금을 건드리지 않습니다.",
          ),
          list(
            ["Top up in whole-dollar amounts by bank card or cryptocurrency; the prepaid balance never expires.", "Give the benchmark its own API key with a lifetime spending limit so a retry loop cannot drain the account.", "Keep the winning configuration pinned: model, size, references and maximum attempts — and re-test after catalog changes."],
            ["Пополняйте баланс на whole-dollar суммы банковской картой или криптовалютой; prepaid balance never expires.", "Выдайте benchmark отдельный API key с lifetime spending limit, чтобы retry loop не опустошил аккаунт.", "Зафиксируйте выигравшую конфигурацию — model, size, references и maximum attempts — и перепроверяйте после смены catalog."],
            ["用银行卡或加密货币按整数美元充值；预付余额永不过期。", "为基准测试使用独立的 API key 并设置 lifetime spending limit，防止重试循环耗尽账户。", "固定胜出配置——模型、尺寸、参考图与最大尝试次数——并在目录变化后重新测试。"],
            ["은행 카드 또는 암호화폐로 정수 달러 금액을 충전하고 prepaid balance는 만료되지 않습니다.", "benchmark 전용 API key에 lifetime spending limit을 둬 retry loop가 계정을 소진하지 못하게 합니다.", "승리한 구성(model, size, references, maximum attempts)을 고정하고 catalog 변경 후 다시 테스트합니다."],
          ),
        ],
      ),
    ],
    faq: [
      faq(
        tr("Which image API should I test first for the lowest cost?", "Какой image API тестировать первым ради минимальной цены?", "追求最低成本应先测试哪个图像 API？", "최저 비용을 노린다면 어느 image API를 먼저 테스트해야 하나요?"),
        tr("Start with Nano Banana 2 when your deliverables are explicit 1K/2K/4K assets — the fixed image leg makes spend predictable at $0.0336 per 1K for regular B2C. Start with GPT Image 2 when your workflow already speaks OpenAI Images. In both cases, compare settled cost per accepted asset, not the price list.", "Начните с Nano Banana 2, если deliverables — явные ассеты 1K/2K/4K: fixed image leg делает spend предсказуемым — $0.0336 за 1K для обычного B2C. Начните с GPT Image 2, если workflow уже говорит на OpenAI Images. В обоих случаях сравнивайте settled cost per accepted asset, а не прайс-лист.", "如果交付物是明确的 1K/2K/4K 资产，先测 Nano Banana 2——固定图像项让支出可预测，普通 B2C 每张 1K 为 $0.0336。如果工作流已使用 OpenAI Images，先测 GPT Image 2。两种情况下都比较每个验收资产的结算成本，而非价目表。", "deliverable이 명시적 1K/2K/4K asset이면 Nano Banana 2부터 테스트하세요. fixed image leg 덕분에 일반 B2C 기준 1K당 $0.0336으로 spend를 예측할 수 있습니다. workflow가 이미 OpenAI Images를 쓰면 GPT Image 2부터 시작하세요. 어느 경우든 가격표가 아닌 accepted asset당 settled cost를 비교합니다."),
      ),
      faq(
        tr("Is Nano Banana 2 always the cheaper option?", "Nano Banana 2 — всегда более дешёвый вариант?", "Nano Banana 2 总是更便宜的选项吗？", "Nano Banana 2가 항상 더 저렴한 옵션인가요?"),
        tr("No. Its image-output leg is predictable by size, but references, text or thinking output, grounding and retries all add settled cost. A GPT Image 2 workflow can be cheaper overall when it passes review more often or saves integration work on an existing OpenAI Images client.", "Нет. Его image-output leg предсказуем по size, но references, text/thinking output, grounding и retries добавляют settled cost. Workflow на GPT Image 2 может быть дешевле в целом, если чаще проходит review или экономит integration work на существующем OpenAI Images client.", "不是。它的图像输出项按尺寸可预测，但参考图、文本或思考输出、grounding 与重试都会增加结算成本。若 GPT Image 2 工作流审核通过率更高，或在现有 OpenAI Images 客户端上节省集成工作，总成本可能更低。", "아닙니다. image-output leg는 크기별로 예측 가능하지만 reference, text/thinking output, grounding, retry가 settled cost를 더합니다. GPT Image 2 workflow가 review를 더 자주 통과하거나 기존 OpenAI Images client에서 integration work를 줄이면 전체적으로 더 저렴할 수 있습니다."),
      ),
      faq(
        tr("What do 1,000 accepted 1K images cost on Nano Banana 2?", "Сколько стоят 1 000 принятых 1K-картинок на Nano Banana 2?", "在 Nano Banana 2 上 1,000 张已验收 1K 图像要多少钱？", "Nano Banana 2에서 accepted 1K 이미지 1,000장 비용은?"),
        tr("At 100% acceptance the image-output leg alone is 1,000 × $0.0336 = $33.60 for a regular B2C account. At an 80% acceptance rate you pay for 1,250 generations — $42 — before adding text input at $0.50/M official and any grounding from terminal usage. That is why acceptance rate belongs in every 'cheapest' calculation.", "При 100% acceptance один image-output leg стоит 1 000 × $0.0336 = $33.60 для обычного B2C. При acceptance rate 80% вы платите за 1 250 генераций — $42 — и это до text input по $0.50/M official и возможного grounding из terminal usage. Поэтому acceptance rate обязан быть в каждом расчёте «самого дешёвого».", "100% 验收率时，仅图像输出项就是 1,000 × $0.0336 = $33.60（普通 B2C）。80% 验收率时需为 1,250 次生成付费——$42——且还未计入官方 $0.50/M 的文本输入与终态 usage 中的 grounding。这就是为什么每个“最便宜”计算都必须包含验收率。", "acceptance 100%면 image-output leg만 일반 B2C 기준 1,000 × $0.0336 = $33.60입니다. acceptance rate 80%면 1,250번 generation 비용 $42를 내고, 여기에 공식 $0.50/M의 text input과 terminal usage의 grounding이 추가됩니다. 그래서 모든 'cheapest' 계산에 acceptance rate가 포함되어야 합니다."),
      ),
      faq(
        tr("Does a rejected image still cost money?", "Отклонённая картинка тоже стоит денег?", "被拒的图像也会收费吗？", "거부된 이미지도 비용이 드나요?"),
        tr("Yes, if the provider delivered output and terminal usage settled. Your quality rejection does not reverse the provider's work, so paid rejects belong in the cost per accepted asset — ignoring them is the most common way 'cheap' workflows turn expensive.", "Да, если provider доставил output и terminal usage settled. Ваш quality reject не отменяет работу provider, поэтому paid rejects входят в cost per accepted asset — игнорировать их = самый частый способ превратить «дешёвый» workflow в дорогой.", "会。只要提供商已交付输出并结算终态 usage 就收费。你的质量拒绝不会撤销提供商的工作，因此付费拒绝必须计入每个验收资产成本——忽略它们是“便宜”工作流变贵的最常见原因。", "네, provider가 output을 전달하고 terminal usage가 정산됐다면 비용이 듭니다. quality reject가 provider 작업을 되돌리지 않으므로 paid reject는 accepted asset당 비용에 포함해야 합니다. 이를 무시하는 것이 '저렴한' workflow가 비싸지는 가장 흔한 경로입니다."),
      ),
      faq(
        tr("Do both image models work with the same apiToken.sale key?", "Обе image-модели работают с одним ключом apiToken.sale?", "两款图像模型能用同一个 apiToken.sale 密钥吗？", "두 image model 모두 같은 apiToken.sale key로 쓸 수 있나요?"),
        tr("Yes. One prepaid key and balance call gemini-3.1-flash-image on the Gemini-native route with x-goog-api-key and gpt-image-2 on the OpenAI Images routes with Authorization: Bearer — both at router.apitoken.sale. Only the protocol shape changes; the 50% regular B2C discount applies to both.", "Да. Один prepaid-ключ и баланс вызывают gemini-3.1-flash-image на Gemini-native route с x-goog-api-key и gpt-image-2 на OpenAI Images routes с Authorization: Bearer — оба на router.apitoken.sale. Меняется только protocol shape; скидка 50% для обычного B2C действует на обе модели.", "可以。同一个预付密钥与余额即可调用 Gemini 原生路由上的 gemini-3.1-flash-image（x-goog-api-key）和 OpenAI Images 路由上的 gpt-image-2（Authorization: Bearer），两者都在 router.apitoken.sale。只有协议形态不同；普通 B2C 五折对两者都适用。", "네. 하나의 prepaid key와 balance로 Gemini-native route의 gemini-3.1-flash-image(x-goog-api-key)와 OpenAI Images route의 gpt-image-2(Authorization: Bearer)를 모두 router.apitoken.sale에서 호출합니다. protocol shape만 다를 뿐 일반 B2C 50% 할인은 둘 다에 적용됩니다."),
      ),
      faq(
        tr("Can I publish one permanent 'cheapest image API' verdict?", "Можно ли раз и навсегда назвать один API «самым дешёвым»?", "能否发布一个永久有效的“最便宜图像 API”结论？", "'가장 저렴한 image API' 판정을 한 번만 내리면 끝인가요?"),
        tr("No. The verdict is a property of your workload at a point in time. Re-run the benchmark after material changes to prompts, output size, reference count, catalog availability or provider behavior — and date every published claim.", "Нет. Verdict — свойство вашего workload в конкретный момент. Повторяйте benchmark после существенной смены prompts, output size, числа references, catalog availability или provider behavior — и датируйте каждый публичный claim.", "不能。结论只是某一时刻、特定工作负载的属性。提示词、输出尺寸、参考图数量、目录可用性或提供商行为显著变化后要重新测试——并为每条公开结论标注日期。", "아닙니다. verdict는 특정 시점의 workload 속성입니다. prompt, output size, reference 수, catalog availability, provider behavior가 크게 바뀌면 benchmark를 다시 실행하고 모든 공개 claim에 날짜를 붙이세요."),
      ),
    ],
  };
