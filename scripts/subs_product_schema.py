#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
subs_product_schema.py — схема и сид ПРОДУКТА клиентских подписок.

Живёт в ОБЩЕЙ subscriptions.db (тот же источник истины, что и пул Claude-подписок:
кросс-инстансно, одна на весь флот). Добавляет три таблицы — они ИНЕРТНЫ для текущего
движка (он читает только таблицу `subs`), поэтому создание безопасно и идемпотентно:

  plan_capacity — SUPPLY. Ёмкость ОДНОЙ Claude-подписки плана (pro/max5/max20) по окну
                  (5h/7d/30d) в USD-ЭКВИВАЛЕНТЕ (наш usd_cost). «Сколько у нас есть».
  client_plans  — PRODUCT. Тарифы для клиента: право = USD-квоты по окнам + правило
                  периода (календарный месяц / N дней) + флот обслуживания.
  client_subs   — ASSIGNMENT. Подписка конкретного клиента (chat_id → тариф, границы
                  периода, статус). Календарная граница 30d = то же число след. месяца.

ВСЁ В USD-ЭКВИВАЛЕНТЕ (не в токенах) — по решению владельца.

Калибровка supply (План §2, «util приходит ПРОЦЕНТОМ, не токенами»):
  Замер на плане Pro (2 сессии в одном 5-часовом окне, оба скрина, сброс 4:50am):
    5h = $12.02 + $5.89 = $17.91 (окно добито до 100%)
    7d = те же $17.91 = 11% недели → $17.91/0.11 = $162.82
    30d(кал.месяц ≈ 30/7 нед) = $162.82 × 4.2857 = $697.79
  Max 20x = ×20 (номинальный множитель Anthropic «20× Pro»):
    5h $358.20 · 7d $3256.36 · 30d $13955.84
  → всё derived, уточняется живой калибровкой subs_poll_log × куб.

Клиентский тариф «Max20x/30» = 1/30 подписки Max20x по КАЖДОМУ окну ⇒ 30 клиентов
ровно заполняют одну Max20x на всех окнах сразу; держим < 30 → всегда свободные токены.

ОТДЕЛЬНО: фото-генерация. К подписке прилагается бюджет photo_usd (по умолчанию $2
на клиента за период) — это ВНЕШНИЙ платный API картинок (оплачиваем мы напрямую,
НЕ из пула Claude-подписок). Учитывается и резервируется отдельной строкой.

Запуск:  python3 scripts/subs_product_schema.py [путь_к_subscriptions.db]
         SUBS_DB=/path/... python3 scripts/subs_product_schema.py
Идемпотентно: повторный запуск не плодит дублей (UPSERT по ключам).
"""
import os, sys, time, sqlite3

def db_path():
    if len(sys.argv) > 1 and sys.argv[1]:
        return sys.argv[1]
    env = os.environ.get("SUBS_DB")
    if env:
        return env
    return os.path.expanduser("~/.config/personal-agents/subscriptions.db")

# ── SUPPLY: ёмкость плана по окну (USD на ОДНУ подписку) ────────────────────────
# (plan, window, usd, source, note)
CAPACITY = [
    ("pro",   "5h",  17.91,   "measured", "2 сессии в одном 5ч-окне до 100% ($12.02+$5.89)"),
    ("pro",   "7d",  162.82,  "derived",  "$17.91 = 11% недели → /0.11"),
    ("pro",   "30d", 697.79,  "derived",  "7d × 30/7 (календарный месяц)"),
    ("max20", "5h",  358.20,  "derived",  "pro×20 (номинал Anthropic 20×)"),
    ("max20", "7d",  3256.36, "derived",  "pro×20"),
    ("max20", "30d", 13955.84,"derived",  "pro×20 (7d × 30/7)"),
]

# ── PRODUCT: тарифы клиента (USD-квоты по окнам + фото-API) ────────────────────
# (plan_id, title, q5h, q7d, q30d, photo_usd, period, is_trial, fleet, active, note)
PLANS = [
    # q7d=108.54 (истинная 1/30 = 3256.36/30 = 108.545, округляем ВНИЗ → ровно 30
    # клиентов влезают в подписку, клиент не получает больше обещанного):
    ("max20_30", "Клиент Max20x/30", 11.94, 108.54, 465.19, 2.00,
     "calendar_month", 0, "prod", 1,
     "1/30 подписки Max20x по каждому окну; 30 клиентов = 1 подписка; +$2 фото-API"),
    # Фри-триал 7 дней — врезан в математику (занимает полный слот на 5h/7d, пока активен):
    ("trial7", "Фри-триал 7 дней", 11.94, 108.54, 108.54, 0.50,
     "days:7", 1, "prod", 1,
     "Бесплатно 7 дней = неделя платного тарифа, одноразово; резервирует слот пока активен"),
]

def main():
    p = db_path()
    now = int(time.time())
    c = sqlite3.connect(p, timeout=10)
    c.execute("PRAGMA journal_mode=WAL")

    c.execute("""CREATE TABLE IF NOT EXISTS plan_capacity(
        plan TEXT NOT NULL, window TEXT NOT NULL, usd REAL NOT NULL,
        source TEXT NOT NULL DEFAULT 'derived', note TEXT NOT NULL DEFAULT '',
        ts INTEGER NOT NULL, PRIMARY KEY(plan, window))""")
    c.execute("""CREATE TABLE IF NOT EXISTS client_plans(
        plan_id TEXT PRIMARY KEY, title TEXT NOT NULL,
        q5h_usd REAL NOT NULL, q7d_usd REAL NOT NULL, q30d_usd REAL NOT NULL,
        photo_usd REAL NOT NULL DEFAULT 0,
        period TEXT NOT NULL DEFAULT 'calendar_month',
        is_trial INTEGER NOT NULL DEFAULT 0, fleet TEXT NOT NULL DEFAULT 'prod',
        active INTEGER NOT NULL DEFAULT 1, note TEXT NOT NULL DEFAULT '',
        ts INTEGER NOT NULL)""")
    # мягкая миграция: добить колонку photo_usd в уже существующую таблицу
    try:
        c.execute("ALTER TABLE client_plans ADD COLUMN photo_usd REAL NOT NULL DEFAULT 0")
    except sqlite3.OperationalError:
        pass
    c.execute("""CREATE TABLE IF NOT EXISTS client_subs(
        chat_id TEXT PRIMARY KEY, plan_id TEXT NOT NULL,
        started_ts INTEGER NOT NULL, period_end_ts INTEGER NOT NULL,
        status TEXT NOT NULL DEFAULT 'active', note TEXT NOT NULL DEFAULT '',
        ts INTEGER NOT NULL)""")

    for plan, win, usd, src, note in CAPACITY:
        c.execute("""INSERT INTO plan_capacity(plan,window,usd,source,note,ts)
            VALUES(?,?,?,?,?,?)
            ON CONFLICT(plan,window) DO UPDATE SET
              usd=excluded.usd, source=excluded.source, note=excluded.note, ts=excluded.ts""",
            (plan, win, usd, src, note, now))

    for pid, title, q5, q7, q30, photo, period, trial, fleet, active, note in PLANS:
        c.execute("""INSERT INTO client_plans(plan_id,title,q5h_usd,q7d_usd,q30d_usd,
              photo_usd,period,is_trial,fleet,active,note,ts)
            VALUES(?,?,?,?,?,?,?,?,?,?,?,?)
            ON CONFLICT(plan_id) DO UPDATE SET
              title=excluded.title, q5h_usd=excluded.q5h_usd, q7d_usd=excluded.q7d_usd,
              q30d_usd=excluded.q30d_usd, photo_usd=excluded.photo_usd, period=excluded.period,
              is_trial=excluded.is_trial, fleet=excluded.fleet, active=excluded.active,
              note=excluded.note, ts=excluded.ts""",
            (pid, title, q5, q7, q30, photo, period, trial, fleet, active, note, now))

    c.commit()

    print(f"БД: {p}")
    print("── plan_capacity (SUPPLY, USD на 1 подписку) ──")
    for r in c.execute("SELECT plan,window,usd,source FROM plan_capacity ORDER BY plan,window"):
        print(f"  {r[0]:<6} {r[1]:<4} ${r[2]:>10.2f}  [{r[3]}]")
    print("── client_plans (PRODUCT, USD-квоты) ──")
    for r in c.execute("""SELECT plan_id,title,q5h_usd,q7d_usd,q30d_usd,photo_usd,period,is_trial,active
                          FROM client_plans ORDER BY is_trial,plan_id"""):
        flag = "ТРИАЛ" if r[7] else "платный"
        st = "ON" if r[8] else "off"
        print(f"  {r[0]:<10} {flag:<7} [{st}] 5h ${r[2]:.2f} · 7d ${r[3]:.2f} · 30d ${r[4]:.2f} · фото ${r[5]:.2f} · {r[6]}")
    print("── client_subs (ASSIGNMENT) ──")
    n = c.execute("SELECT COUNT(*) FROM client_subs").fetchone()[0]
    print(f"  строк: {n}")
    c.close()

if __name__ == "__main__":
    main()
