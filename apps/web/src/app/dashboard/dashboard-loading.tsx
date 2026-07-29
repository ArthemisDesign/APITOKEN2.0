export function DashboardLoading({ label = "Loading your account…" }: { label?: string }) {
  return (
    <div className="app dashboard-loading dashboard-shell-loading" role="status" aria-live="polite">
      <p className="sr-only">{label}</p>
      <aside className="side dashboard-shell-side" aria-hidden="true">
        <span className="brand side-brand">apiToken.sale</span>
        <div className="dashboard-shell-nav">
          {[0, 1, 2, 3, 4, 5, 6].map((item) => <span className="skl dashboard-shell-nav-row" key={item} />)}
        </div>
        <span className="skl dashboard-shell-user" />
      </aside>
      <main className="app-main" aria-hidden="true">
        <header className="app-top">
          <div className="app-top-in">
            <span className="skl dashboard-shell-heading" />
            <span className="skl dashboard-shell-balance" />
          </div>
        </header>
        <div className="app-body-in">
          <section className="panel overview-panel">
            <div className="overview-primary-grid">
              <span className="skl dashboard-shell-primary" />
              <span className="skl dashboard-shell-primary" />
            </div>
            <div className="overview-metrics-grid">
              {[0, 1, 2].map((item) => <span className="skl dashboard-shell-metric" key={item} />)}
            </div>
            <span className="skl dashboard-shell-activity" />
          </section>
        </div>
      </main>
    </div>
  );
}
