import type { ImageSeoSpec } from "./shared";
import { faq, list, note, paragraph, section, steps, table, tr } from "./shared";

export const spec: ImageSeoSpec = {
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
  };
