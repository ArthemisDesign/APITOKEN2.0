import Link from "next/link";
import { SiteHeader } from "@/components/chrome";

export default function HomePage() {
  return (
    <>
      <SiteHeader />
      <main id="main-content">
        <section className="wrap openkeys-hero">
          <div className="page-heading">
            <span className="eyebrow">Claude API</span>
            <h1 className="p-h1">Готовый ключ. Без регистрации.</h1>
            <p className="p-sub">
              Номинал ключа — в долларах <b>официального прайса Anthropic</b>. «Ключ на $50» даёт ровно столько же
              работы, сколько $50 на api.anthropic.com. Никакой внутренней валюты и «миллиардов токенов», курс которых
              нельзя проверить.
            </p>
          </div>

          <div className="overview-primary-grid">
            <article className="card overview-balance-card">
              <div className="overview-card-head">
                <span className="overview-card-label">Подключение</span>
                <span className="chip">две строки</span>
              </div>
              <pre className="openkeys-pre">
                <code>{`export ANTHROPIC_BASE_URL="https://api.apitoken.sale"
export ANTHROPIC_API_KEY="sk-pool-ваш-ключ"

claude`}</code>
              </pre>
              <p className="overview-balance-rate">
                Работает без правок в коде: Claude Code, Cursor, Cline, Roo Code, официальный SDK и обычный curl.
              </p>
              <div className="overview-card-actions">
                <Link className="btn btn-primary btn-sm" href="/docs">
                  Документация
                </Link>
                <Link className="btn btn-ghost btn-sm" href="/usage">
                  Проверить остаток
                </Link>
              </div>
            </article>

            <article className="card overview-access-card">
              <div className="overview-card-head">
                <span className="overview-card-label">Что видно по ключу</span>
              </div>
              <ul className="openkeys-list">
                <li>Остаток и расход в долларах прайса Anthropic</li>
                <li>Разбивка по моделям и по типам токенов</li>
                <li>Вход, выход и кэш — каждый по своей ставке</li>
                <li>График расхода по дням за 30 дней</li>
              </ul>
              <p className="overview-balance-rate">
                Персональная ссылка приходит вместе с ключом, авторизация не нужна.
              </p>
            </article>
          </div>
        </section>
      </main>
    </>
  );
}
