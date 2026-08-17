import type { ImageSeoSpec } from "./shared";
import { faq, list, note, section, steps, table, tr } from "./shared";

export const spec: ImageSeoSpec = {
    slug: "cheapest-image-generation-api",
    cluster: "free",
    related: ["image-generation-api-pricing", "nano-banana-2-vs-gpt-image-2", "nano-banana-2-api-cost", "gpt-image-2-api-cost"],
    title: tr(
      "Cheapest Image Generation API: Compare Real Workflow Cost",
      "Самый дешёвый API генерации изображений: сравнение реальной цены",
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
      "Find the cheapest AI image API for your workload by comparing Nano Banana 2 and GPT Image 2 on output size, references, retries, acceptance rate and apiToken.sale's B2C discount.",
      "Найдите самый дешёвый AI image API для своей задачи: сравните Nano Banana 2 и GPT Image 2 по size, references, retries, acceptance rate и B2C-скидке apiToken.sale.",
      "按输出尺寸、参考图、重试、验收率与 apiToken.sale B2C 折扣比较 Nano Banana 2 和 GPT Image 2，找到适合工作负载的最低成本方案。",
      "output 크기, reference, retry, acceptance rate, apiToken.sale B2C 할인으로 Nano Banana 2와 GPT Image 2를 비교해 workload에 가장 저렴한 API를 찾으세요.",
    ),
    keywords: tr(
      ["cheapest image generation api", "cheap ai image api", "lowest cost image generator api", "nano banana vs gpt image price", "affordable image api", "image api discount"],
      ["дешевый api генерации изображений", "самый дешевый ai image api", "недорогой image generator api", "nano banana vs gpt image цена", "доступный image api", "скидка image api"],
      ["最便宜图像生成 api", "低成本 ai 图像 api", "最低成本图像生成器 api", "nano banana vs gpt image 价格", "实惠图像 api", "图像 api 折扣"],
      ["가장 저렴한 이미지 생성 api", "저렴한 ai image api", "최저 비용 image generator api", "nano banana vs gpt image 가격", "합리적인 image api", "image api 할인"],
    ),
    dek: tr(
      "The cheapest model is the one that produces an accepted asset with the fewest paid attempts. Compare complete settled spend—not only a provider's lowest output headline—and keep protocol integration cost in the decision.",
      "Самая дешёвая модель — та, что создаёт принятый ассет за минимальное число платных attempts. Сравнивайте complete settled spend, а не минимальный headline, и учитывайте цену protocol integration.",
      "最便宜的模型，是用最少付费尝试产出可验收资产的模型。应比较完整结算支出，而非最低输出报价，并把协议集成成本纳入决策。",
      "가장 저렴한 모델은 가장 적은 유료 attempt로 accepted asset을 만드는 모델입니다. 최저 output headline이 아닌 complete settled spend와 protocol 통합 비용을 비교하세요.",
    ),
    sections: [
      section(
        tr("Match the model to the cost driver", "Сопоставьте модель с cost driver", "按成本驱动因素匹配模型", "cost driver에 모델 맞추기"),
        [
          table(
            { headers: ["Workload", "Start with", "Reason to benchmark"], rows: [["Predictable 1K social assets", "Nano Banana 2", "Fixed 1K image leg and explicit aspect ratios"], ["Existing OpenAI Images client", "GPT Image 2", "Lower integration and migration work"], ["Many visual references", "Nano Banana 2", "Up to 14 supported inputs"], ["Strict PNG edit pipeline", "GPT Image 2", "Native multipart edit route"], ["Mixed portfolio", "Evaluate both", "Acceptance rate can outweigh token price"]] },
            { headers: ["Задача", "Начните с", "Что проверить"], rows: [["Предсказуемые 1K social assets", "Nano Banana 2", "Fixed 1K image leg и aspect ratios"], ["Готовый OpenAI Images client", "GPT Image 2", "Меньше integration/migration work"], ["Много visual references", "Nano Banana 2", "До 14 inputs"], ["Strict PNG edit pipeline", "GPT Image 2", "Native multipart edit route"], ["Mixed portfolio", "Обе модели", "Acceptance rate может перевесить token price"]] },
            { headers: ["工作负载", "优先测试", "需要基准验证的原因"], rows: [["可预测 1K 社交资产", "Nano Banana 2", "固定 1K 图像项与明确宽高比"], ["已有 OpenAI Images 客户端", "GPT Image 2", "集成与迁移工作更少"], ["大量视觉参考图", "Nano Banana 2", "最多 14 个输入"], ["严格 PNG 编辑流程", "GPT Image 2", "原生 multipart 编辑路由"], ["混合资产组合", "两者都评测", "验收率可能超过 token 价格影响"]] },
            { headers: ["Workload", "시작 모델", "benchmark 이유"], rows: [["예측 가능한 1K social asset", "Nano Banana 2", "고정 1K image leg와 명시적 aspect ratio"], ["기존 OpenAI Images client", "GPT Image 2", "통합·migration 작업 감소"], ["많은 visual reference", "Nano Banana 2", "최대 14개 input"], ["Strict PNG edit pipeline", "GPT Image 2", "native multipart edit route"], ["Mixed portfolio", "둘 다 평가", "acceptance rate가 token price보다 클 수 있음"]] },
          ),
          note(
            "A 50% B2C discount applies to both models, so it does not by itself choose the winner. The difference comes from usage shape, retries and how much client code you must maintain.",
            "B2C-скидка 50% действует на обе модели и сама не выбирает победителя. Разница — в usage shape, retries и объёме client code.",
            "两款模型都享受 B2C 五折，因此折扣本身不能决定胜者；差异来自 usage 形态、重试次数与客户端维护成本。",
            "B2C 50% 할인은 두 모델 모두에 적용되어 승자를 정하지 않습니다. usage shape, retry, 유지할 client code가 차이를 만듭니다.",
          ),
        ],
      ),
      section(
        tr("Run a fair cost test", "Проведите честный cost test", "运行公平成本测试", "공정한 비용 테스트"),
        [
          steps(
            ["Choose 20–50 representative briefs and define pass/fail before generation.", "Use equivalent resolution, references and maximum attempts on both routes.", "Record request ID, terminal usage, latency and pass/fail for every paid attempt.", "Divide total settled charge by passed assets and compare confidence intervals, not the best single image.", "Repeat when prompts, resolution or the provider catalog changes materially."],
            ["Выберите 20–50 representative briefs и задайте pass/fail до генерации.", "Используйте equivalent resolution, references и maximum attempts на обеих routes.", "Записывайте request ID, terminal usage, latency и pass/fail каждого paid attempt.", "Разделите total settled charge на passed assets и сравните диапазоны, а не лучший единичный image.", "Повторите тест при существенной смене prompts, resolution или provider catalog."],
            ["选择 20–50 个代表性 brief，并在生成前定义通过/失败标准。", "两条路由使用等价分辨率、参考图与最大尝试次数。", "为每次付费尝试记录 request ID、终态 usage、延迟与通过/失败。", "用总已结算费用除以通过资产数，比较区间而非最佳单张。", "当提示词、分辨率或提供商目录显著变化时重新测试。"],
            ["20~50개 representative brief를 고르고 생성 전 pass/fail을 정의합니다.", "두 route에 equivalent resolution, references, maximum attempts를 사용합니다.", "모든 paid attempt의 request ID, terminal usage, latency, pass/fail을 기록합니다.", "total settled charge를 passed asset 수로 나눠 최고 한 장이 아닌 범위를 비교합니다.", "prompt, resolution, provider catalog가 크게 바뀌면 반복합니다."],
          ),
        ],
      ),
      section(
        tr("False savings to reject", "Ложная экономия", "应拒绝的虚假节省", "거부해야 할 가짜 절감"),
        [
          list(
            ["Calling a 4K model output and downscaling every accepted asset to 1K.", "Counting only successful outputs while hiding paid rejects and moderation failures.", "Using an image model for text-only planning that a cheaper text model can perform.", "Retrying automatically after a delivered response or without a total attempt budget.", "Publishing one permanent 'cheapest' claim while live catalog availability and workload quality can change."],
            ["Генерировать 4K и уменьшать каждый принятый ассет до 1K.", "Считать только successful outputs, скрывая paid rejects и moderation failures.", "Использовать image-модель для text-only planning, который выполнит дешёвая text model.", "Автоматически retry после delivered response или без total attempt budget.", "Публиковать вечный claim «самый дешёвый», хотя catalog availability и workload quality меняются."],
            ["生成 4K 后把所有验收资产缩小到 1K。", "只统计成功输出，隐藏付费拒绝与 moderation 失败。", "让图像模型执行可由更便宜文本模型完成的纯文本规划。", "响应已经交付后仍自动重试，或没有总尝试预算。", "实时目录与工作负载质量会变化，却发布永久“最便宜”结论。"],
            ["4K를 생성하고 모든 accepted asset을 1K로 축소합니다.", "paid reject와 moderation 실패를 숨기고 successful output만 셉니다.", "저렴한 text model이 할 text-only planning에 image model을 씁니다.", "delivered response 이후 또는 total attempt budget 없이 자동 retry합니다.", "live catalog availability와 workload quality가 변하는데 영구 'cheapest' claim을 게시합니다."],
          ),
        ],
      ),
    ],
    faq: [
      faq(tr("Which image API should I test first for low cost?", "Какой image API первым тестировать ради низкой цены?", "低成本场景应先测试哪个图像 API？", "낮은 비용을 위해 어느 image API를 먼저 테스트해야 하나요?"), tr("Start with Nano Banana 2 for explicit 1K assets and GPT Image 2 when your workflow already speaks OpenAI Images. Then compare settled cost per accepted asset.", "Начните с Nano Banana 2 для explicit 1K assets и с GPT Image 2, если workflow уже использует OpenAI Images. Затем сравните settled cost per accepted asset.", "明确 1K 资产先测 Nano Banana 2；已有 OpenAI Images 工作流先测 GPT Image 2。最终比较每个验收资产的结算成本。", "명시적 1K asset은 Nano Banana 2, workflow가 이미 OpenAI Images를 쓰면 GPT Image 2로 시작하고 accepted asset당 settled cost를 비교하세요.")),
      faq(tr("Is Nano Banana 2 always cheaper?", "Nano Banana 2 всегда дешевле?", "Nano Banana 2 总是更便宜吗？", "Nano Banana 2가 항상 더 저렴한가요?"), tr("No. Its output leg is predictable by size, but references, text output, grounding and retries change total cost. A GPT Image 2 workflow can be cheaper when it passes more often or needs less integration.", "Нет. Его output leg предсказуем по size, но references, text output, grounding и retries меняют total cost. GPT Image 2 может быть дешевле при лучшем pass rate или меньшей integration work.", "不是。其输出项按尺寸可预测，但参考图、文本输出、grounding 与重试会改变总成本。若 GPT Image 2 通过率更高或集成更少，可能更便宜。", "아닙니다. output leg는 크기별로 예측 가능하지만 reference, text output, grounding, retry가 총비용을 바꿉니다. GPT Image 2가 더 잘 통과하거나 통합이 적으면 더 저렴할 수 있습니다.")),
      faq(tr("Does a failed image still cost money?", "Неудачная картинка тоже стоит денег?", "失败图像也会收费吗？", "실패 이미지도 비용이 드나요?"), tr("If the provider delivered output and terminal usage settled, yes. Your quality rejection does not reverse provider work; include it in cost per accepted asset.", "Если provider доставил output и terminal usage settled — да. Ваш quality reject не отменяет работу provider; включайте её в cost per accepted asset.", "如果提供商已交付输出并结算终态 usage，则会收费。质量拒绝不会撤销提供商工作，应计入每个验收资产成本。", "provider가 output과 terminal usage를 전달해 정산했다면 비용이 듭니다. quality reject가 provider 작업을 되돌리지 않으므로 accepted asset당 비용에 포함하세요.")),
      faq(tr("Can I rely on the cheapest claim forever?", "Можно ли навсегда полагаться на claim «самый дешёвый»?", "能否永久依赖“最便宜”结论？", "'가장 저렴' 결론을 영구히 믿어도 되나요?"), tr("No. Re-run the benchmark after material changes to prompts, output size, reference count, catalog availability or provider behavior.", "Нет. Повторяйте benchmark после существенной смены prompts, output size, reference count, catalog availability или provider behavior.", "不能。提示词、输出尺寸、参考图数量、目录可用性或提供商行为显著变化后，应重新基准测试。", "아닙니다. prompt, output size, reference count, catalog availability, provider behavior가 크게 바뀌면 benchmark를 다시 실행하세요.")),
    ],
  };
