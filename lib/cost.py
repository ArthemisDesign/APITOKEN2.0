"""USD-эквивалент хода из токенов — по-модельные цены, как в движке tg_agent.
Стоимость считается по ОФИЦИАЛЬНЫМ API-ценам Anthropic ($/Mtoken), даже когда ход
идёт на квоте подписки — это метрика «сколько бы стоило по API».
Порт из RUST/tools/tg_agent/src/bin/main.rs (struct Px / model_price / usd_cost)."""

def model_price(model: str) -> dict:
    """Цены $/Mtoken: inp (вход), cw5 (кэш-запись 5м), cw1 (кэш-запись 1ч),
    rd (кэш-чтение), out (выход). По имени модели."""
    m = (model or "").lower()
    if "sonnet-5" in m or m == "claude-sonnet-5":
        return dict(inp=3.0,  cw5=3.75,  cw1=6.0,  rd=0.30, out=15.0)
    if any(x in m for x in ("opus-4-8", "opus-4-7", "opus-4-6", "opus-4-5")):
        return dict(inp=5.0,  cw5=6.25,  cw1=10.0, rd=0.50, out=25.0)
    if "opus-4-1" in m or "opus-4-0" in m or m == "claude-opus-4":
        return dict(inp=15.0, cw5=18.75, cw1=30.0, rd=1.50, out=75.0)
    if "sonnet-4" in m:
        return dict(inp=3.0,  cw5=3.75,  cw1=6.0,  rd=0.30, out=15.0)
    if "haiku-4-5" in m:
        return dict(inp=1.0,  cw5=1.25,  cw1=2.0,  rd=0.10, out=5.0)
    if "haiku-3-5" in m:
        return dict(inp=0.80, cw5=1.0,   cw1=1.60, rd=0.08, out=4.0)
    return dict(inp=5.0, cw5=6.25, cw1=10.0, rd=0.50, out=25.0)  # дефолт = opus-4-8

def usd_cost(model, intok=0, cache_read=0, cache_write_5m=0, cache_write_1h=0,
             out=0, web_searches=0) -> float:
    """USD за ход из фактических токенов (4 корзины + сплит кэш-записи 5м/1ч + web_search)."""
    p = model_price(model)
    return (intok * p["inp"] + cache_read * p["rd"]
            + cache_write_5m * p["cw5"] + cache_write_1h * p["cw1"]
            + out * p["out"]) / 1_000_000.0 + web_searches * 0.01

def cost_from_usage(model: str, u: dict):
    """Из блока usage ответа claude → (usd, разбивка токенов). Нет сплита кэш-записи →
    считаем всё как 5м (как в движке)."""
    u = u or {}
    ti = u.get("input_tokens", 0);  to = u.get("output_tokens", 0)
    cr = u.get("cache_read_input_tokens", 0)
    cc = u.get("cache_creation_input_tokens", 0)
    split = u.get("cache_creation", {}) or {}
    cc5 = split.get("ephemeral_5m_input_tokens", 0)
    cc1 = split.get("ephemeral_1h_input_tokens", 0)
    if cc5 + cc1 == 0 and cc > 0:
        cc5 = cc
    ws = (u.get("server_tool_use", {}) or {}).get("web_search_requests", 0)
    usd = usd_cost(model, ti, cr, cc5, cc1, to, ws)
    return usd, dict(intok=ti, out=to, cache_read=cr, cache_write=cc5 + cc1, web=ws)

if __name__ == "__main__":  # spot-check против движковых тестов
    assert abs(usd_cost("claude-haiku-4-5", intok=1_000_000) - 1.0) < 1e-9
    assert abs(usd_cost("claude-haiku-4-5", out=1_000_000) - 5.0) < 1e-9
    assert abs(usd_cost("claude-opus-4-8", out=1_000_000) - 25.0) < 1e-9
    print("cost.py: прайс-формулы совпадают с движком ✓")
