const NAV = [
  { label: "Дашборд", active: true },
  { label: "Лиды", soon: true },
  { label: "Контакты", soon: true },
  { label: "Парсинг", soon: true },
];

const STATS = [
  { label: "Лиды", value: "—" },
  { label: "В работе", value: "—" },
  { label: "Закрыто", value: "—" },
  { label: "Парсинг за 24ч", value: "—" },
];

export default function Home() {
  return (
    <div className="cab">
      <aside className="cab-sidebar">
        <span className="brand">
          <span>
            apitoken<em>.crm</em>
          </span>
        </span>
        <nav className="cab-nav">
          {NAV.map((item) => (
            <span key={item.label} className={`cab-link${item.active ? " active" : ""}`}>
              {item.label}
              {item.soon && <span className="soon-pill">soon</span>}
            </span>
          ))}
        </nav>
        <div className="cab-side-foot">внутренний инструмент · только для команды</div>
      </aside>

      <div className="cab-main">
        <header className="cab-topbar">
          <span className="page-title" style={{ marginBottom: 0 }}>
            CRM &amp; Parsing
          </span>
          <span className="badge">crm.panel.apitoken.sale</span>
        </header>

        <main className="cab-content">
          <h1 className="page-title">Дашборд</h1>
          <p className="page-sub">Каркас CRM — разделы и данные подключаются по мере постановки задач.</p>

          <div className="stat-grid">
            {STATS.map((s) => (
              <div key={s.label} className="stat-card">
                <div className="stat-label">{s.label}</div>
                <div className="stat-value">{s.value}</div>
              </div>
            ))}
          </div>

          <div className="card">
            <div className="card-title">Пока пусто</div>
            <div className="card-sub">
              Это подготовленный каркас в дизайне панелей apitoken.sale. Наполнение (лиды, парсинг,
              воронка) появится после постановки требований.
            </div>
          </div>
        </main>
      </div>
    </div>
  );
}
