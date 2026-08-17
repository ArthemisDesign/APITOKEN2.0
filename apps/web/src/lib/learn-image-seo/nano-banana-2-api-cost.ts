import type { ImageSeoSpec } from "./shared";
import { faq, list, note, paragraph, section, sharedCode, steps, table, tr, ROUTER } from "./shared";

export const spec: ImageSeoSpec = {
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
  };
