import type { ImageSeoSpec } from "./shared";
import { faq, list, note, paragraph, section, sharedCode, steps, table, tr } from "./shared";

export const spec: ImageSeoSpec = {
    slug: "image-generation-api-for-ecommerce",
    cluster: "integrate",
    related: ["image-editing-api-guide", "batch-image-generation-api", "nano-banana-2-vs-gpt-image-2", "image-generation-api-pricing"],
    title: tr(
      "Image Generation API for Ecommerce Product Images",
      "API генерации изображений для карточек товаров",
      "电商产品图像生成 API",
      "이커머스 상품 이미지 생성 API",
    ),
    h1: tr(
      "Generate ecommerce product images through an API",
      "Генерация изображений для ecommerce через API",
      "通过 API 生成电商产品图像",
      "API로 이커머스 상품 이미지 생성",
    ),
    description: tr(
      "Use Nano Banana 2 or GPT Image 2 for ecommerce product images: reference-safe editing, backgrounds, aspect ratios, batch controls, validation, web delivery and cost tracking.",
      "Используйте Nano Banana 2 или GPT Image 2 для product images: reference-safe edits, backgrounds, aspect ratios, batch controls, validation, web delivery и контроль цены.",
      "使用 Nano Banana 2 或 GPT Image 2 制作电商产品图：参考图安全编辑、背景、宽高比、批量控制、验证、网页交付与成本跟踪。",
      "Nano Banana 2 또는 GPT Image 2로 reference-safe edit, background, aspect ratio, batch control, validation, web delivery, 비용 추적을 갖춘 ecommerce 상품 이미지를 만드세요.",
    ),
    keywords: tr(
      ["ecommerce image generation api", "product image api", "ai product photography api", "product background generator api", "catalog image automation", "ai ecommerce images"],
      ["api генерации изображений для ecommerce", "api картинок товара", "ai фото товара api", "api генератора фона товара", "автоматизация изображений каталога", "ai картинки для интернет магазина"],
      ["电商图像生成 api", "产品图像 api", "ai 产品摄影 api", "产品背景生成 api", "目录图像自动化", "ai 电商图像"],
      ["ecommerce 이미지 생성 api", "상품 이미지 api", "ai 상품 사진 api", "상품 배경 생성 api", "카탈로그 이미지 자동화", "ai ecommerce 이미지"],
    ),
    dek: tr(
      "Product imagery is an identity-preservation problem before it is a prompt-writing problem. Lock the product traits, permit only named scene changes, validate every output, and optimize cost only after the acceptance gate is reliable.",
      "Product imagery — сначала задача сохранения identity, затем prompt writing. Зафиксируйте свойства товара, разрешите только названные изменения сцены, валидируйте каждый output и оптимизируйте цену после надёжного acceptance gate.",
      "产品图像首先是身份保持问题，其次才是提示词问题。锁定产品特征，只允许明确场景变化，验证每个输出，并在验收门可靠后优化成本。",
      "product imagery는 prompt 작성 전에 identity 보존 문제입니다. 상품 특성을 고정하고 명시된 장면 변경만 허용하며 모든 output을 검증한 뒤 acceptance gate가 안정적일 때 비용을 최적화하세요.",
    ),
    sections: [
      section(
        tr("Route each product-image job", "Маршрутизируйте product-image jobs", "路由每个产品图任务", "product-image job routing"),
        [
          table(
            { headers: ["Job", "Recommended starting route", "Mandatory check"], rows: [["New lifestyle scene", "Nano Banana 2 1K with explicit ratio", "Product identity and packaging text"], ["Strict PNG background edit", "GPT Image 2 edits", "No product geometry change"], ["Many reference angles", "Nano Banana 2", "Reference consistency across views"], ["Existing OpenAI media pipeline", "GPT Image 2", "Opaque background and auto-size acceptance"], ["Large catalog", "Batch queue over either", "Cost per accepted SKU asset"]] },
            { headers: ["Job", "Стартовый route", "Обязательная проверка"], rows: [["Новая lifestyle scene", "Nano Banana 2 1K с explicit ratio", "Product identity и packaging text"], ["Strict PNG background edit", "GPT Image 2 edits", "Нет изменения product geometry"], ["Много reference angles", "Nano Banana 2", "Reference consistency между видами"], ["Готовый OpenAI media pipeline", "GPT Image 2", "Opaque background и auto-size acceptance"], ["Большой каталог", "Batch queue на любой модели", "Cost per accepted SKU asset"]] },
            { headers: ["任务", "建议起始路由", "强制检查"], rows: [["新生活方式场景", "Nano Banana 2 1K + 明确宽高比", "产品一致性与包装文字"], ["严格 PNG 背景编辑", "GPT Image 2 edits", "产品几何形状不变"], ["多角度参考图", "Nano Banana 2", "各视角参考一致性"], ["已有 OpenAI 媒体流水线", "GPT Image 2", "接受不透明背景与 auto 尺寸"], ["大型目录", "任一模型上的 batch 队列", "每个 SKU 验收资产成本"]] },
            { headers: ["Job", "권장 시작 route", "필수 검사"], rows: [["새 lifestyle scene", "명시적 ratio의 Nano Banana 2 1K", "product identity와 packaging text"], ["Strict PNG background edit", "GPT Image 2 edits", "product geometry 불변"], ["많은 reference angle", "Nano Banana 2", "view 간 reference consistency"], ["기존 OpenAI media pipeline", "GPT Image 2", "opaque background와 auto-size 수용"], ["대형 catalog", "어느 모델이든 batch queue", "SKU accepted asset당 비용"]] },
          ),
          note(
            "GPT Image 2 does not publish transparent background or exact dimensions on this route. If those are hard storefront requirements, reject the route at design time instead of repairing every output later.",
            "GPT Image 2 на этом route не обещает transparent background или exact dimensions. Если это жёсткие storefront requirements, отклоните route на design stage, а не чините каждый output.",
            "该路由的 GPT Image 2 不承诺透明背景或准确尺寸。若它们是硬性店面要求，应在设计阶段排除该路由，而不是事后修复每个输出。",
            "GPT Image 2는 이 route에서 transparent background나 exact dimensions를 약속하지 않습니다. storefront 필수 요건이면 모든 output을 나중에 고치지 말고 설계 단계에서 route를 제외하세요.",
          ),
        ],
      ),
      section(
        tr("Production workflow", "Production workflow", "生产工作流", "production workflow"),
        [
          steps(
            ["Normalize source photography and assign an immutable SKU/asset version.", "Write allowed changes and protected traits: shape, color, logo, labels and included accessories.", "Generate one bounded candidate, then validate product identity, text, artifacts, composition and policy compliance.", "Store the original, output, prompt version, model, request ID, terminal usage and reviewer verdict together.", "Publish a web derivative only after optimization; never use the API's base64 payload directly as a storefront asset."],
            ["Нормализуйте source photography и задайте immutable SKU/asset version.", "Опишите allowed changes и protected traits: shape, color, logo, labels, accessories.", "Сгенерируйте один bounded candidate и проверьте identity, text, artifacts, composition и policy.", "Храните original, output, prompt version, model, request ID, terminal usage и reviewer verdict вместе.", "Публикуйте оптимизированную web derivative; не используйте API base64 напрямую как storefront asset."],
            ["规范源产品照片并分配不可变 SKU/asset 版本。", "明确允许变化与受保护特征：形状、颜色、logo、标签及配件。", "生成一个有界候选项，验证产品一致性、文字、瑕疵、构图与合规。", "把原图、输出、prompt 版本、模型、request ID、终态 usage 与审核结论一起保存。", "优化后再发布 web derivative；不要直接把 API base64 当店面资产。"],
            ["source photography를 정규화하고 immutable SKU/asset version을 부여합니다.", "허용 변경과 protected trait인 shape, color, logo, label, accessory를 적습니다.", "bounded candidate 하나를 생성해 product identity, text, artifact, composition, policy를 검증합니다.", "original, output, prompt version, model, request ID, terminal usage, reviewer verdict를 함께 저장합니다.", "최적화한 web derivative만 게시하고 API base64를 storefront asset으로 직접 쓰지 않습니다."],
          ),
          sharedCode(`PRODUCT: SKU-1042, matte black bottle, silver cap, logo unchanged
ALLOWED: replace the background with a warm kitchen scene
PROTECTED: silhouette, cap shape, logo spelling, label colors
OUTPUT: one centered catalog hero image; no added text or accessories`),
        ],
      ),
      section(
        tr("Cost and image SEO after generation", "Цена и image SEO после генерации", "生成后的成本与图像 SEO", "생성 후 비용과 image SEO"),
        [
          list(
            ["Track settled cost per accepted SKU image, not request count.", "Start with the smallest delivery resolution and promote only hero assets that need more detail.", "Use descriptive filenames and truthful alt text based on the actual product and scene; do not stuff model names into alt text.", "Encode responsive WebP/AVIF derivatives, preserve a lossless master and publish explicit width/height to avoid layout shift.", "Keep generated-image provenance and review evidence outside public metadata when it contains internal IDs or prompts."],
            ["Считайте settled cost per accepted SKU image, а не request count.", "Начинайте с минимальной delivery resolution и повышайте только hero assets, которым нужна детализация.", "Используйте descriptive filenames и правдивый alt по товару/сцене; не набивайте alt названиями моделей.", "Создайте responsive WebP/AVIF, сохраните lossless master и публикуйте width/height против layout shift.", "Храните provenance/review evidence вне public metadata, если там internal IDs или prompts."],
            ["跟踪每个验收 SKU 图像的结算成本，而不是请求数。", "从最小交付分辨率开始，仅升级需要细节的 hero 资产。", "使用描述性文件名与符合真实产品/场景的 alt，不要把模型名称堆入 alt。", "生成响应式 WebP/AVIF，保留无损 master，并发布明确 width/height 防止布局偏移。", "若来源与审核证据含内部 ID 或提示词，应保存在公开 metadata 之外。"],
            ["request 수가 아닌 accepted SKU image당 settled cost를 추적합니다.", "가장 작은 delivery resolution으로 시작하고 세부가 필요한 hero asset만 올립니다.", "실제 상품/장면에 맞는 descriptive filename과 정직한 alt를 쓰고 model name을 alt에 채우지 않습니다.", "responsive WebP/AVIF derivative, lossless master, 명시적 width/height로 layout shift를 막습니다.", "provenance/review evidence에 internal ID나 prompt가 있으면 public metadata 밖에 보관합니다."],
          ),
          paragraph(
            "For regular B2C, the 50% discount applies to the exact official generation or edit usage. Conversion, CDN storage, human review and rejected assets remain your application costs and belong in the ecommerce business case.",
            "Для обычного B2C скидка 50% применяется к exact official generation/edit usage. Conversion, CDN storage, human review и rejected assets остаются application costs и входят в ecommerce business case.",
            "普通 B2C 的五折适用于准确官方生成或编辑 usage；转换、CDN 存储、人工审核与被拒资产仍属于应用成本，应计入电商商业模型。",
            "일반 B2C의 50% 할인은 exact official generation/edit usage에 적용됩니다. conversion, CDN storage, human review, rejected asset은 application cost로 ecommerce business case에 포함해야 합니다.",
          ),
        ],
      ),
    ],
    faq: [
      faq(tr("Which model is better for product images?", "Какая модель лучше для product images?", "哪款模型更适合产品图？", "상품 이미지에는 어느 모델이 더 좋나요?"), tr("Use an eval. Nano Banana 2 is the stronger starting point for explicit ratios and many references; GPT Image 2 fits strict PNG edits and existing OpenAI Images clients.", "Нужен eval. Nano Banana 2 удобнее для explicit ratios и many references; GPT Image 2 — для strict PNG edits и готовых OpenAI Images clients.", "应通过评测决定。明确宽高比与多参考图优先测试 Nano Banana 2；严格 PNG 编辑与已有 OpenAI Images 客户端优先测试 GPT Image 2。", "eval이 필요합니다. 명시적 ratio와 많은 reference는 Nano Banana 2, strict PNG edit과 기존 OpenAI Images client는 GPT Image 2로 시작하세요.")),
      faq(tr("Can I generate transparent product cutouts with GPT Image 2 here?", "Можно ли здесь делать transparent cutouts через GPT Image 2?", "本站能用 GPT Image 2 生成透明产品抠图吗？", "여기 GPT Image 2로 투명 상품 cutout을 만들 수 있나요?"), tr("No. The proved route supports an opaque background only. Use a validated downstream segmentation process or choose a workflow whose live contract supports your required background.", "Нет. Подтверждённый route поддерживает только opaque background. Используйте validated downstream segmentation или другой workflow с нужным live contract.", "不能。已验证路由仅支持不透明背景。应使用经过验证的下游分割流程，或选择实时契约满足背景要求的工作流。", "아닙니다. 검증된 route는 opaque background만 지원합니다. 검증된 downstream segmentation이나 필요한 background를 지원하는 live contract workflow를 쓰세요.")),
      faq(tr("How do I prevent the model from changing the product?", "Как не дать модели изменить товар?", "如何防止模型改变产品？", "모델이 상품을 바꾸지 않게 하려면?"), tr("Declare protected traits, provide clean references, make one named change at a time and reject any output that fails product-identity validation. A prompt is not a substitute for validation.", "Опишите protected traits, дайте clean references, меняйте по одному named element и отклоняйте output, не прошедший product-identity validation. Prompt не заменяет validation.", "声明受保护特征，提供干净参考图，每次只做一个明确变更，并拒绝未通过产品一致性验证的输出。提示词不能替代验证。", "protected trait를 선언하고 clean reference를 제공하며 한 번에 named change 하나만 하고 product-identity validation 실패 output을 거부합니다. prompt가 validation을 대체하지 않습니다.")),
      faq(tr("Does the 50% discount include CDN and review cost?", "Включает ли скидка 50% CDN и review cost?", "五折是否包含 CDN 与审核成本？", "50% 할인에 CDN과 review 비용도 포함되나요?"), tr("No. It applies to official model usage for regular B2C. Storage, transformations, CDN delivery, human review and rejected assets are separate application costs.", "Нет. Она применяется к official model usage обычного B2C. Storage, transformations, CDN, human review и rejected assets — отдельные application costs.", "不包含。它适用于普通 B2C 的官方模型 usage；存储、转换、CDN、人工审核与被拒资产是独立应用成本。", "아닙니다. 일반 B2C official model usage에 적용되며 storage, transformation, CDN, human review, rejected asset은 별도 application cost입니다.")),
    ],
  };
