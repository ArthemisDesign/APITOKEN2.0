#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
subs_reorder_alert.py — оповещение о ДОЗАКАЗЕ подписки в бот покупки токенов.

Когда ёмкость продаж клиентских подписок на prod-флоте подходит к концу
(сигнал 🟡/🔴 из subs_capacity_calc.compute), шлём сообщение админам ЧЕРЕЗ ТОКЕН
того же бота, что покупает токены (auth_token_bot) — для оператора это выглядит
как алерт от бота покупки, но код самого бота (внешний репо) НЕ трогаем.

Дедуп: состояние в reorder_alert.json (последний сигнал + время). Повторно шлём
только при СМЕНЕ сигнала или по истечении кулдауна (деф. 12ч). При возврате в 🟢
после тревоги — одно сообщение «ёмкость восстановлена» и сброс.

Конфиг (env, обычно из /etc/agents/auth_bot.env — путь в AUTH_BOT_ENV):
  AUTH_BOT_TOKEN   — токен бота (обязателен для реальной отправки)
  AUTH_BOT_ADMIN   — получатели: список через запятую; шлём ТОЛЬКО числовым id
                     (@username через sendMessage недоступен — пропускаем с ворнингом)
  SUBS_DB          — путь к subscriptions.db (деф. ~/.config/personal-agents/…)
  HEADROOM, REORDER, ALERT_COOLDOWN (сек, деф. 43200)

Запуск:
  python3 scripts/subs_reorder_alert.py            # проверить и, если надо, оповестить
  python3 scripts/subs_reorder_alert.py --dry      # только показать вердикт, не слать
  AUTH_BOT_ENV=/etc/agents/auth_bot.env python3 scripts/subs_reorder_alert.py
"""
import os, sys, json, time, urllib.request, urllib.parse

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from subs_capacity_calc import compute, db_path  # noqa: E402

DRY = "--dry" in sys.argv

def load_env_file(path):
    """Подмешать KEY=VALUE из env-файла в os.environ (не перетирая уже заданное)."""
    if not path or not os.path.exists(path):
        return
    for line in open(path, encoding="utf-8", errors="ignore"):
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        k, v = line.split("=", 1)
        k = k.strip(); v = v.strip().strip('"').strip("'")
        os.environ.setdefault(k, v)

def state_path():
    return os.path.join(os.path.dirname(db_path()), "reorder_alert.json")

def load_state():
    try:
        return json.load(open(state_path(), encoding="utf-8"))
    except Exception:
        return {"signal": "green", "ts": 0}

def save_state(sig):
    try:
        tmp = state_path() + ".tmp"
        json.dump({"signal": sig, "ts": int(time.time())}, open(tmp, "w", encoding="utf-8"))
        os.replace(tmp, state_path())
    except Exception as e:
        print(f"warn: не смог сохранить состояние: {e}", file=sys.stderr)

def admin_chat_ids():
    raw = os.environ.get("AUTH_BOT_ADMIN", "")
    ids, skipped = [], []
    for tok in raw.replace(";", ",").split(","):
        tok = tok.strip()
        if not tok:
            continue
        t = tok.lstrip("-")
        if t.isdigit():
            ids.append(tok)
        else:
            skipped.append(tok)
    if skipped:
        print(f"warn: пропускаю не-числовые AUTH_BOT_ADMIN (sendMessage только по id): {skipped}",
              file=sys.stderr)
    return ids

def tg_send(token, chat_id, text):
    url = f"https://api.telegram.org/bot{token}/sendMessage"
    data = urllib.parse.urlencode({
        "chat_id": chat_id, "text": text,
        "parse_mode": "HTML", "disable_web_page_preview": "true",
    }).encode()
    with urllib.request.urlopen(urllib.request.Request(url, data=data), timeout=20) as resp:
        return json.loads(resp.read().decode())

def build_message(r, recovered=False):
    if recovered:
        return ("🟢 <b>Подписки: ёмкость восстановлена</b>\n"
                f"Свободно слотов: <b>{r['can_sell']}</b> (порог {r['reorder']}).\n"
                f"Активных Max20x: {r['n_subs']} · клиентов {r['clients']['total']}.")
    dot = {"yellow": "🟡", "red": "🔴"}[r["signal"]]
    w7 = next((w for w in r["windows"] if w["w"] == "7d"), {"util": 0})
    head = "пора дозаказывать" if r["signal"] == "yellow" else "ёмкость исчерпана"
    return (f"{dot} <b>Подписки: {head}</b>\n"
            f"Свободно слотов: <b>{r['can_sell']}</b> (порог {r['reorder']}).\n"
            f"Prod-флот: {r['n_subs']} × Max20x · клиентов {r['clients']['paid']}+"
            f"{r['clients']['trial']}={r['clients']['total']} · занято 7d {w7['util']*100:.0f}%.\n"
            f"👉 {r['recommend']}")

def main():
    load_env_file(os.environ.get("AUTH_BOT_ENV", "/etc/agents/auth_bot.env"))
    headroom = float(os.environ.get("HEADROOM", "0.0"))
    reorder  = int(os.environ.get("REORDER", "2"))
    cooldown = int(os.environ.get("ALERT_COOLDOWN", "43200"))

    r = compute(fleet="prod", headroom=headroom, reorder=reorder)
    sig = r["signal"]
    st = load_state()
    last_sig, last_ts = st.get("signal", "green"), st.get("ts", 0)
    now = int(time.time())

    print(f"сигнал: {sig} · свободно {r['can_sell']} · {r['recommend']}")

    recovered = sig == "green" and last_sig in ("yellow", "red")
    need_alert = sig in ("yellow", "red") and (
        sig != last_sig or (now - last_ts) >= cooldown
    )

    if not (need_alert or recovered):
        print("оповещение не требуется (нет смены сигнала / кулдаун).")
        if sig != last_sig:
            save_state(sig)
        return

    msg = build_message(r, recovered=recovered)
    if DRY:
        print("── DRY-RUN, сообщение НЕ отправлено ──")
        print(msg)
        return

    token = os.environ.get("AUTH_BOT_TOKEN", "")
    ids = admin_chat_ids()
    if not token or not ids:
        print(f"error: нет AUTH_BOT_TOKEN или числовых AUTH_BOT_ADMIN — не могу отправить "
              f"(token:{'есть' if token else 'нет'}, получателей:{len(ids)})", file=sys.stderr)
        sys.exit(1)

    sent = 0
    for cid in ids:
        try:
            res = tg_send(token, cid, msg)
            if res.get("ok"):
                sent += 1
            else:
                print(f"warn: TG отклонил для {cid}: {res}", file=sys.stderr)
        except Exception as e:
            print(f"warn: ошибка отправки {cid}: {e}", file=sys.stderr)
    print(f"отправлено {sent}/{len(ids)} админам.")
    if sent:
        save_state(sig)

if __name__ == "__main__":
    main()
