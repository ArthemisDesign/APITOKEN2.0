import Link from "next/link";

export default function HomePage() {
  return (
    <main>
      <h1>Claude API по готовому ключу</h1>
      <p className="muted">
        Ключ выдаётся с номиналом в долларах <strong>официального прайса Anthropic</strong>: «ключ на $50» — это
        ровно столько же работы, сколько $50 на api.anthropic.com. Никакой внутренней валюты и пересчёта в
        «миллиарды токенов».
      </p>

      <div className="card">
        <h2 style={{ marginTop: 0 }}>Как это работает</h2>
        <p>
          Вы покупаете ключ, вставляете его в свой инструмент и работаете. Регистрация, почта и карта не нужны —
          ключ самодостаточен, а остаток виден по личной ссылке, которая приходит вместе с ним.
        </p>
        <pre>
          <code>{`export ANTHROPIC_API_KEY=sk-pool-…
export ANTHROPIC_BASE_URL=https://api.apitoken.sale`}</code>
        </pre>
        <p className="muted">
          Работает без изменений в коде: Claude Code, Cursor, Cline, Roo Code, официальный SDK Anthropic и обычный
          curl. Подробности — в <Link href="/docs">инструкции по подключению</Link>.
        </p>
      </div>

      <div className="card">
        <h2 style={{ marginTop: 0 }}>Что важно знать про расход</h2>
        <p>
          Списывается ровно то, что стоит запрос по прайсу Anthropic: вход, выход и кэш считаются отдельно и по
          своим ставкам. Повторный контекст, который попал в кэш, стоит примерно в десять раз дешевле обычного
          ввода — на длинных сессиях с агентом это основная часть экономии.
        </p>
        <p className="muted">
          Остаток и потраченное всегда можно посмотреть по своей ссылке или на странице{" "}
          <Link href="/usage">«Мой расход»</Link>, введя туда сам ключ.
        </p>
      </div>
    </main>
  );
}
