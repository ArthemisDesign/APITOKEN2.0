"use client";

import { useState } from "react";
import { HighlightedCode } from "./highlighted-code";
import { Prose } from "./prose";
import {
  GEMINI_BATCH_CREATE_CURL,
  GEMINI_BATCH_DIRECT_BASE,
  GEMINI_BATCH_ENDPOINTS,
  GEMINI_BATCH_JSONL,
  GEMINI_BATCH_MANAGE_CURL,
  GEMINI_BATCH_PARSE_TS,
  GEMINI_BATCH_POLL_CURL,
  GEMINI_BATCH_ROUTER_BASE,
  GEMINI_BATCH_UPLOAD_CURL,
  type DocsLanguage,
} from "./gemini-batch-guide-data";

const tr = (language: DocsLanguage, en: string, ru: string) => language === "ru" ? ru : en;

const endpointPurpose = {
  create: ["Create an asynchronous operation", "Создать асинхронную operation"],
  poll: ["Poll state and read item results", "Проверить состояние и получить результаты"],
  list: ["List this account’s operations", "Список операций этого аккаунта"],
  cancel: ["Request cancellation; already dispatched items may finish", "Запросить отмену; отправленные items могут завершиться"],
  delete: ["Delete a terminal operation", "Удалить завершённую операцию"],
  upload: ["Start or continue a resumable JSONL upload", "Начать или продолжить resumable-загрузку JSONL"],
  "files-list": ["List account-scoped gateway files", "Список account-scoped файлов шлюза"],
  "file-get": ["Read file metadata", "Получить метаданные файла"],
  "file-download": ["Stream or range-download an active file", "Потоково или диапазоном скачать активный файл"],
  "file-delete": ["Delete a file not referenced by live work", "Удалить файл без активных ссылок"],
} as const;

const limits = [
  ["Inline create body", "20 MiB", "Тело inline create", "20 МиБ"],
  ["Items per Batch", "100,000", "Items в одном Batch", "100 000"],
  ["Nonterminal Batches per account", "100", "Незавершённых Batches на аккаунт", "100"],
  ["JSONL line", "20 MiB", "Одна строка JSONL", "20 МиБ"],
  ["Upload chunk", "8 MiB", "Один upload chunk", "8 МиБ"],
  ["Uploaded file TTL", "48 hours", "TTL загруженного файла", "48 часов"],
  ["Queue deadline", "48 hours", "Максимальное время в очереди", "48 часов"],
  ["Terminal result retention", "42 days", "Хранение terminal result", "42 дня"],
  ["List page", "1–1,000", "Размер страницы list", "1–1 000"],
] as const;

const differences = [
  ["Execution", "Asynchronous non-streaming gateway jobs; not Vertex AI Batch, GCS or BigQuery.", "Асинхронные непотоковые задания шлюза; это не Vertex AI Batch, GCS или BigQuery."],
  ["Pricing", "Normal Gemini tariff and your account discount. No separate Batch discount or completion SLA.", "Обычный тариф Gemini и скидка аккаунта. Отдельной Batch-скидки и гарантии срока нет."],
  ["Inline schema", "Official InlinedRequests wrapping (requests.requests[]) and a raw array are both accepted. Proto-JSON snake_case aliases are accepted.", "Принимаются официальная обёртка InlinedRequests (requests.requests[]) и сырой массив. Алиасы proto-JSON snake_case принимаются."],
  ["Files", "Files belong to your apiToken.sale account, not to a Google Cloud project. Foreign Google files are invisible.", "Файлы принадлежат аккаунту apiToken.sale, а не Google Cloud project. Чужие Google-файлы недоступны."],
  ["fileData", "Gateway files cannot currently be embedded with fileData. Use JSONL file input, or inlineData for synchronous generation.", "Сейчас gateway-файлы нельзя встраивать через fileData. Используйте JSONL file input, а для синхронной генерации — inlineData."],
  ["File output", "File-input jobs publish an ordered encrypted JSONL responsesFile before done=true; inline jobs retain inlinedResponses.", "Задания с файловым вводом публикуют упорядоченный зашифрованный JSONL responsesFile до done=true; inline jobs сохраняют inlinedResponses."],
  ["Timestamps", "Operation timestamps are RFC 3339 UTC (google-datetime).", "Времена operation — RFC 3339 UTC (google-datetime)."],
  ["Unsupported", "No webhooks, embedding/update Batch methods, image-output models, Google IAM or stock Vertex resource semantics.", "Нет webhooks, embedding/update Batch, image-output моделей, Google IAM и Vertex resource semantics."],
] as const;

const errors = [
  ["400 INVALID_ARGUMENT", "Fix malformed JSON, both/neither input forms, invalid JSONL, unsupported model or field. Do not retry unchanged.", "Исправьте JSON, выбор input form, JSONL, модель или поле. Не повторяйте неизменённый запрос."],
  ["401 UNAUTHENTICATED", "Send an active sk-pool key in x-goog-api-key.", "Передайте активный sk-pool ключ в x-goog-api-key."],
  ["402 RESOURCE_EXHAUSTED", "The available balance cannot cover the whole Batch hold. Top up or reduce the workload.", "Доступного баланса не хватает на hold всего Batch. Пополните баланс или уменьшите задание."],
  ["404 NOT_FOUND", "Wrong, deleted, expired or foreign-account job/file; verify the full resource name and account.", "Неверный, удалённый, истёкший или чужой job/file; проверьте полное имя и аккаунт."],
  ["409 ABORTED", "Idempotency key reused with changed content, or upload offset mismatch.", "Idempotency key использован с другим телом либо не совпал upload offset."],
  ["FAILED_PRECONDITION", "Wait/cancel before deleting a live Batch; a live Batch reference also blocks file deletion.", "Дождитесь/отмените Batch перед удалением; активная ссылка Batch также блокирует удаление файла."],
  ["429 / 503", "Temporary quota, capacity or authority condition. Poll/retry with capped exponential backoff and jitter.", "Временный лимит мощности, квоты или authority. Используйте ограниченный exponential backoff с jitter."],
] as const;

export function GeminiBatchGuide({ language }: { language: DocsLanguage }) {
  return <>
    <div className="docs-notice"><Prose text={tr(language,
      `Use the recommended \`${GEMINI_BATCH_ROUTER_BASE}\` or the direct \`${GEMINI_BATCH_DIRECT_BASE}\`. Only the hostname changes: paths, bodies and \`x-goog-api-key\` authentication are identical. Keep the key server-side.`,
      `Используйте рекомендуемый \`${GEMINI_BATCH_ROUTER_BASE}\` или прямой \`${GEMINI_BATCH_DIRECT_BASE}\`. Меняется только hostname: пути, тела и авторизация \`x-goog-api-key\` одинаковы. Храните ключ только на сервере.`)} /></div>

    <h3>{tr(language, "Endpoints", "Endpoints")}</h3>
    <div className="table-scroll"><table className="mtable"><thead><tr>
      <th>{tr(language, "Method", "Метод")}</th><th>{tr(language, "Path", "Путь")}</th><th>{tr(language, "Purpose", "Назначение")}</th>
    </tr></thead><tbody>{GEMINI_BATCH_ENDPOINTS.map(([method, path, purpose]) => <tr key={`${method}-${path}`}>
      <td><code>{method}</code></td><td><code>{path}</code></td><td><span>{tr(language, endpointPurpose[purpose][0], endpointPurpose[purpose][1])}</span></td>
    </tr>)}</tbody></table></div>

    <div className="docs-cache-stack batch-guide-stack">
      <BatchCodeCard language={language} title={tr(language, "1. Create an inline Batch", "1. Создайте inline Batch")} text={tr(language,
        "Save the returned name (batches/batch-…). Idempotency-Key is optional but recommended: an exact replay returns the same operation; the same key with different content returns 409. Without it, every POST creates a new job.",
        "Сохраните возвращённое name (batches/batch-…). Idempotency-Key необязателен, но рекомендуется: точный replay вернёт ту же operation, а другое тело с тем же ключом даст 409. Без заголовка каждый POST создаёт новое задание.")} code={GEMINI_BATCH_CREATE_CURL} label="Bash · curl" />
      <BatchCodeCard language={language} title={tr(language, "2. Poll and read every item", "2. Опрашивайте и читайте каждый item")} text={tr(language,
        "Poll with a delay until done is true. Then inspect the job state, string-valued batchStats, and every inlinedResponses entry. Each entry contains either response or error; a terminal job may include item-level failures.",
        "Опрашивайте с задержкой до done=true. Затем проверьте state, строковые batchStats и каждый элемент inlinedResponses. В элементе есть либо response, либо error; terminal job может содержать ошибки отдельных items.")} code={GEMINI_BATCH_POLL_CURL} label="Bash · poll" />
      <BatchCodeCard language={language} title={tr(language, "3. Parse the terminal operation", "3. Разберите terminal operation")} text={tr(language,
        "Treat counters as decimal strings and parse them with BigInt. Do not treat done=true alone as proof that every item succeeded.",
        "Счётчики приходят десятичными строками — разбирайте их через BigInt. Само по себе done=true не означает успех каждого item.")} code={GEMINI_BATCH_PARSE_TS} label="TypeScript" />
      <BatchCodeCard language={language} title={tr(language, "4. List, cancel and delete resources", "4. List, cancel и delete ресурсов")} text={tr(language,
        "Use the complete batches/{id} name returned by create. Cancellation is best effort for already dispatched items; delete a Batch only after terminal state. Files are account-scoped and cannot be deleted while referenced by live work.",
        "Используйте полное имя batches/{id} из create. Для уже отправленных items отмена работает best effort; удаляйте Batch только после terminal state. Файлы account-scoped и не удаляются, пока на них ссылается активное задание.")} code={GEMINI_BATCH_MANAGE_CURL} label="Bash · lifecycle" />
    </div>

    <h3>{tr(language, "Large input with JSONL", "Большой ввод через JSONL")}</h3>
    <p className="docs-note"><Prose text={tr(language,
      "A nonempty line must be one JSON object with a unique correlation key and a request object. Do not wrap lines in an array. Blank lines and CRLF are accepted. Upload the file to this gateway, then pass the returned files/{id} as inputConfig.fileName.",
      "Каждая непустая строка — отдельный JSON object с correlation key и объектом request. Не оборачивайте строки в массив. Пустые строки и CRLF допустимы. Загрузите файл в этот шлюз и передайте возвращённое files/{id} в inputConfig.fileName.")} /></p>
    <div className="docs-cache-stack batch-guide-stack">
      <BatchCodeCard language={language} title={tr(language, "JSONL input format", "Формат входного JSONL")} text={tr(language,
        "key is required for file input and may be up to 512 bytes. Results remain in input order; keep the key in your own input-to-output mapping.",
        "Для file input поле key обязательно и может занимать до 512 байт. Результаты сохраняют порядок ввода; храните key в своей карте input→output.")} code={GEMINI_BATCH_JSONL} label="JSONL" />
      <BatchCodeCard language={language} title={tr(language, "Resumable upload and fileName create", "Resumable upload и create через fileName")} text={tr(language,
        "The returned upload URL is relative. Non-final upload chunks are exactly 8 MiB and use exact offsets; after an ambiguous response issue command=query before retrying. Full output downloads stream without buffering, and Range supports resumable bounded reads.",
        "Возвращаемый upload URL относительный. Нефинальные chunks имеют ровно 8 МиБ и точные offsets; после неоднозначного ответа выполните command=query перед повтором. Полный output скачивается потоком, а Range даёт возобновляемые ограниченные чтения.")} code={GEMINI_BATCH_UPLOAD_CURL} label="Bash · upload" />
    </div>

    <h3>{tr(language, "Limits and retention", "Лимиты и хранение")}</h3>
    <div className="table-scroll"><table className="mtable"><thead><tr><th>{tr(language, "Limit", "Лимит")}</th><th>{tr(language, "Value", "Значение")}</th></tr></thead><tbody>
      {limits.map(([enName, enValue, ruName, ruValue]) => <tr key={enName}><td><span>{tr(language, enName, ruName)}</span></td><td><code>{tr(language, enValue, ruValue)}</code></td></tr>)}
    </tbody></table></div>
    <p className="docs-note"><Prose text={tr(language,
      "The account must have enough available balance for the complete conservative hold at create time. Accepted work is durable and may remain queued during temporary capacity pressure. Ultra subscriptions use up to 20 durable Batch slots; every other plan uses 2, with a random durable 2–5 second interval between starts on one subscription. Standard Gemini token pricing and your normal account discount apply; there is no separate Batch discount or completion-time SLA.",
      "При создании на аккаунте должен быть доступен баланс для conservative hold всего Batch. Принятое задание сохраняется и может оставаться в очереди при временном дефиците мощности. Ultra использует до 20 durable Batch slots, остальные планы — 2; старты на одной подписке разделены случайным durable интервалом 2–5 секунд. Действуют обычные цены Gemini и скидка аккаунта; отдельной Batch-скидки и гарантии срока нет.")} /></p>

    <h3>{tr(language, "How this differs from Google Batch", "Отличия от официального Google Batch")}</h3>
    <div className="table-scroll"><table className="mtable"><thead><tr><th>{tr(language, "Area", "Область")}</th><th>{tr(language, "apiToken.sale behavior", "Поведение apiToken.sale")}</th></tr></thead><tbody>
      {differences.map(([area, en, ru]) => <tr key={area}><td><strong>{area}</strong></td><td><span>{tr(language, en, ru)}</span></td></tr>)}
    </tbody></table></div>

    <h3>{tr(language, "Batch troubleshooting", "Ошибки Batch")}</h3>
    <div className="table-scroll"><table className="mtable docs-errors"><thead><tr><th>{tr(language, "Status", "Статус")}</th><th>{tr(language, "What to do", "Что делать")}</th></tr></thead><tbody>
      {errors.map(([status, en, ru]) => <tr key={status}><td><code>{status}</code></td><td><span>{tr(language, en, ru)}</span></td></tr>)}
    </tbody></table></div>
  </>;
}

function BatchCodeCard({ language, title, text, code, label }: { language: DocsLanguage; title: string; text: string; code: string; label: string }) {
  const [copied, setCopied] = useState(false);
  async function copy() {
    await navigator.clipboard.writeText(code);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1_200);
  }
  return <article className="cache-card ym-hide-content"><header><h3>{title}</h3><p><Prose text={text} /></p></header><div className="ib-code"><div className="ib-code-bar"><i className="ib-dots" aria-hidden="true" /><span>{label}</span><button type="button" onClick={copy}>{copied ? tr(language, "Copied", "Скопировано") : tr(language, "Copy", "Копировать")}</button></div><pre><code><HighlightedCode code={code} /></code></pre></div></article>;
}
