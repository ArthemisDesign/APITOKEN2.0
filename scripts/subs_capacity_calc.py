#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
subs_capacity_calc.py — ЖИВОЙ калькулятор ёмкости продаж клиентских подписок.

Отвечает на три вопроса, всё в USD-ЭКВИВАЛЕНТЕ по трём окнам (5h/7d/30d):
  1. Сколько у нас ёмкости на флоте (сумма активных Claude-подписок нужного fleet)?
  2. Скольким ещё клиентам можно продать (с буфером «всегда свободные токены»)?
  3. Когда докупать новую Max20x-подписку (точка дозаказа)?

Фри-триал 7 дней ВРЕЗАН в математику: пока активен — резервирует слот наравне с
платным (на окнах 5h/7d квоты равны), поэтому считается в занятой ёмкости.

Источник — общая subscriptions.db:
  ёмкость  = Σ plan_capacity[sub.plan][window]   по subs где fleet=F и status=active
  занято   = Σ client_plans[cs.plan_id].q{window} по client_subs (active/trial),
             но только тарифы своего fleet (client_plans.fleet=F)
Буфер: держим (1-HEADROOM) ёмкости → остаток = гарантированные свободные токены.

compute() возвращает dict — его переиспользует subs_reorder_alert.py.

Запуск:
  python3 scripts/subs_capacity_calc.py                 # prod, человекочитаемо
  python3 scripts/subs_capacity_calc.py --fleet dev
  python3 scripts/subs_capacity_calc.py --add-paid 22 --add-trial 4   # what-if
  python3 scripts/subs_capacity_calc.py --json          # машинный вывод
  HEADROOM=0.1 REORDER=2 SUBS_DB=/path python3 scripts/subs_capacity_calc.py  # вернуть буфер
"""
import os, sys, json, sqlite3

WINDOWS = ["5h", "7d", "30d"]
PAID_PLAN  = "max20_30"
TRIAL_PLAN = "trial7"

def db_path():
    env = os.environ.get("SUBS_DB")
    if env: return env
    return os.path.expanduser("~/.config/personal-agents/subscriptions.db")

def argval(flag, default=0, cast=int):
    if flag in sys.argv:
        i = sys.argv.index(flag)
        if i + 1 < len(sys.argv):
            return cast(sys.argv[i + 1])
    return default

def compute(dbp=None, fleet="prod", headroom=0.0, reorder=2, add_paid=0, add_trial=0):
    """Считает ёмкость/занятость/сигнал по флоту. Возвращает dict (всё в USD)."""
    c = sqlite3.connect(dbp or db_path(), timeout=10)
    c.row_factory = sqlite3.Row

    # ── ёмкость: активные подписки нужного флота
    subs = c.execute("SELECT plan FROM subs WHERE fleet=? AND status='active'",
                     (fleet,)).fetchall()
    plan_counts = {}
    for s in subs:
        plan_counts[s["plan"]] = plan_counts.get(s["plan"], 0) + 1
    pc = {(r["plan"], r["window"]): r["usd"]
          for r in c.execute("SELECT plan,window,usd FROM plan_capacity")}
    capacity = {w: 0.0 for w in WINDOWS}
    for plan, n in plan_counts.items():
        for w in WINDOWS:
            capacity[w] += n * pc.get((plan, w), 0.0)

    # ── тарифы (только своего флота учитываем в занятости)
    plans = {r["plan_id"]: r for r in c.execute("SELECT * FROM client_plans")}
    def q(pid, w):    return plans[pid][f"q{w}_usd"]  if pid in plans else 0.0
    def photo(pid):   return plans[pid]["photo_usd"]  if pid in plans else 0.0
    def isfleet(pid): return (plans[pid]["fleet"] == fleet) if pid in plans else False
    def istrial(pid): return bool(plans[pid]["is_trial"]) if pid in plans else False

    committed = {w: 0.0 for w in WINDOWS}
    photo_committed = 0.0
    n_paid = n_trial = 0
    for cs in c.execute("SELECT plan_id,status FROM client_subs WHERE status IN ('active','trial')"):
        pid = cs["plan_id"]
        if not isfleet(pid):
            continue
        for w in WINDOWS: committed[w] += q(pid, w)
        photo_committed += photo(pid)
        if istrial(pid): n_trial += 1
        else:            n_paid += 1
    # what-if добавки
    n_paid += add_paid; n_trial += add_trial
    for w in WINDOWS:
        committed[w] += add_paid * q(PAID_PLAN, w) + add_trial * q(TRIAL_PLAN, w)
    photo_committed += add_paid * photo(PAID_PLAN) + add_trial * photo(TRIAL_PLAN)
    c.close()

    unit = {w: (plans[PAID_PLAN][f"q{w}_usd"] if PAID_PLAN in plans else 0.0) for w in WINDOWS}
    windows = []
    more_by_w = {}
    for w in WINDOWS:
        usable = capacity[w] * (1 - headroom)
        buf    = capacity[w] * headroom
        free   = usable - committed[w]
        util   = committed[w] / capacity[w] if capacity[w] else 0.0
        # +eps: точное кратное (358.20/11.94=30) в float даёт 29.9999 → int срежет
        more   = int(free / unit[w] + 1e-9) if unit[w] else 0
        more_by_w[w] = more
        windows.append({"w": w, "capacity": round(capacity[w], 2),
                        "committed": round(committed[w], 2), "buffer": round(buf, 2),
                        "free": round(free, 2), "util": round(util, 4), "more": more})

    binding = min(more_by_w, key=more_by_w.get) if more_by_w else "7d"
    can_sell = more_by_w.get(binding, 0)
    total_slots  = int(capacity["7d"] / unit["7d"] + 1e-9) if unit["7d"] else 0
    usable_slots = int(capacity["7d"] * (1 - headroom) / unit["7d"] + 1e-9) if unit["7d"] else 0

    # сколько подписок докупить, чтобы снова стало ≥ reorder свободных слотов
    n_now = len(subs); need = 0
    unit7 = unit["7d"]; cap_per = pc.get(("max20", "7d"), 0.0)
    if can_sell <= reorder and unit7 and cap_per:
        need = 1
        while need <= 100:
            cap7 = (n_now + need) * cap_per
            free_slots = int((cap7 * (1 - headroom) - committed["7d"]) / unit7 + 1e-9)
            if free_slots >= reorder:
                break
            need += 1

    signal = "green" if can_sell > reorder else ("yellow" if can_sell > 0 else "red")
    if signal == "green":
        recommend = f"Запас: до дозаказа ещё {can_sell-reorder}, потолок {can_sell}."
    elif signal == "yellow":
        recommend = f"Пора дозаказывать: свободно {can_sell} ≤ {reorder} — купи +{max(need,1)} Max20x заранее."
    else:
        recommend = (f"Ёмкость исчерпана — докупить +{max(need,1)} Max20x (auth_token_bot)."
                     if n_now else "Нет активных подписок этого флота — нужна Max20x.")

    return {
        "fleet": fleet, "headroom": headroom, "reorder": reorder,
        "n_subs": n_now, "plan_counts": plan_counts,
        "clients": {"paid": n_paid, "trial": n_trial, "total": n_paid + n_trial},
        "windows": windows,
        "slots": {"total": total_slots, "usable": usable_slots, "used": n_paid + n_trial},
        "can_sell": can_sell, "binding": binding,
        "photo_committed": round(photo_committed, 2),
        "need_subs": need, "signal": signal, "recommend": recommend,
    }

def print_human(r):
    dot = {"green": "🟢", "yellow": "🟡", "red": "🔴"}[r["signal"]]
    print(f"═══ ЁМКОСТЬ ПРОДАЖ (флот {r['fleet']}) ═══")
    print(f"Активных подписок: {r['n_subs']}  {r['plan_counts']}")
    print(f"Буфер: {r['headroom']:.0%} · точка дозаказа: свободно ≤ {r['reorder']}")
    cl = r["clients"]; print(f"Клиентов: платных {cl['paid']} + триалов {cl['trial']} = {cl['total']}")
    print()
    hdr = f"{'Окно':<5} {'ёмкость':>11} {'занято':>11} {'буфер':>10} {'свободно':>11} {'занято%':>8} {'ещё':>6}"
    print(hdr); print("─"*len(hdr))
    for w in r["windows"]:
        print(f"{w['w']:<5} ${w['capacity']:>10.2f} ${w['committed']:>10.2f} ${w['buffer']:>9.2f} "
              f"${w['free']:>10.2f} {w['util']*100:>7.1f}% {w['more']:>6}")
    print()
    s = r["slots"]
    print(f"СЛОТЫ (окно 7d): всего {s['total']}/подписку · с буфером {s['usable']} · занято {s['used']}")
    print(f"{dot} Можно продать ещё: {r['can_sell']} (узкое окно {r['binding']})")
    print(f"💸 Фото-API у текущих клиентов: ${r['photo_committed']:.2f}/период")
    print(f"{dot} {r['recommend']}")

def main():
    headroom = float(os.environ.get("HEADROOM", "0.0"))
    reorder  = int(os.environ.get("REORDER", "2"))
    fleet    = argval("--fleet", "prod", cast=str)
    r = compute(fleet=fleet, headroom=headroom, reorder=reorder,
                add_paid=argval("--add-paid", 0), add_trial=argval("--add-trial", 0))
    if "--json" in sys.argv:
        print(json.dumps(r, ensure_ascii=False, indent=2))
    else:
        print_human(r)

if __name__ == "__main__":
    main()
