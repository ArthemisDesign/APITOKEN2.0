import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { __clearToastsForTests, dismissToast, getToasts, subscribeToasts, toast, TOAST_TIMEOUT_MS } from "./toast";

// Стор тостов тестируется без React: подписка, авто-скрытие по таймаутам
// легаси (5 с success / 9 с bad), ручное закрытие.

beforeEach(() => {
  vi.useFakeTimers();
  __clearToastsForTests();
});
afterEach(() => {
  vi.useRealTimers();
  __clearToastsForTests();
});

describe("toast", () => {
  it("добавляет тост в стек и уведомляет подписчиков", () => {
    const listener = vi.fn();
    const unsubscribe = subscribeToasts(listener);
    toast("Сохранено.");
    expect(getToasts()).toEqual([{ id: expect.any(Number), message: "Сохранено.", kind: "ok" }]);
    expect(listener).toHaveBeenCalledTimes(1);
    unsubscribe();
  });

  it("kind по умолчанию — ok; bad помечается отдельно", () => {
    toast("обычный");
    toast("плохой", "bad");
    const [okNote, badNote] = getToasts();
    expect(okNote.kind).toBe("ok");
    expect(badNote.kind).toBe("bad");
  });

  it("success авто-скрывается за 5 с, bad — за 9 с (таймауты admin-panel.js)", () => {
    toast("обычный");
    toast("плохой", "bad");
    vi.advanceTimersByTime(TOAST_TIMEOUT_MS.ok);
    expect(getToasts()).toHaveLength(1);
    expect(getToasts()[0].kind).toBe("bad");
    vi.advanceTimersByTime(TOAST_TIMEOUT_MS.bad - TOAST_TIMEOUT_MS.ok);
    expect(getToasts()).toHaveLength(0);
  });

  it("dismissToast удаляет вручную и гасит таймер", () => {
    toast("плохой", "bad");
    const id = getToasts()[0].id;
    dismissToast(id);
    expect(getToasts()).toHaveLength(0);
    vi.advanceTimersByTime(60_000);
    expect(getToasts()).toHaveLength(0);
  });

  it("стабильная ссылка snapshot: без изменений массив тот же", () => {
    toast("обычный");
    const first = getToasts();
    expect(getToasts()).toBe(first);
  });
});
