import { afterEach, describe, expect, it, vi } from "vitest";
import { __resetDialogsForTests, dialog, getDialogs, resolveDialog, subscribeDialogs } from "./dialog";

// Стор промис-диалогов тестируется без React: постановка в стек, резолв
// значениями полей (подтверждение) и null (отмена).

afterEach(() => __resetDialogsForTests());

describe("dialog", () => {
  it("кладёт запрос в стек и уведомляет подписчиков", () => {
    const listener = vi.fn();
    const unsubscribe = subscribeDialogs(listener);
    void dialog({ title: "Отключить аккаунт", danger: true });
    expect(getDialogs()).toHaveLength(1);
    expect(getDialogs()[0]).toMatchObject({ title: "Отключить аккаунт", danger: true });
    expect(listener).toHaveBeenCalledTimes(1);
    unsubscribe();
  });

  it("подтверждение резолвит промис значениями полей", async () => {
    const promise = dialog({ title: "Пополнить", fields: [{ name: "amount", label: "Сумма" }] });
    const request = getDialogs()[0];
    resolveDialog(request.id, { amount: "25" });
    await expect(promise).resolves.toEqual({ amount: "25" });
    expect(getDialogs()).toHaveLength(0);
  });

  it("отмена резолвит промис null", async () => {
    const promise = dialog({ title: "Удалить?" });
    resolveDialog(getDialogs()[0].id, null);
    await expect(promise).resolves.toBeNull();
  });

  it("resolveDialog с чужим id — no-op", () => {
    void dialog({ title: "Остаться" });
    resolveDialog(999_999, null);
    expect(getDialogs()).toHaveLength(1);
  });

  it("параллельные диалоги стекуются и резолвятся независимо", async () => {
    const first = dialog({ title: "Первый" });
    const second = dialog({ title: "Второй" });
    expect(getDialogs()).toHaveLength(2);
    resolveDialog(getDialogs()[1].id, { ok: "2" });
    resolveDialog(getDialogs()[0].id, null);
    await expect(second).resolves.toEqual({ ok: "2" });
    await expect(first).resolves.toBeNull();
    expect(getDialogs()).toHaveLength(0);
  });
});
