import Link from "next/link";
import { notFound } from "next/navigation";
import { loadUsageByViewToken } from "@/lib/keys";
import { formatUsd } from "@/lib/money";

export const dynamic = "force-dynamic";
export const metadata = { title: "Расход ключа — OpenKeys" };

export default async function UsagePage({ params }: { params: Promise<{ token: string }> }) {
  const { token } = await params;
  const usage = await loadUsageByViewToken(token);
  if (!usage) notFound();

  const face = usage.faceValueNano;
  const remaining = usage.officialRemainingNano;
  const usedPercent = face > 0n ? Number(((face - remaining) * 100n) / face) : 0;

  return (
    <main>
      <h1>Баланс ключа</h1>
      <p className="muted">
        Ключ: <code>{usage.keyMasked}</code>
      </p>

      <div className="card">
        <div className="bar">
          <span style={{ width: `${Math.min(100, Math.max(0, 100 - usedPercent))}%` }} />
        </div>
        <div className="row">
          <span className="muted">Осталось</span>
          <span className="big">{formatUsd(remaining)}</span>
        </div>
        <div className="row">
          <span className="muted">Потрачено</span>
          <span>{formatUsd(usage.officialSpentNano)}</span>
        </div>
        <div className="row">
          <span className="muted">Номинал ключа</span>
          <span>{formatUsd(face, 0)}</span>
        </div>
        <div className="row">
          <span className="muted">Статус</span>
          <span style={{ color: usage.status === "active" ? "var(--ok)" : "var(--warn)" }}>
            {usage.status === "active" ? "активен" : "отключён"}
          </span>
        </div>
        <div className="row">
          <span className="muted">Выпущен</span>
          <span>{usage.createdAt.toISOString().slice(0, 10)}</span>
        </div>
      </div>

      <p className="muted">
        Все суммы — в долларах официального прайса Anthropic: столько же вы заплатили бы за эти запросы на
        api.anthropic.com. Как подключиться — <Link href="/docs">инструкция</Link>.
      </p>
    </main>
  );
}
