export const metadata = { title: "Подключение — OpenKeys" };

export default function DocsPage() {
  return (
    <main>
      <h1>Подключение за две строки</h1>
      <p className="muted">
        Наш сервер повторяет протокол Anthropic один в один, поэтому в коде меняется только адрес и ключ.
      </p>

      <div className="card">
        <div className="row">
          <span className="muted">Base URL</span>
          <code>https://api.apitoken.sale</code>
        </div>
        <div className="row">
          <span className="muted">Ключ</span>
          <code>sk-pool-…</code>
        </div>
      </div>

      <h2>Claude Code, Cursor, Cline, Roo Code</h2>
      <p>Всё это использует официальный SDK Anthropic, поэтому достаточно двух переменных окружения:</p>
      <pre>
        <code>{`export ANTHROPIC_API_KEY=sk-pool-ваш-ключ
export ANTHROPIC_BASE_URL=https://api.apitoken.sale`}</code>
      </pre>
      <p className="muted">
        В инструментах с полем «Base URL» в настройках впишите тот же адрес, а ключ — в поле Anthropic API Key.
      </p>

      <h2>curl</h2>
      <pre>
        <code>{`curl https://api.apitoken.sale/v1/messages \\
  -H "x-api-key: $ANTHROPIC_API_KEY" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "content-type: application/json" \\
  -d '{"model":"claude-opus-5","max_tokens":1024,
       "messages":[{"role":"user","content":"привет"}]}'`}</code>
      </pre>

      <h2>Python</h2>
      <pre>
        <code>{`import anthropic

client = anthropic.Anthropic()   # берёт обе переменные из окружения
message = client.messages.create(
    model="claude-opus-5",
    max_tokens=1024,
    messages=[{"role": "user", "content": "привет"}],
)
print(message.content[0].text)`}</code>
      </pre>

      <h2>Какие модели доступны</h2>
      <p>Список отдаёт сам сервер — от Opus 5 и Sonnet 5 до старших версий линейки:</p>
      <pre>
        <code>{`curl https://api.apitoken.sale/v1/models \\
  -H "x-api-key: $ANTHROPIC_API_KEY" \\
  -H "anthropic-version: 2023-06-01"`}</code>
      </pre>
      <p className="muted">
        Важная деталь: заголовок <code>anthropic-version</code> обязателен, если ваш клиент авторизуется через
        <code> Authorization: Bearer</code>. Без него сервер считает запрос OpenAI-совместимым и отдаёт каталог
        GPT-моделей вместо Claude. На сами запросы к <code>/v1/messages</code> это не влияет.
      </p>

      <h2>Проверить остаток</h2>
      <pre>
        <code>{`curl https://api.apitoken.sale/balance -H "x-api-key: $ANTHROPIC_API_KEY"`}</code>
      </pre>
      <p className="muted">
        То же самое в человеческом виде — на персональной ссылке, которую вы получили вместе с ключом, или на
        странице «Мой расход».
      </p>

      <h2>Стриминг, инструменты, кэш</h2>
      <p>
        Работают без оговорок: <code>stream: true</code>, tool use, подсчёт токенов через{" "}
        <code>/v1/messages/count_tokens</code> и кэширование промптов через <code>cache_control</code>. Кэш
        тарифицируется по льготной ставке Anthropic, а не как обычный ввод, — на повторяющемся контексте это
        экономит примерно в десять раз.
      </p>
    </main>
  );
}
