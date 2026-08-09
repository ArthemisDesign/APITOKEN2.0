import type {
  LearnArticle,
  LearnBlock,
  LearnCluster,
  LearnFaq,
  LearnSection,
  Locale,
  LocalizedContent,
} from "./learn";

type I18n<T> = Record<Locale, T>;

type ImageSeoSpec = {
  slug: string;
  cluster: LearnCluster;
  related: string[];
  title: I18n<string>;
  h1: I18n<string>;
  description: I18n<string>;
  keywords: I18n<string[]>;
  dek: I18n<string>;
  sections: I18n<LearnSection>[];
  faq: I18n<LearnFaq>[];
};

const ROUTER = "https://router.apitoken.sale";
const OPENAI = `${ROUTER}/v1`;

function tr<T>(en: T, ru: T, zh: T, ko: T): I18n<T> {
  return { en, ru, zh, ko };
}

function paragraph(en: string, ru: string, zh: string, ko: string): I18n<LearnBlock> {
  return tr(
    { type: "p", text: en },
    { type: "p", text: ru },
    { type: "p", text: zh },
    { type: "p", text: ko },
  );
}

function note(en: string, ru: string, zh: string, ko: string): I18n<LearnBlock> {
  return tr(
    { type: "note", text: en },
    { type: "note", text: ru },
    { type: "note", text: zh },
    { type: "note", text: ko },
  );
}

function list(en: string[], ru: string[], zh: string[], ko: string[]): I18n<LearnBlock> {
  return tr(
    { type: "list", items: en },
    { type: "list", items: ru },
    { type: "list", items: zh },
    { type: "list", items: ko },
  );
}

function steps(en: string[], ru: string[], zh: string[], ko: string[]): I18n<LearnBlock> {
  return tr(
    { type: "steps", items: en },
    { type: "steps", items: ru },
    { type: "steps", items: zh },
    { type: "steps", items: ko },
  );
}

function table(
  en: { headers: string[]; rows: string[][] },
  ru: { headers: string[]; rows: string[][] },
  zh: { headers: string[]; rows: string[][] },
  ko: { headers: string[]; rows: string[][] },
): I18n<LearnBlock> {
  return tr(
    { type: "table", ...en },
    { type: "table", ...ru },
    { type: "table", ...zh },
    { type: "table", ...ko },
  );
}

function sharedCode(code: string): I18n<LearnBlock> {
  const block: LearnBlock = { type: "code", code };
  return tr(block, block, block, block);
}

function section(
  heading: I18n<string>,
  blocks: I18n<LearnBlock>[],
): I18n<LearnSection> {
  return tr(
    { h2: heading.en, blocks: blocks.map((block) => block.en) },
    { h2: heading.ru, blocks: blocks.map((block) => block.ru) },
    { h2: heading.zh, blocks: blocks.map((block) => block.zh) },
    { h2: heading.ko, blocks: blocks.map((block) => block.ko) },
  );
}

function faq(
  question: I18n<string>,
  answer: I18n<string>,
): I18n<LearnFaq> {
  return tr(
    { q: question.en, a: answer.en },
    { q: question.ru, a: answer.ru },
    { q: question.zh, a: answer.zh },
    { q: question.ko, a: answer.ko },
  );
}

const imageSeoSpecs: ImageSeoSpec[] = [
  {
    slug: "nano-banana-2-api-cost",
    cluster: "explain",
    related: ["nano-banana-2-api-guide", "gpt-image-2-api-cost", "image-generation-api-pricing", "gemini-api-pricing"],
    title: tr(
      "Nano Banana 2 API Cost: Save 50% on Image Generation",
      "Стоимость Nano Banana 2 API: экономия 50% на генерации",
      "Nano Banana 2 API 成本：图像生成节省 50%",
      "Nano Banana 2 API 비용: 이미지 생성 50% 절감",
    ),
    h1: tr(
      "Nano Banana 2 API cost and 50% B2C savings",
      "Стоимость Nano Banana 2 API и экономия 50% для B2C",
      "Nano Banana 2 API 成本与 B2C 五折优惠",
      "Nano Banana 2 API 비용과 B2C 50% 할인",
    ),
    description: tr(
      "Calculate Nano Banana 2 image-generation cost for 1K, 2K and 4K output. See exact official image tokens, apiToken.sale's 50% B2C price and budgeting safeguards.",
      "Рассчитайте стоимость генерации Nano Banana 2 для 1K, 2K и 4K: точные image tokens, цена apiToken.sale со скидкой 50% для B2C и контроль бюджета.",
      "计算 Nano Banana 2 的 1K、2K、4K 图像生成成本：准确 image token、apiToken.sale B2C 五折价格与预算保护。",
      "Nano Banana 2의 1K, 2K, 4K 이미지 생성 비용, 정확한 image token, apiToken.sale B2C 50% 가격과 예산 보호를 확인하세요.",
    ),
    keywords: tr(
      ["nano banana 2 api cost", "nano banana 2 price", "cheap nano banana api", "gemini image generation cost", "nano banana 2 discount", "gemini 3.1 flash image pricing"],
      ["nano banana 2 цена api", "стоимость nano banana 2", "дешевый nano banana api", "цена генерации gemini", "скидка nano banana 2", "gemini 3.1 flash image цена"],
      ["nano banana 2 api 成本", "nano banana 2 价格", "便宜 nano banana api", "gemini 图像生成成本", "nano banana 2 折扣", "gemini 3.1 flash image 定价"],
      ["nano banana 2 api 비용", "nano banana 2 가격", "저렴한 nano banana api", "gemini 이미지 생성 비용", "nano banana 2 할인", "gemini 3.1 flash image 가격"],
    ),
    dek: tr(
      "Nano Banana 2 has fixed image-output token counts for the live 1K, 2K and 4K sizes. A regular B2C account pays half of the official image leg, while input, text/thinking output and grounding remain separate usage legs.",
      "У Nano Banana 2 зафиксировано число image-output tokens для доступных размеров 1K, 2K и 4K. Обычный B2C-аккаунт платит половину официальной image-составляющей, а input, text/thinking output и grounding считаются отдельно.",
      "Nano Banana 2 对当前 1K、2K、4K 尺寸采用固定 image-output token。普通 B2C 账户支付官方图像费用的一半，输入、文本/思考输出与 grounding 仍单独计费。",
      "Nano Banana 2는 제공되는 1K, 2K, 4K 크기에 고정 image-output token을 사용합니다. 일반 B2C 계정은 공식 이미지 leg의 절반을 내며 input, text/thinking output, grounding은 별도입니다.",
    ),
    sections: [
      section(
        tr("Image-output cost by size", "Стоимость image output по размеру", "按尺寸计算图像输出成本", "크기별 image output 비용"),
        [
          table(
            { headers: ["Size", "Billable image tokens", "Official image output", "Regular B2C here"], rows: [["1K", "1,120", "$0.0672", "$0.0336"], ["2K", "1,680", "$0.1008", "$0.0504"], ["4K", "2,520", "$0.1512", "$0.0756"]] },
            { headers: ["Размер", "Image tokens", "Официальный image output", "Обычный B2C здесь"], rows: [["1K", "1 120", "$0.0672", "$0.0336"], ["2K", "1 680", "$0.1008", "$0.0504"], ["4K", "2 520", "$0.1512", "$0.0756"]] },
            { headers: ["尺寸", "计费 image token", "官方图像输出", "本站普通 B2C"], rows: [["1K", "1,120", "$0.0672", "$0.0336"], ["2K", "1,680", "$0.1008", "$0.0504"], ["4K", "2,520", "$0.1512", "$0.0756"]] },
            { headers: ["크기", "과금 image token", "공식 이미지 output", "일반 B2C 가격"], rows: [["1K", "1,120", "$0.0672", "$0.0336"], ["2K", "1,680", "$0.1008", "$0.0504"], ["4K", "2,520", "$0.1512", "$0.0756"]] },
          ),
          note(
            "These are image-output charges, not a promise that the whole request costs exactly that amount. Add text/image input, any text or thinking output, and grounding reported by terminal usage.",
            "Это цена image output, а не обещание полной цены запроса. Добавьте text/image input, возможный text/thinking output и grounding из terminal usage.",
            "这些是图像输出费用，并非整个请求的固定总价；还需加上 text/image input、文本或思考输出以及终态 usage 中的 grounding。",
            "이는 image output 비용이며 전체 요청의 고정 가격이 아닙니다. text/image input, text/thinking output, terminal usage의 grounding을 더해야 합니다.",
          ),
        ],
      ),
      section(
        tr("Budget before you generate", "Как оценить бюджет до генерации", "生成前制定预算", "생성 전 예산 계산"),
        [
          steps(
            ["Call countTokens on gemini-3.1-flash-image to estimate input without buying an image.", "Choose 1K, 2K or 4K explicitly; do not reserve 4K for assets that ship at 1K.", "Add the fixed image-output leg above to bounded input and optional grounding.", "Set a lifetime spending limit on the image-workload key and reconcile terminal usage after the first call."],
            ["Вызовите countTokens для gemini-3.1-flash-image и бесплатно оцените input.", "Явно выберите 1K, 2K или 4K; не резервируйте 4K для ассета, который будет опубликован в 1K.", "Прибавьте фиксированную image-output составляющую из таблицы к ограниченному input и возможному grounding.", "Задайте ключу общий лимит расходов и после первого вызова сверьте terminal usage."],
            ["先对 gemini-3.1-flash-image 调用 countTokens，免费估算输入。", "明确选择 1K、2K 或 4K；最终只发布 1K 时不要预留 4K。", "把上表固定图像输出费用加到有界输入与可选 grounding。", "为图像工作负载密钥设置终身消费上限，并在首个请求后核对终态 usage。"],
            ["gemini-3.1-flash-image에 countTokens를 호출해 이미지 구매 없이 input을 추정합니다.", "1K, 2K, 4K를 명시하고 1K로 배포할 asset에 4K를 예약하지 않습니다.", "표의 고정 image-output leg를 제한된 input과 선택적 grounding에 더합니다.", "이미지 workload key에 평생 누적 지출 한도를 두고 첫 호출의 terminal usage를 대조합니다."],
          ),
          sharedCode(`curl ${ROUTER}/v1beta/models/gemini-3.1-flash-image:generateContent \\
  -H "x-goog-api-key: $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{"contents":[{"parts":[{"text":"Create a square product illustration"}]}],"generationConfig":{"responseModalities":["TEXT","IMAGE"],"imageConfig":{"imageSize":"1K","aspectRatio":"1:1"}}}'`),
        ],
      ),
      section(
        tr("Save without degrading the workflow", "Как экономить без поломки процесса", "不破坏工作流的节省方式", "workflow를 해치지 않는 절감"),
        [
          list(
            ["Use a text Flash model for prompt rewriting or classification; call Flash Image only when the response must contain pixels.", "Start at 1K and promote only assets that fail a delivery-resolution check.", "Reuse one bounded set of references instead of uploading the same large collection to every candidate.", "Do not assume cached input is cheaper: this image model bills cached input at the full input rate."],
            ["Переписывайте prompt и классифицируйте через text Flash; вызывайте Flash Image только когда ответ должен содержать пиксели.", "Начинайте с 1K и повышайте размер только для ассетов, не прошедших проверку разрешения.", "Используйте ограниченный набор references вместо повторной отправки большой коллекции в каждый вариант.", "Не рассчитывайте на дешёвый cache: cached input этой image-модели оплачивается по полной input-ставке."],
            ["提示词改写与分类使用文本 Flash；只有响应必须包含图像时才调用 Flash Image。", "从 1K 开始，仅对未通过交付分辨率检查的资产升级。", "复用有界参考图集合，不要为每个候选项重复上传大型集合。", "不要假设缓存输入更便宜：该图像模型的 cached input 按完整 input 费率计价。"],
            ["prompt 재작성과 분류는 text Flash를 쓰고 응답에 픽셀이 필요할 때만 Flash Image를 호출합니다.", "1K로 시작하고 배포 해상도 검사를 통과하지 못한 asset만 올립니다.", "각 candidate마다 큰 컬렉션을 다시 보내지 말고 제한된 reference 세트를 재사용합니다.", "이 image 모델은 cached input도 전체 input 가격이므로 cache 할인을 가정하지 않습니다."],
          ),
          paragraph(
            "The 50% figure is the regular B2C policy applied after official usage is calculated. B2B uses its negotiated policy, OpenKeys remain 1:1, and a discount never proves that a model is currently available to a particular key.",
            "50% — это политика обычного B2C, применяемая после расчёта official usage. У B2B действует согласованная политика, OpenKeys остаются 1:1, а скидка сама по себе не гарантирует доступность модели конкретному ключу.",
            "50% 是在官方 usage 计算完成后应用的普通 B2C 策略。B2B 使用协商策略，OpenKeys 保持 1:1；折扣本身不代表某个密钥当前一定可用该模型。",
            "50%는 official usage 계산 뒤 적용되는 일반 B2C 정책입니다. B2B는 협상 정책, OpenKeys는 1:1이며 할인만으로 특정 key의 현재 모델 가용성을 증명하지 않습니다.",
          ),
        ],
      ),
    ],
    faq: [
      faq(
        tr("How much is a 1K Nano Banana 2 image here?", "Сколько здесь стоит image output 1K Nano Banana 2?", "本站 1K Nano Banana 2 图像输出多少钱？", "여기서 Nano Banana 2 1K image output은 얼마인가요?"),
        tr("The fixed 1K image-output leg is $0.0336 for a regular B2C account after the 50% discount. Input, optional text/thinking output and grounding are additional.", "Фиксированная составляющая image output 1K стоит $0.0336 для обычного B2C после скидки 50%. Input, возможный text/thinking output и grounding оплачиваются дополнительно.", "普通 B2C 五折后的固定 1K 图像输出费用为 $0.0336；输入、可选文本/思考输出与 grounding 另计。", "일반 B2C의 50% 할인 후 고정 1K image-output leg는 $0.0336이며 input, 선택적 text/thinking output, grounding은 별도입니다."),
      ),
      faq(
        tr("Is 4K always four times the 1K price?", "Всегда ли 4K стоит в четыре раза дороже 1K?", "4K 总是 1K 价格的四倍吗？", "4K는 항상 1K의 네 배 가격인가요?"),
        tr("No. The published image-output legs are $0.0672 official for 1K and $0.1512 for 4K, before the B2C discount. Use the exact size table rather than multiplying dimensions.", "Нет. Официальные image-output составляющие — $0.0672 для 1K и $0.1512 для 4K до B2C-скидки. Используйте таблицу размеров, а не умножение пикселей.", "不是。B2C 折扣前，1K 官方图像输出为 $0.0672，4K 为 $0.1512；应使用准确尺寸表，而不是按像素倍数推算。", "아닙니다. B2C 할인 전 공식 image-output leg는 1K $0.0672, 4K $0.1512입니다. 픽셀 배수가 아닌 정확한 크기 표를 사용하세요."),
      ),
      faq(
        tr("Does the Nano Banana 2 discount apply to every account?", "Скидка Nano Banana 2 действует на любой аккаунт?", "Nano Banana 2 折扣适用于所有账户吗？", "Nano Banana 2 할인이 모든 계정에 적용되나요?"),
        tr("The flat 50% policy applies to regular B2C accounts. B2B pricing follows the account's negotiated rules, while OpenKeys bill at official 1:1 prices.", "Фиксированные 50% действуют для обычных B2C. Цена B2B определяется индивидуальными правилами аккаунта, а OpenKeys тарифицируются 1:1 по официальной цене.", "固定五折适用于普通 B2C；B2B 按账户协商规则计价，OpenKeys 则按官方价格 1:1 计费。", "고정 50%는 일반 B2C에 적용됩니다. B2B는 계정 협상 규칙, OpenKeys는 공식 가격 1:1입니다."),
      ),
      faq(
        tr("Can I use 0.5K to save more?", "Можно ли использовать 0.5K для дополнительной экономии?", "能否使用 0.5K 进一步省钱？", "0.5K로 더 절약할 수 있나요?"),
        tr("Not on the live subscription route. It currently admits 1K, 2K and 4K; 0.5K is rejected locally until that private capability is live-verified.", "Не на текущем subscription route. Сейчас доступны 1K, 2K и 4K; 0.5K отклоняется локально до live-проверки private capability.", "当前订阅路由不支持。现有可用尺寸为 1K、2K、4K；在私有能力完成实时验证前，0.5K 会被本地拒绝。", "현재 subscription route에서는 불가합니다. 1K, 2K, 4K만 허용되며 0.5K는 private capability live 검증 전까지 로컬에서 거부됩니다."),
      ),
    ],
  },
  {
    slug: "gpt-image-2-api-cost",
    cluster: "explain",
    related: ["gpt-image-2-api-guide", "nano-banana-2-api-cost", "image-generation-api-pricing", "image-editing-api-guide"],
    title: tr(
      "GPT Image 2 API Cost: Save 50% on Generation and Edits",
      "Стоимость GPT Image 2 API: экономия 50% на генерации и edits",
      "GPT Image 2 API 成本：生成与编辑节省 50%",
      "GPT Image 2 API 비용: 생성과 편집 50% 절감",
    ),
    h1: tr(
      "GPT Image 2 API cost and 50% B2C savings",
      "Стоимость GPT Image 2 API и скидка 50% для B2C",
      "GPT Image 2 API 成本与 B2C 五折优惠",
      "GPT Image 2 API 비용과 B2C 50% 할인",
    ),
    description: tr(
      "Calculate GPT Image 2 generation and edit cost from text input, image input, cached input and image-output tokens, with apiToken.sale's flat 50% B2C discount.",
      "Рассчитайте стоимость генерации и edits GPT Image 2 по text input, image input, cached input и image-output tokens со скидкой apiToken.sale 50% для B2C.",
      "根据 text input、image input、cached input 与 image-output token 计算 GPT Image 2 生成和编辑成本，并应用 apiToken.sale B2C 五折。",
      "text input, image input, cached input, image-output token으로 GPT Image 2 생성·편집 비용과 apiToken.sale B2C 50% 할인을 계산하세요.",
    ),
    keywords: tr(
      ["gpt image 2 api cost", "gpt image 2 price", "cheap gpt image api", "gpt image 2 discount", "openai image generation cost", "gpt image edit price"],
      ["gpt image 2 цена api", "стоимость gpt image 2", "дешевый gpt image api", "скидка gpt image 2", "стоимость генерации openai", "цена gpt image edit"],
      ["gpt image 2 api 成本", "gpt image 2 价格", "便宜 gpt image api", "gpt image 2 折扣", "openai 图像生成成本", "gpt 图像编辑价格"],
      ["gpt image 2 api 비용", "gpt image 2 가격", "저렴한 gpt image api", "gpt image 2 할인", "openai 이미지 생성 비용", "gpt image edit 가격"],
    ),
    dek: tr(
      "GPT Image 2 does not have one honest fixed price per picture: the settled total follows terminal text-input, image-input, cached-input and image-output usage. Regular B2C pays exactly half of the resulting official cost.",
      "У GPT Image 2 нет одной честной фиксированной цены картинки: итог определяется terminal usage для text input, image input, cached input и image output. Обычный B2C платит ровно половину рассчитанной официальной стоимости.",
      "GPT Image 2 没有诚实的单张固定价格：结算总额由终态 text input、image input、cached input 与 image output usage 决定。普通 B2C 支付计算后官方成本的一半。",
      "GPT Image 2에는 정직한 이미지당 고정 가격이 없습니다. terminal text input, image input, cached input, image output usage로 정산하며 일반 B2C는 공식 비용의 정확히 절반을 냅니다.",
    ),
    sections: [
      section(
        tr("Every billed leg", "Все составляющие цены", "全部计费项", "모든 과금 leg"),
        [
          table(
            { headers: ["Usage leg", "Official per 1M", "Regular B2C here"], rows: [["Fresh text input", "$5", "$2.50"], ["Fresh image input", "$8", "$4"], ["Cached text input", "$1.25", "$0.625"], ["Cached image input", "$2", "$1"], ["Image output", "$30", "$15"]] },
            { headers: ["Usage leg", "Официально за 1M", "Обычный B2C здесь"], rows: [["Fresh text input", "$5", "$2.50"], ["Fresh image input", "$8", "$4"], ["Cached text input", "$1.25", "$0.625"], ["Cached image input", "$2", "$1"], ["Image output", "$30", "$15"]] },
            { headers: ["Usage 项", "官方每 1M", "本站普通 B2C"], rows: [["Fresh text input", "$5", "$2.50"], ["Fresh image input", "$8", "$4"], ["Cached text input", "$1.25", "$0.625"], ["Cached image input", "$2", "$1"], ["Image output", "$30", "$15"]] },
            { headers: ["Usage leg", "공식 1M당", "일반 B2C 가격"], rows: [["Fresh text input", "$5", "$2.50"], ["Fresh image input", "$8", "$4"], ["Cached text input", "$1.25", "$0.625"], ["Cached image input", "$2", "$1"], ["Image output", "$30", "$15"]] },
          ),
          paragraph(
            "The formula is sum(tokens in each leg × its official rate) × 0.5 for regular B2C. Cached text and image input are 25% of their fresh official rates before the B2C discount is applied.",
            "Формула для обычного B2C: сумма(tokens каждого leg × official rate) × 0.5. Cached text и image input сначала считаются как 25% от fresh official rate, затем применяется B2C-скидка.",
            "普通 B2C 公式为：各项 token × 官方费率之和，再乘 0.5。cached text/image input 先按 fresh 官方费率的 25% 计算，再应用 B2C 折扣。",
            "일반 B2C 공식은 각 leg token × 공식 요금의 합 × 0.5입니다. cached text/image input은 fresh 공식 요금의 25%를 계산한 뒤 B2C 할인을 적용합니다.",
          ),
        ],
      ),
      section(
        tr("Measure a real request", "Измерьте реальный запрос", "测量真实请求", "실제 요청 측정"),
        [
          steps(
            ["Use gpt-image-2 on POST /v1/images/generations for a new asset or /v1/images/edits for one to five strict PNG references.", "Keep the shipped profile bounded: one PNG output, non-streaming, with only the documented opaque/low/auto controls.", "Read terminal usage rather than estimating output tokens from PNG bytes or dimensions.", "Match the request with the dashboard charge and keep its ID beside the generated asset."],
            ["Для нового ассета вызовите gpt-image-2 через POST /v1/images/generations, для edits с 1–5 строгими PNG — /v1/images/edits.", "Сохраняйте подтверждённый профиль: один PNG без стриминга и только документированные controls opaque/low/auto.", "Читайте terminal usage, не оценивайте output tokens по размеру PNG или dimensions.", "Сверьте request с charge в дашборде и храните его ID рядом с ассетом."],
            ["新资产使用 gpt-image-2 调用 POST /v1/images/generations；1–5 张严格 PNG 参考图编辑使用 /v1/images/edits。", "保持已发布的有界配置：单张非流式 PNG，仅使用已记录的 opaque/low/auto 控制。", "读取终态 usage，不要按 PNG 字节或尺寸估算输出 token。", "将请求与仪表板扣费核对，并把 request ID 与生成资产一起保存。"],
            ["새 asset은 gpt-image-2의 POST /v1/images/generations, 1~5 strict PNG edit은 /v1/images/edits를 사용합니다.", "한 장의 non-streaming PNG와 문서화된 opaque/low/auto control만 쓰는 bounded profile을 유지합니다.", "PNG bytes나 dimensions로 output token을 추정하지 말고 terminal usage를 읽습니다.", "request를 dashboard charge와 대조하고 ID를 생성 asset과 함께 보관합니다."],
          ),
          sharedCode(`curl ${OPENAI}/images/generations \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{"model":"gpt-image-2","prompt":"A clean studio product photograph","background":"opaque","quality":"low","size":"auto"}'`),
        ],
      ),
      section(
        tr("Savings that are actually measurable", "Экономия, которую можно измерить", "真正可测量的节省", "실제로 측정 가능한 절감"),
        [
          list(
            ["Use generation for a new asset and edits only when references improve acceptance; references add image-input cost.", "Reuse provider-recognized cached inputs where terminal usage proves the cached leg instead of assuming every repeated file is cached.", "Reject unusable outputs with a fixed visual checklist; endless prompt retries erase a nominal token discount.", "Give the image worker its own key and lifetime spending limit so batch volume cannot consume the entire account balance."],
            ["Для нового ассета используйте generation, а edits — только когда references повышают acceptance: они добавляют image-input cost.", "Считайте input cached только когда это подтверждает terminal usage; повторный файл не гарантирует cache.", "Проверяйте результат фиксированным visual checklist: бесконечные prompt retries съедают скидку.", "Выдайте image-worker отдельный ключ с общим лимитом расходов, чтобы batch не потратил весь баланс аккаунта."],
            ["新资产使用 generation；仅当参考图能提升验收率时使用 edits，因为参考图会增加 image-input 成本。", "只有终态 usage 明确证明 cached 项时才计入缓存，不要假设重复文件必然缓存。", "用固定视觉清单拒绝不可用输出；无尽提示词重试会抹掉名义折扣。", "为图像 worker 使用独立密钥与终身消费上限，避免 batch 耗尽整个账户余额。"],
            ["새 asset은 generation을 쓰고 reference가 acceptance를 높일 때만 edit을 사용합니다. reference는 image-input 비용을 더합니다.", "반복 파일이 항상 cache된다고 가정하지 말고 terminal usage가 cached leg를 증명할 때만 인정합니다.", "고정 visual checklist로 실패 output을 거부해 무한 prompt retry가 할인을 없애지 않게 합니다.", "image worker 전용 key와 평생 누적 지출 한도로 batch가 전체 잔액을 소진하지 않게 합니다."],
          ),
          note(
            "Do not publish a made-up price per image. GPT Image 2 output usage varies, exact dimensions are not promised on this subscription wire, and the terminal usage event is the billing authority.",
            "Не публикуйте выдуманную цену картинки. У GPT Image 2 меняется output usage, этот subscription wire не обещает точные dimensions, а billing authority — terminal usage event.",
            "不要发布虚构的单张价格。GPT Image 2 的 output usage 会变化，该订阅传输不承诺准确尺寸，结算权威是终态 usage event。",
            "가짜 이미지당 가격을 게시하지 마세요. GPT Image 2 output usage는 변하고 이 subscription wire는 정확한 dimensions를 약속하지 않으며 billing authority는 terminal usage event입니다.",
          ),
        ],
      ),
    ],
    faq: [
      faq(
        tr("How much does GPT Image 2 cost here?", "Сколько стоит GPT Image 2 здесь?", "本站 GPT Image 2 多少钱？", "여기서 GPT Image 2 비용은 얼마인가요?"),
        tr("A regular B2C account pays $2.50/M fresh text input, $4/M fresh image input and $15/M image output, with cached input at one quarter of fresh before the same 50% discount. The request total follows terminal usage.", "Обычный B2C платит $2.50/M fresh text input, $4/M fresh image input и $15/M image output; cached input сначала равен четверти fresh, затем получает ту же скидку 50%. Итог берётся из terminal usage.", "普通 B2C 的 fresh text input 为 $2.50/M、fresh image input 为 $4/M、image output 为 $15/M；cached input 先按 fresh 的四分之一计算，再享受相同五折。总额以终态 usage 为准。", "일반 B2C는 fresh text input $2.50/M, fresh image input $4/M, image output $15/M이며 cached input은 fresh의 1/4 계산 후 동일한 50% 할인을 받습니다. 합계는 terminal usage를 따릅니다."),
      ),
      faq(
        tr("Why is there no fixed price for one GPT Image 2 picture?", "Почему нет фиксированной цены одной картинки GPT Image 2?", "为什么没有 GPT Image 2 单张固定价格？", "왜 GPT Image 2 이미지당 고정 가격이 없나요?"),
        tr("Because billing combines actual text input, optional image references, cached usage and the image-output tokens reported for that request. PNG byte size is not the billing formula.", "Потому что billing складывает фактический text input, references, cached usage и image-output tokens конкретного запроса. Размер PNG в байтах не является формулой цены.", "因为结算组合了实际 text input、可选参考图、cached usage 与该请求报告的 image-output token；PNG 字节大小不是计费公式。", "실제 text input, 선택적 image reference, cached usage, 해당 요청의 image-output token을 합산하기 때문입니다. PNG byte 크기는 과금 공식이 아닙니다."),
      ),
      faq(
        tr("Are edits included in the 50% discount?", "На edits тоже действует скидка 50%?", "编辑也享受五折吗？", "edit에도 50% 할인이 적용되나요?"),
        tr("Yes for regular B2C: the policy applies to the complete official cost, including image-input references and image output. B2B and OpenKeys follow their own account-class rules.", "Да, для обычного B2C: политика применяется ко всей official cost, включая image-input references и image output. У B2B и OpenKeys свои правила класса аккаунта.", "普通 B2C 是：策略应用于完整官方成本，包括参考图 image input 与 image output。B2B 和 OpenKeys 遵循各自账户类型规则。", "일반 B2C에는 적용됩니다. reference image input과 image output을 포함한 전체 official cost에 정책이 적용되며 B2B/OpenKeys는 자체 account-class 규칙을 따릅니다."),
      ),
      faq(
        tr("Does GPT Image 2 support transparent backgrounds here?", "Поддерживает ли GPT Image 2 прозрачный фон здесь?", "本站 GPT Image 2 支持透明背景吗？", "여기 GPT Image 2는 투명 배경을 지원하나요?"),
        tr("No. The proved public profile supports opaque/low/auto and one PNG output; transparent background is rejected rather than silently approximated.", "Нет. Подтверждённый public profile поддерживает opaque/low/auto и один PNG; transparent background отклоняется, а не имитируется.", "不支持。已验证公开配置支持 opaque/low/auto 与单张 PNG；透明背景会被拒绝，不会静默近似。", "아닙니다. 검증된 public profile은 opaque/low/auto와 PNG 한 장을 지원하며 투명 배경은 조용히 근사하지 않고 거부합니다."),
      ),
    ],
  },
  {
    slug: "nano-banana-2-vs-gpt-image-2",
    cluster: "compare",
    related: ["nano-banana-2-api-cost", "gpt-image-2-api-cost", "cheapest-image-generation-api", "image-editing-api-guide"],
    title: tr(
      "Nano Banana 2 vs GPT Image 2 API: Cost and Capabilities",
      "Nano Banana 2 vs GPT Image 2 API: цена и возможности",
      "Nano Banana 2 vs GPT Image 2 API：成本与能力",
      "Nano Banana 2 vs GPT Image 2 API: 비용과 기능",
    ),
    h1: tr(
      "Nano Banana 2 vs GPT Image 2 for image generation",
      "Nano Banana 2 или GPT Image 2 для генерации изображений",
      "Nano Banana 2 与 GPT Image 2 图像生成对比",
      "이미지 생성용 Nano Banana 2와 GPT Image 2 비교",
    ),
    description: tr(
      "Compare Nano Banana 2 and GPT Image 2 by price, protocol, image sizes, reference limits, output format and the workloads where each image API is the safer choice.",
      "Сравните Nano Banana 2 и GPT Image 2 по цене, protocol, размерам, references, формату output и сценариям, где каждый image API подходит лучше.",
      "按价格、协议、图像尺寸、参考图限制、输出格式与适用工作负载比较 Nano Banana 2 和 GPT Image 2。",
      "가격, protocol, 이미지 크기, reference 제한, output 형식과 workload별로 Nano Banana 2와 GPT Image 2를 비교합니다.",
    ),
    keywords: tr(
      ["nano banana 2 vs gpt image 2", "best image generation api", "gpt image vs gemini image", "nano banana api comparison", "ai image api comparison", "image generation api cost"],
      ["nano banana 2 или gpt image 2", "лучший api генерации изображений", "gpt image vs gemini image", "сравнение nano banana api", "сравнение ai image api", "стоимость image generation api"],
      ["nano banana 2 对比 gpt image 2", "最佳图像生成 api", "gpt image 对比 gemini image", "nano banana api 对比", "ai 图像 api 对比", "图像生成 api 成本"],
      ["nano banana 2 vs gpt image 2", "최고의 이미지 생성 api", "gpt image vs gemini image", "nano banana api 비교", "ai image api 비교", "이미지 생성 api 비용"],
    ),
    dek: tr(
      "Nano Banana 2 offers explicit 1K/2K/4K sizes, broad aspect ratios and up to 14 references on the native Gemini shape. GPT Image 2 offers a narrow OpenAI Images route with one-to-five PNG references and terminal token billing. Both receive the regular B2C 50% discount.",
      "Nano Banana 2 даёт явные 1K/2K/4K, широкий набор aspect ratios и до 14 references в native Gemini shape. GPT Image 2 использует узкий OpenAI Images route с 1–5 PNG и terminal token billing. На обе модели действует обычная B2C-скидка 50%.",
      "Nano Banana 2 在原生 Gemini 结构中提供明确的 1K/2K/4K、丰富宽高比与最多 14 张参考图。GPT Image 2 使用有界 OpenAI Images 路由，支持 1–5 张 PNG，并按终态 token 计费。普通 B2C 两者均五折。",
      "Nano Banana 2는 native Gemini 형식에서 명시적 1K/2K/4K, 다양한 aspect ratio, 최대 14 references를 제공합니다. GPT Image 2는 1~5 PNG와 terminal token billing의 제한된 OpenAI Images route를 사용하며 둘 다 일반 B2C 50% 할인을 받습니다.",
    ),
    sections: [
      section(
        tr("Side-by-side contract", "Контракт side by side", "并排比较接口契约", "계약 나란히 비교"),
        [
          table(
            { headers: ["Decision", "Nano Banana 2", "GPT Image 2"], rows: [["Model", "gemini-3.1-flash-image", "gpt-image-2"], ["Protocol", "Gemini generateContent + x-goog-api-key", "OpenAI Images + Bearer"], ["Output", "inlineData image part", "one non-streaming base64 PNG"], ["Published sizes", "1K, 2K, 4K", "auto; exact dimensions not promised"], ["References", "up to 14 supported image inputs", "1–5 strict PNG files"], ["Price authority", "fixed image tokens by size + other legs", "terminal text/image input and image-output usage"]] },
            { headers: ["Критерий", "Nano Banana 2", "GPT Image 2"], rows: [["Модель", "gemini-3.1-flash-image", "gpt-image-2"], ["Protocol", "Gemini generateContent + x-goog-api-key", "OpenAI Images + Bearer"], ["Output", "inlineData image part", "один non-streaming base64 PNG"], ["Размеры", "1K, 2K, 4K", "auto; exact dimensions не обещаны"], ["References", "до 14 image inputs", "1–5 строгих PNG"], ["Price authority", "фиксированные image tokens по size + другие legs", "terminal text/image input и image-output usage"]] },
            { headers: ["决策项", "Nano Banana 2", "GPT Image 2"], rows: [["模型", "gemini-3.1-flash-image", "gpt-image-2"], ["协议", "Gemini generateContent + x-goog-api-key", "OpenAI Images + Bearer"], ["输出", "inlineData 图像 part", "单张非流式 base64 PNG"], ["已发布尺寸", "1K、2K、4K", "auto；不承诺准确尺寸"], ["参考图", "最多 14 张受支持图像输入", "1–5 张严格 PNG"], ["价格权威", "按尺寸固定 image token + 其他项", "终态 text/image input 与 image-output usage"]] },
            { headers: ["결정", "Nano Banana 2", "GPT Image 2"], rows: [["모델", "gemini-3.1-flash-image", "gpt-image-2"], ["Protocol", "Gemini generateContent + x-goog-api-key", "OpenAI Images + Bearer"], ["Output", "inlineData image part", "non-streaming base64 PNG 한 장"], ["공개 크기", "1K, 2K, 4K", "auto; 정확한 dimensions 미보장"], ["References", "최대 14개 지원 image input", "1~5 strict PNG"], ["Price authority", "크기별 고정 image token + 기타 leg", "terminal text/image input 및 image-output usage"]] },
          ),
          note(
            "The protocols are not interchangeable. A successful model choice still fails if a Gemini inlineData response is sent to a client that only knows the OpenAI Images schema.",
            "Protocols невзаимозаменяемы. Даже правильная модель не поможет, если Gemini inlineData попадёт в client, понимающий только OpenAI Images schema.",
            "两种协议不可互换。如果把 Gemini inlineData 响应交给只理解 OpenAI Images schema 的客户端，正确的模型选择仍会失败。",
            "protocol은 교환할 수 없습니다. Gemini inlineData 응답을 OpenAI Images schema만 아는 client에 보내면 올바른 모델 선택도 실패합니다.",
          ),
        ],
      ),
      section(
        tr("Choose by acceptance criteria", "Выбирайте по acceptance criteria", "按验收标准选择", "acceptance 기준으로 선택"),
        [
          steps(
            ["Write a fixed test set with text-only generation, one-reference editing and your hardest aspect ratio.", "Run the same visual brief on both models without silently changing resolution or references.", "Score instruction fidelity, product/reference fidelity, artifacts, latency and settled charge.", "Pin the winner per asset class; do not make one image model the global default without evidence."],
            ["Соберите fixed test set: text-only generation, edit с одной reference и самый сложный aspect ratio.", "Запустите один visual brief на обеих моделях без скрытой смены resolution или references.", "Оцените instruction fidelity, product/reference fidelity, artifacts, latency и settled charge.", "Закрепите победителя по классу ассетов, а не делайте одну image-модель global default без evidence."],
            ["建立固定测试集：纯文本生成、单参考图编辑以及最困难的宽高比。", "两种模型使用同一视觉 brief，不要静默改变分辨率或参考图。", "评分指令遵循、产品/参考图一致性、瑕疵、延迟与结算费用。", "按资产类别固定胜出模型，不要在缺乏证据时设为全局默认。"],
            ["text-only generation, reference 한 장 edit, 가장 어려운 aspect ratio의 고정 test set을 만듭니다.", "resolution이나 references를 몰래 바꾸지 않고 두 모델에 같은 visual brief를 실행합니다.", "instruction fidelity, product/reference fidelity, artifact, latency, settled charge를 채점합니다.", "근거 없이 한 image model을 global default로 두지 말고 asset class별 승자를 고정합니다."],
          ),
          paragraph(
            "Choose Nano Banana 2 when explicit size/aspect controls or many references are contract requirements. Choose GPT Image 2 when an existing OpenAI Images client, strict PNG edit flow or one-output contract reduces integration risk. Quality still needs your own eval.",
            "Выбирайте Nano Banana 2, если нужны явные size/aspect controls или много references. GPT Image 2 подходит, когда существующий OpenAI Images client, строгий PNG edit flow или контракт одного output снижает integration risk. Качество всё равно требует вашего eval.",
            "明确尺寸/宽高比或多参考图是硬性要求时选 Nano Banana 2；已有 OpenAI Images 客户端、严格 PNG 编辑流程或单输出契约能降低集成风险时选 GPT Image 2。质量仍需自有评测。",
            "명시적 size/aspect control이나 많은 references가 계약 요건이면 Nano Banana 2, 기존 OpenAI Images client·strict PNG edit·one-output 계약이 통합 위험을 줄이면 GPT Image 2를 선택하세요. 품질은 자체 eval이 필요합니다.",
          ),
        ],
      ),
      section(
        tr("Compare total cost, not one headline", "Сравнивайте total cost, а не один headline", "比较总成本而非单一报价", "한 가지 headline이 아닌 총비용 비교"),
        [
          list(
            ["Nano Banana 2 has a predictable image-output leg by size, but input, text/thinking and grounding can add cost.", "GPT Image 2 combines actual token legs; reference edits cost more input than prompt-only generation.", "Both are 50% off official spend for regular B2C, so retries and acceptance rate often decide the cheaper workflow.", "Compare the charge per accepted asset, including failed outputs, storage, validation and human review."],
            ["У Nano Banana 2 предсказуемый image-output leg по size, но input, text/thinking и grounding добавляют стоимость.", "GPT Image 2 складывает actual token legs; reference edits требуют больше input, чем prompt-only generation.", "Обе модели стоят на 50% дешевле official spend для обычного B2C, поэтому итог часто решают retries и acceptance rate.", "Сравнивайте charge на принятый ассет, включая неудачные outputs, storage, validation и human review."],
            ["Nano Banana 2 的图像输出项可按尺寸预测，但输入、文本/思考与 grounding 会增加成本。", "GPT Image 2 汇总实际 token 项；参考图编辑的输入成本高于纯提示词生成。", "普通 B2C 两者均为官方成本五折，因此重试与验收率常决定哪个工作流更便宜。", "比较每个已验收资产的费用，并计入失败输出、存储、验证与人工审核。"],
            ["Nano Banana 2는 크기별 image-output leg가 예측 가능하지만 input, text/thinking, grounding 비용이 더해집니다.", "GPT Image 2는 actual token leg를 합산하며 reference edit은 prompt-only generation보다 input 비용이 큽니다.", "일반 B2C에서 둘 다 official spend의 50%이므로 retry와 acceptance rate가 더 저렴한 workflow를 결정하는 경우가 많습니다.", "실패 output, storage, validation, human review를 포함한 accepted asset당 charge를 비교합니다."],
          ),
        ],
      ),
    ],
    faq: [
      faq(tr("Which API is cheaper, Nano Banana 2 or GPT Image 2?", "Какой API дешевле: Nano Banana 2 или GPT Image 2?", "Nano Banana 2 和 GPT Image 2 哪个更便宜？", "Nano Banana 2와 GPT Image 2 중 어느 API가 더 저렴한가요?"), tr("There is no universal winner. Nano Banana 2 exposes a fixed image-output cost by size; GPT Image 2 settles variable token legs. Compare cost per accepted asset on your prompts and references.", "Универсального победителя нет. Nano Banana 2 даёт fixed image-output cost по size, GPT Image 2 — variable token legs. Сравните cost per accepted asset на своих prompts и references.", "没有通用答案。Nano Banana 2 按尺寸提供固定图像输出成本；GPT Image 2 结算可变 token 项。应使用自己的提示词与参考图比较每个验收资产的成本。", "보편적 승자는 없습니다. Nano Banana 2는 크기별 고정 image-output 비용, GPT Image 2는 가변 token leg를 정산하므로 실제 prompt/reference의 accepted asset당 비용을 비교하세요.")),
      faq(tr("Which model supports more reference images?", "Какая модель поддерживает больше references?", "哪款模型支持更多参考图？", "어느 모델이 더 많은 reference image를 지원하나요?"), tr("Nano Banana 2 accepts up to 14 supported image inputs. GPT Image 2 edits accept one to five strict PNG references on the published route.", "Nano Banana 2 принимает до 14 поддерживаемых image inputs. Публичный GPT Image 2 edits route принимает 1–5 строгих PNG references.", "Nano Banana 2 最多接受 14 张受支持图像输入；GPT Image 2 已发布编辑路由接受 1–5 张严格 PNG 参考图。", "Nano Banana 2는 최대 14개 지원 image input, GPT Image 2 공개 edit route는 1~5 strict PNG references를 받습니다.")),
      faq(tr("Can both models use the same API request?", "Можно ли вызвать обе модели одним API request?", "两款模型能使用同一种 API 请求吗？", "두 모델에 같은 API request를 쓸 수 있나요?"), tr("No. Nano Banana 2 uses native Gemini generateContent and x-goog-api-key; GPT Image 2 uses OpenAI Images routes and Authorization: Bearer.", "Нет. Nano Banana 2 использует native Gemini generateContent и x-goog-api-key; GPT Image 2 — OpenAI Images routes и Authorization: Bearer.", "不能。Nano Banana 2 使用原生 Gemini generateContent 与 x-goog-api-key；GPT Image 2 使用 OpenAI Images 路由与 Authorization: Bearer。", "아닙니다. Nano Banana 2는 native Gemini generateContent와 x-goog-api-key, GPT Image 2는 OpenAI Images route와 Authorization: Bearer를 사용합니다.")),
      faq(tr("Do both receive the 50% discount?", "На обе модели действует скидка 50%?", "两款模型都享受五折吗？", "두 모델 모두 50% 할인을 받나요?"), tr("Yes for regular B2C accounts after exact official usage is calculated. B2B and OpenKeys use their own pricing policies.", "Да, для обычных B2C после расчёта exact official usage. У B2B и OpenKeys свои pricing policies.", "普通 B2C 在准确官方 usage 计算后，两款模型都享受五折；B2B 与 OpenKeys 使用各自定价策略。", "일반 B2C는 exact official usage 계산 후 둘 다 50% 할인을 받으며 B2B/OpenKeys는 자체 pricing policy를 사용합니다.")),
    ],
  },
  {
    slug: "image-generation-api-pricing",
    cluster: "explain",
    related: ["nano-banana-2-api-cost", "gpt-image-2-api-cost", "cheapest-image-generation-api", "how-billing-works"],
    title: tr(
      "Image Generation API Pricing: Tokens, Images and Discounts",
      "Цены API генерации изображений: tokens, картинки и скидки",
      "图像生成 API 定价：Token、图像与折扣",
      "이미지 생성 API 가격: token, 이미지, 할인",
    ),
    h1: tr(
      "How image generation API pricing works",
      "Как устроена цена API генерации изображений",
      "图像生成 API 如何计价",
      "이미지 생성 API 가격 계산 방식",
    ),
    description: tr(
      "Understand AI image API pricing across Nano Banana 2 and GPT Image 2: input, references, image-output tokens, size, retries, account-class discounts and cost per accepted asset.",
      "Разберитесь в цене AI image API для Nano Banana 2 и GPT Image 2: input, references, image-output tokens, size, retries, скидки по классу аккаунта и цена принятого ассета.",
      "了解 Nano Banana 2 与 GPT Image 2 的 AI 图像 API 定价：输入、参考图、图像输出 token、尺寸、重试、账户折扣及每个验收资产成本。",
      "Nano Banana 2와 GPT Image 2의 input, reference, image-output token, 크기, retry, 계정 등급 할인과 accepted asset당 비용을 이해하세요.",
    ),
    keywords: tr(
      ["image generation api pricing", "ai image api cost", "image generator api price", "image generation token pricing", "cheap image api", "ai image generation discount"],
      ["цена api генерации изображений", "стоимость ai image api", "image generator api цена", "image generation token pricing", "дешевый image api", "скидка ai генерация"],
      ["图像生成 api 定价", "ai 图像 api 成本", "图像生成器 api 价格", "图像生成 token 定价", "便宜图像 api", "ai 图像生成折扣"],
      ["이미지 생성 api 가격", "ai image api 비용", "image generator api 가격", "image generation token 가격", "저렴한 image api", "ai 이미지 생성 할인"],
    ),
    dek: tr(
      "The useful unit is not price per request but cost per accepted asset. Start with authoritative usage legs, apply the account's policy, then include retries and validation failures that never reach production.",
      "Полезная единица — не цена request, а cost per accepted asset. Начните с authoritative usage legs, примените pricing policy аккаунта и добавьте retries и failed validations, которые не дошли до production.",
      "真正有用的单位不是每次请求价格，而是每个已验收资产成本。先按权威 usage 项计算，再应用账户策略，并计入未进入生产的重试与验证失败。",
      "유용한 단위는 request당 가격이 아니라 accepted asset당 비용입니다. authoritative usage leg에서 시작해 계정 policy를 적용하고 production에 쓰이지 못한 retry와 validation 실패까지 포함합니다.",
    ),
    sections: [
      section(
        tr("The complete cost equation", "Полная формула стоимости", "完整成本公式", "전체 비용 공식"),
        [
          table(
            { headers: ["Component", "Nano Banana 2", "GPT Image 2"], rows: [["Prompt/text input", "$0.50/M official", "$5/M official"], ["Reference image input", "input tokens at model rate", "$8/M official"], ["Rendered image", "$60/M image tokens; fixed counts by size", "$30/M actual image-output tokens"], ["Cache", "no image-model input discount", "cached input at 25% of fresh"], ["Regular B2C", "50% off exact official total", "50% off exact official total"]] },
            { headers: ["Компонент", "Nano Banana 2", "GPT Image 2"], rows: [["Prompt/text input", "$0.50/M official", "$5/M official"], ["Reference image input", "input tokens по ставке модели", "$8/M official"], ["Rendered image", "$60/M image tokens; fixed counts по size", "$30/M actual image-output tokens"], ["Cache", "нет input-скидки image-модели", "cached input = 25% fresh"], ["Обычный B2C", "50% от exact official total", "50% от exact official total"]] },
            { headers: ["组成", "Nano Banana 2", "GPT Image 2"], rows: [["Prompt/text input", "$0.50/M 官方", "$5/M 官方"], ["参考图输入", "按模型费率计算 input token", "$8/M 官方"], ["渲染图像", "$60/M image token；按尺寸固定数量", "$30/M 实际 image-output token"], ["缓存", "该图像模型无 input 折扣", "cached input 为 fresh 的 25%"], ["普通 B2C", "准确官方总额五折", "准确官方总额五折"]] },
            { headers: ["구성", "Nano Banana 2", "GPT Image 2"], rows: [["Prompt/text input", "$0.50/M 공식", "$5/M 공식"], ["Reference image input", "모델 요금의 input token", "$8/M 공식"], ["Rendered image", "$60/M image token; 크기별 고정 수량", "$30/M 실제 image-output token"], ["Cache", "image-model input 할인 없음", "cached input은 fresh의 25%"], ["일반 B2C", "exact official total의 50%", "exact official total의 50%"]] },
          ),
          paragraph(
            "Charge per accepted asset = (all settled request charges, including rejected outputs) ÷ accepted assets. This exposes workflows that look cheap per token but require three or four retries.",
            "Charge per accepted asset = все settled request charges, включая rejected outputs, ÷ принятые ассеты. Формула показывает workflows, которые выглядят дешёвыми по token rate, но требуют 3–4 retries.",
            "每个验收资产费用 = 所有已结算请求费用（含被拒输出）÷ 验收资产数。它能揭示单 token 看似便宜、却需要三四次重试的工作流。",
            "accepted asset당 charge = rejected output을 포함한 모든 settled request charge ÷ accepted asset 수입니다. token rate는 싸지만 3~4번 retry하는 workflow를 드러냅니다.",
          ),
        ],
      ),
      section(
        tr("Discount depends on account class", "Скидка зависит от класса аккаунта", "折扣取决于账户类型", "할인은 계정 등급에 따라 다름"),
        [
          table(
            { headers: ["Account class", "Pricing rule"], rows: [["Regular B2C", "Global 50% discount, then any more-specific valid rule"], ["B2B", "Only its negotiated provider/model policy"], ["OpenKeys", "Official 1:1 pricing; no B2C discount"], ["Service", "Meter-only; no customer charge"]] },
            { headers: ["Класс", "Pricing rule"], rows: [["Обычный B2C", "Глобальные 50%, затем более specific valid rule"], ["B2B", "Только согласованная provider/model policy"], ["OpenKeys", "Official 1:1; без B2C-скидки"], ["Service", "Meter-only; customer charge не вычисляется"]] },
            { headers: ["账户类型", "定价规则"], rows: [["普通 B2C", "全局五折，再应用更具体的有效规则"], ["B2B", "仅使用协商后的 provider/model 策略"], ["OpenKeys", "官方 1:1；无 B2C 折扣"], ["Service", "仅计量；不计算客户扣费"]] },
            { headers: ["계정 등급", "Pricing rule"], rows: [["일반 B2C", "글로벌 50% 후 더 구체적인 valid rule"], ["B2B", "협상된 provider/model policy만"], ["OpenKeys", "공식 1:1; B2C 할인 없음"], ["Service", "Meter-only; customer charge 없음"]] },
          ),
          note(
            "A discount changes the payable amount, not model availability. Always discover the model with the same key before building a budget or publishing an availability claim.",
            "Скидка меняет payable amount, а не model availability. Всегда проверяйте модель тем же ключом до бюджета или публичного availability claim.",
            "折扣改变应付金额，不改变模型可用性。制定预算或发布可用性声明前，必须使用同一密钥发现模型。",
            "할인은 payable amount를 바꾸지만 model availability는 바꾸지 않습니다. 예산이나 availability claim 전에 같은 key로 모델을 discovery하세요.",
          ),
        ],
      ),
      section(
        tr("Build a defensible budget", "Постройте проверяемый бюджет", "建立可验证预算", "검증 가능한 예산 만들기"),
        [
          steps(
            ["Discover the exact image model and protocol with the production key.", "Estimate or cap input, references, requested size and maximum attempts.", "Run one bounded generation and save terminal usage, request ID and discounted charge.", "Measure acceptance rate on a representative set, then multiply cost per accepted asset by forecast volume.", "Set the key's lifetime spending limit below the business budget and alert before it is exhausted."],
            ["Найдите exact image model и protocol production-ключом.", "Ограничьте input, references, requested size и maximum attempts.", "Выполните bounded generation и сохраните terminal usage, request ID и discounted charge.", "Измерьте acceptance rate на representative set и умножьте cost per accepted asset на прогнозный объём.", "Задайте lifetime spending limit ключа ниже business budget и alert до исчерпания."],
            ["使用生产密钥发现准确图像模型与协议。", "估算或限制输入、参考图、请求尺寸与最大尝试次数。", "运行一次有界生成，保存终态 usage、request ID 与折后费用。", "在代表性集合上测量验收率，再用每个验收资产成本乘以预测数量。", "把密钥终身消费上限设在业务预算之下，并在耗尽前告警。"],
            ["production key로 exact image model과 protocol을 discovery합니다.", "input, references, requested size, maximum attempts를 추정하거나 제한합니다.", "bounded generation 한 번의 terminal usage, request ID, discounted charge를 저장합니다.", "representative set에서 acceptance rate를 측정해 accepted asset당 비용에 예상 물량을 곱합니다.", "key 평생 누적 지출 한도를 business budget 아래로 두고 소진 전 alert합니다."],
          ),
        ],
      ),
    ],
    faq: [
      faq(tr("What is the cheapest way to estimate an image request?", "Как дешевле всего оценить image request?", "估算图像请求最便宜的方法是什么？", "image request를 가장 저렴하게 추정하는 방법은?"), tr("For Nano Banana 2, countTokens estimates input without generating an image, then add the fixed output leg for 1K/2K/4K. GPT Image 2 requires a bounded real request and terminal usage for an authoritative total.", "Для Nano Banana 2 countTokens бесплатно оценивает input, затем добавляется fixed output leg 1K/2K/4K. Для authoritative total GPT Image 2 нужен bounded real request и terminal usage.", "Nano Banana 2 可先用 countTokens 免费估算输入，再加 1K/2K/4K 固定输出项。GPT Image 2 的权威总额需要一次有界真实请求与终态 usage。", "Nano Banana 2는 countTokens로 input을 무료 추정한 뒤 1K/2K/4K 고정 output leg를 더합니다. GPT Image 2 authoritative total은 bounded real request와 terminal usage가 필요합니다.")),
      faq(tr("Does 50% off mean every picture costs half a fixed list price?", "Означает ли 50%, что каждая картинка стоит половину fixed list price?", "五折是否意味着每张图片都是固定标价的一半？", "50% 할인은 모든 이미지가 고정 가격의 절반이라는 뜻인가요?"), tr("No. The policy halves the exact official usage cost for regular B2C. Nano Banana 2 has a predictable image leg by size; GPT Image 2 still has variable terminal token usage.", "Нет. Политика делит exact official usage cost обычного B2C пополам. У Nano Banana 2 предсказуем image leg по size, а GPT Image 2 сохраняет variable terminal usage.", "不是。普通 B2C 的策略把准确官方 usage 成本减半。Nano Banana 2 的图像项按尺寸可预测，GPT Image 2 仍按可变终态 token usage。", "아닙니다. 일반 B2C의 exact official usage cost를 절반으로 줄입니다. Nano Banana 2는 크기별 image leg가 예측 가능하지만 GPT Image 2는 terminal token usage가 가변입니다.")),
      faq(tr("Should failed images be included in the budget?", "Нужно ли учитывать неудачные картинки?", "预算是否要计入失败图像？", "실패 이미지도 예산에 포함해야 하나요?"), tr("Yes. If a request delivered and settled usage, its charge is part of acquisition cost even when the asset fails your quality check.", "Да. Если request доставил результат и settled usage, его charge входит в acquisition cost, даже если ассет не прошёл quality check.", "要。如果请求已交付并结算 usage，即使资产未通过质量检查，其费用仍属于获取成本。", "예. request가 결과와 settled usage를 전달했다면 asset이 quality check를 통과하지 못해도 charge는 acquisition cost에 포함됩니다.")),
      faq(tr("Where do I verify the final image charge?", "Где проверить итоговое списание за image?", "在哪里核对最终图像扣费？", "최종 image charge는 어디서 확인하나요?"), tr("Use terminal provider usage together with the matching dashboard ledger entry. Do not infer money from file size, dimensions or partial output.", "Сверьте terminal provider usage с matching ledger entry в дашборде. Не выводите сумму из file size, dimensions или partial output.", "把提供商终态 usage 与匹配的仪表板账本记录一起核对；不要从文件大小、尺寸或部分输出推断费用。", "terminal provider usage와 matching dashboard ledger entry를 함께 확인하고 file size, dimensions, partial output으로 금액을 추론하지 마세요.")),
    ],
  },
  {
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
  },
  {
    slug: "image-editing-api-guide",
    cluster: "integrate",
    related: ["gpt-image-2-api-guide", "nano-banana-2-api-guide", "nano-banana-2-vs-gpt-image-2", "image-generation-api-for-ecommerce"],
    title: tr(
      "Image Editing API Guide: GPT Image 2 and Nano Banana 2",
      "API редактирования изображений: GPT Image 2 и Nano Banana 2",
      "图像编辑 API 指南：GPT Image 2 与 Nano Banana 2",
      "이미지 편집 API 가이드: GPT Image 2와 Nano Banana 2",
    ),
    h1: tr(
      "Edit images through GPT Image 2 or Nano Banana 2",
      "Редактирование изображений через GPT Image 2 или Nano Banana 2",
      "使用 GPT Image 2 或 Nano Banana 2 编辑图像",
      "GPT Image 2 또는 Nano Banana 2로 이미지 편집",
    ),
    description: tr(
      "Build an image-editing API workflow with GPT Image 2 or Nano Banana 2: reference limits, formats, endpoints, cost controls, validation and safe retry rules.",
      "Постройте image-editing API workflow на GPT Image 2 или Nano Banana 2: лимиты references, форматы, endpoints, контроль цены, validation и безопасные retries.",
      "使用 GPT Image 2 或 Nano Banana 2 构建图像编辑 API 工作流：参考图限制、格式、端点、成本控制、验证与安全重试。",
      "GPT Image 2 또는 Nano Banana 2로 reference 제한, 형식, endpoint, 비용 통제, validation, 안전한 retry를 갖춘 image-editing workflow를 만드세요.",
    ),
    keywords: tr(
      ["image editing api", "ai image edit api", "gpt image 2 edit api", "nano banana image editing", "reference image api", "product image editing api"],
      ["api редактирования изображений", "ai image edit api", "gpt image 2 edit api", "nano banana редактирование", "reference image api", "api обработки фото товара"],
      ["图像编辑 api", "ai 图像编辑 api", "gpt image 2 编辑 api", "nano banana 图像编辑", "参考图 api", "产品图像编辑 api"],
      ["이미지 편집 api", "ai image edit api", "gpt image 2 edit api", "nano banana 이미지 편집", "reference image api", "상품 이미지 편집 api"],
    ),
    dek: tr(
      "GPT Image 2 exposes a strict multipart PNG edit route. Nano Banana 2 treats references as native multimodal input and supports more formats and images. The right choice depends on the reference contract your application can validate.",
      "GPT Image 2 предоставляет strict multipart PNG edit route. Nano Banana 2 принимает references как native multimodal input и поддерживает больше форматов и изображений. Выбор зависит от reference contract, который приложение умеет валидировать.",
      "GPT Image 2 提供严格 multipart PNG 编辑路由。Nano Banana 2 把参考图作为原生多模态输入，支持更多格式与数量。应根据应用可验证的参考图契约选择。",
      "GPT Image 2는 strict multipart PNG edit route를, Nano Banana 2는 references를 native multimodal input으로 받아 더 많은 형식과 수량을 지원합니다. 앱이 검증할 수 있는 reference contract로 선택하세요.",
    ),
    sections: [
      section(
        tr("Choose the reference contract", "Выберите reference contract", "选择参考图契约", "reference contract 선택"),
        [
          table(
            { headers: ["Capability", "GPT Image 2", "Nano Banana 2"], rows: [["Route", "POST /v1/images/edits", "generateContent with inlineData"], ["References", "1–5", "up to 14"], ["Input files", "strict PNG", "PNG, JPEG, WEBP, HEIC, HEIF"], ["Output", "one base64 PNG", "image inlineData part"], ["Controls", "opaque / low / auto", "1K/2K/4K + published aspect ratios"]] },
            { headers: ["Capability", "GPT Image 2", "Nano Banana 2"], rows: [["Route", "POST /v1/images/edits", "generateContent с inlineData"], ["References", "1–5", "до 14"], ["Input files", "strict PNG", "PNG, JPEG, WEBP, HEIC, HEIF"], ["Output", "один base64 PNG", "image inlineData part"], ["Controls", "opaque / low / auto", "1K/2K/4K + aspect ratios"]] },
            { headers: ["能力", "GPT Image 2", "Nano Banana 2"], rows: [["路由", "POST /v1/images/edits", "含 inlineData 的 generateContent"], ["参考图", "1–5", "最多 14"], ["输入文件", "严格 PNG", "PNG、JPEG、WEBP、HEIC、HEIF"], ["输出", "单张 base64 PNG", "image inlineData part"], ["控制", "opaque / low / auto", "1K/2K/4K + 已发布宽高比"]] },
            { headers: ["기능", "GPT Image 2", "Nano Banana 2"], rows: [["Route", "POST /v1/images/edits", "inlineData 포함 generateContent"], ["References", "1~5", "최대 14"], ["Input files", "strict PNG", "PNG, JPEG, WEBP, HEIC, HEIF"], ["Output", "base64 PNG 한 장", "image inlineData part"], ["Controls", "opaque / low / auto", "1K/2K/4K + 공개 aspect ratio"]] },
          ),
          sharedCode(`curl ${OPENAI}/images/edits \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -F "model=gpt-image-2" \\
  -F "prompt=Keep the product unchanged and replace only the background" \\
  -F "image=@reference.png;type=image/png"`),
        ],
      ),
      section(
        tr("Validate before dispatch", "Валидируйте до dispatch", "发送前验证", "dispatch 전 검증"),
        [
          steps(
            ["Decode and inspect every reference server-side; reject unexpected MIME, empty files and oversized payloads before reservation.", "Write an edit brief that separates immutable product traits from the requested change.", "Choose the exact route and output contract your client can decode; do not mix inlineData with OpenAI Images parsing.", "After delivery, validate dimensions, format, product identity and prohibited changes before publishing.", "Store request ID and terminal usage with the source and result for rollback and cost attribution."],
            ["Декодируйте и проверяйте каждую reference server-side; отклоняйте unexpected MIME, empty и oversized payloads до reservation.", "Разделите в edit brief неизменяемые свойства продукта и requested change.", "Выберите route и output contract, который умеет декодировать client; не смешивайте inlineData с OpenAI Images parser.", "После delivery проверьте dimensions, format, product identity и запрещённые изменения.", "Храните request ID и terminal usage с source/result для rollback и cost attribution."],
            ["服务端解码并检查每张参考图；在预留前拒绝异常 MIME、空文件与超限 payload。", "在编辑 brief 中分离不可变产品特征与请求变更。", "选择客户端能解码的准确路由与输出契约，不要把 inlineData 与 OpenAI Images 解析混用。", "交付后验证尺寸、格式、产品一致性与禁止改动，再发布。", "把 request ID 与终态 usage 和源图/结果一起保存，以便回滚与成本归因。"],
            ["모든 reference를 server-side에서 decode·검사하고 reservation 전에 예상 밖 MIME, empty, oversized payload를 거부합니다.", "edit brief에서 불변 product 특성과 requested change를 분리합니다.", "client가 decode할 exact route/output contract를 고르고 inlineData와 OpenAI Images parsing을 섞지 않습니다.", "delivery 후 dimensions, format, product identity, 금지 변경을 검증한 뒤 게시합니다.", "rollback과 cost attribution을 위해 source/result에 request ID와 terminal usage를 저장합니다."],
          ),
        ],
      ),
      section(
        tr("Control edit cost and retries", "Контролируйте цену edits и retries", "控制编辑成本与重试", "edit 비용과 retry 통제"),
        [
          list(
            ["References are billable input. Send only files that constrain the requested edit.", "A provider-delivered edit is not safe to replay automatically after timeout ambiguity or output delivery.", "Limit variants and attempts per source asset; quality review must end the loop.", "Use separate keys for production edits and experiments so limits and attribution remain clear.", "Regular B2C gets 50% off official usage, but an unnecessary reference or retry is still unnecessary spend."],
            ["References — billable input. Отправляйте только файлы, ограничивающие requested edit.", "Provider-delivered edit нельзя автоматически replay после ambiguous timeout или output delivery.", "Ограничьте variants и attempts на source asset; quality review должен завершать loop.", "Разделите ключи production edits и experiments для ясных limits/attribution.", "Обычный B2C получает 50%, но ненужная reference или retry остаётся лишней тратой."],
            ["参考图属于计费输入，只发送确实约束编辑的文件。", "发生超时歧义或输出已交付后，不要自动重放提供商编辑。", "限制每个源资产的变体与尝试次数，让质量审核终止循环。", "生产编辑与实验使用不同密钥，保持限制与归因清晰。", "普通 B2C 可享五折，但不必要参考图或重试仍是浪费。"],
            ["reference는 billable input이므로 requested edit을 제약하는 파일만 보냅니다.", "ambiguous timeout이나 output delivery 후 provider-delivered edit을 자동 replay하지 않습니다.", "source asset당 variant와 attempt를 제한하고 quality review가 loop를 끝내게 합니다.", "production edit과 experiment key를 분리해 limit/attribution을 명확히 합니다.", "일반 B2C는 50% 할인되지만 불필요한 reference/retry는 여전히 낭비입니다."],
          ),
        ],
      ),
    ],
    faq: [
      faq(tr("Which API accepts more reference images?", "Какой API принимает больше references?", "哪个 API 接受更多参考图？", "어느 API가 더 많은 reference image를 받나요?"), tr("Nano Banana 2 accepts up to 14 supported image inputs. GPT Image 2 accepts one to five strict PNG files on its edits route.", "Nano Banana 2 принимает до 14 поддерживаемых image inputs. GPT Image 2 edits route принимает 1–5 строгих PNG.", "Nano Banana 2 最多接受 14 张受支持图像输入；GPT Image 2 编辑路由接受 1–5 张严格 PNG。", "Nano Banana 2는 최대 14개 지원 image input, GPT Image 2 edit route는 1~5 strict PNG를 받습니다.")),
      faq(tr("Can GPT Image 2 edit JPEG directly?", "Можно ли GPT Image 2 напрямую редактировать JPEG?", "GPT Image 2 能直接编辑 JPEG 吗？", "GPT Image 2가 JPEG를 직접 편집할 수 있나요?"), tr("Not on the published route. Convert and validate it as PNG before multipart upload, or use Nano Banana 2 when its supported JPEG input contract fits the workflow.", "Не на published route. Конвертируйте и проверьте JPEG как PNG до multipart upload либо используйте Nano Banana 2 с поддерживаемым JPEG contract.", "已发布路由不支持。multipart 上传前转换并验证为 PNG，或在适合工作流时使用支持 JPEG 输入的 Nano Banana 2。", "공개 route에서는 안 됩니다. multipart upload 전 PNG로 변환·검증하거나 지원 JPEG input contract가 맞으면 Nano Banana 2를 사용하세요.")),
      faq(tr("Do edits cost more than prompt-only generation?", "Edits дороже prompt-only generation?", "编辑是否比纯提示词生成更贵？", "edit이 prompt-only generation보다 비싼가요?"), tr("They add billable image input, so an otherwise comparable edit normally has more input cost. It may still be cheaper per accepted asset if references reduce retries.", "Они добавляют billable image input, поэтому comparable edit обычно дороже по input. Но он может быть дешевле per accepted asset, если references снижают retries.", "编辑增加计费 image input，因此可比编辑通常输入成本更高；若参考图减少重试，每个验收资产成本仍可能更低。", "billable image input이 추가되어 comparable edit은 보통 input 비용이 더 큽니다. reference가 retry를 줄이면 accepted asset당 비용은 더 낮을 수 있습니다.")),
      faq(tr("Can I retry an edit after a timeout?", "Можно ли retry edit после timeout?", "超时后能否重试编辑？", "timeout 후 edit을 retry해도 되나요?"), tr("Only when you can prove the prior attempt was not accepted. An ambiguous timeout may hide completed provider work; preserve the request ID and reconcile before another paid attempt.", "Только если доказано, что prior attempt не был принят. Ambiguous timeout может скрывать выполненную работу provider; сохраните request ID и проведите reconciliation до нового paid attempt.", "只有能证明前一次尝试未被接受时才可重试。歧义超时可能隐藏已完成的提供商工作；应保留 request ID 并先核对。", "prior attempt가 accepted되지 않았음을 증명할 때만 가능합니다. ambiguous timeout은 완료된 provider 작업을 숨길 수 있으므로 request ID를 보존하고 다음 paid attempt 전에 대조하세요.")),
    ],
  },
  {
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
  },
  {
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
  },
];

function contentFor(spec: ImageSeoSpec, locale: Locale): LocalizedContent {
  return {
    title: spec.title[locale],
    h1: spec.h1[locale],
    description: spec.description[locale],
    keywords: spec.keywords[locale],
    dek: spec.dek[locale],
    sections: spec.sections.map((item) => item[locale]),
    faq: spec.faq.map((item) => item[locale]),
  };
}

export const learnImageSeoEn: LearnArticle[] = imageSeoSpecs.map((spec) => ({
  slug: spec.slug,
  cluster: spec.cluster,
  related: spec.related,
  ...contentFor(spec, "en"),
  published: "2026-08-09",
  updated: "2026-08-09",
}));

export const learnImageSeoRu: Record<string, LocalizedContent> = Object.fromEntries(
  imageSeoSpecs.map((spec) => [spec.slug, contentFor(spec, "ru")]),
);

export const learnImageSeoZh: Record<string, LocalizedContent> = Object.fromEntries(
  imageSeoSpecs.map((spec) => [spec.slug, contentFor(spec, "zh")]),
);

export const learnImageSeoKo: Record<string, LocalizedContent> = Object.fromEntries(
  imageSeoSpecs.map((spec) => [spec.slug, contentFor(spec, "ko")]),
);
