import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  __clearPollersForTests,
  dismissError,
  getErrorRecoveryVersion,
  getErrors,
  getPoller,
  refreshPoller,
  revalidateAll,
  subscribeErrors,
} from "./usePoll";

// Ядро опроса (getPoller/Poller) тестируется без React: подписка, дедупликация,
// интервальный тик, ошибки, revalidateAll. usePoll — тонкая обёртка над этим
// через useSyncExternalStore.

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

beforeEach(() => __clearPollersForTests());
afterEach(() => {
  vi.useRealTimers();
  __clearPollersForTests();
});

describe("getPoller", () => {
  it("дедуплицирует poller'ы по ключу", () => {
    const a = getPoller("k", async () => 1);
    const b = getPoller("k", async () => 2);
    expect(a).toBe(b);
  });

  it("первая подписка запускает загрузку, snapshot обновляется данными", async () => {
    const fetcher = vi.fn(async () => ({ ok: 1 }));
    const poller = getPoller<{ ok: number }>("dash", fetcher);
    expect(poller.getSnapshot().isLoading).toBe(true);

    const listener = vi.fn();
    const unsubscribe = poller.subscribe(listener);
    expect(fetcher).toHaveBeenCalledTimes(1);

    await flush();
    const snapshot = poller.getSnapshot();
    expect(snapshot.data).toEqual({ ok: 1 });
    expect(snapshot.error).toBeUndefined();
    expect(snapshot.isLoading).toBe(false);
    expect(listener).toHaveBeenCalled();
    unsubscribe();
  });

  it("параллельные refresh() делят один in-flight запрос", async () => {
    let resolveFetch!: (value: number) => void;
    const fetcher = vi.fn(() => new Promise<number>((resolve) => (resolveFetch = resolve)));
    const poller = getPoller<number>("dedup", fetcher);
    poller.subscribe(() => {});
    poller.refresh();
    poller.refresh();
    expect(fetcher).toHaveBeenCalledTimes(1);
    resolveFetch(7);
    await flush();
    expect(poller.getSnapshot().data).toBe(7);
  });

  it("опрашивает по интервалу (document отсутствует → вкладка «видима»)", async () => {
    vi.useFakeTimers();
    const fetcher = vi.fn(async () => 1);
    const poller = getPoller("interval", fetcher, { interval: 1000 });
    const unsubscribe = poller.subscribe(() => {});
    await vi.advanceTimersByTimeAsync(0);
    expect(fetcher).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(1000);
    expect(fetcher).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(3000);
    expect(fetcher).toHaveBeenCalledTimes(5);

    unsubscribe();
    await vi.advanceTimersByTimeAsync(5000);
    expect(fetcher).toHaveBeenCalledTimes(5); // таймер остановлен после отписки
  });

  it("stale snapshot немедленно обновляется при remount, свежий cache сеть не вызывает", async () => {
    vi.useFakeTimers();
    const fetcher = vi.fn(async () => fetcher.mock.calls.length);
    const poller = getPoller("remount", fetcher, { interval: 1000 });
    const unsubscribe = poller.subscribe(() => {});
    await vi.advanceTimersByTimeAsync(0);
    expect(fetcher).toHaveBeenCalledTimes(1);

    unsubscribe();
    const unsubscribeFresh = poller.subscribe(() => {});
    await vi.advanceTimersByTimeAsync(0);
    expect(fetcher).toHaveBeenCalledTimes(1);
    unsubscribeFresh();

    await vi.advanceTimersByTimeAsync(1000);
    const unsubscribeStale = poller.subscribe(() => {});
    await vi.advanceTimersByTimeAsync(0);
    expect(fetcher).toHaveBeenCalledTimes(2);
    unsubscribeStale();
  });

  it("error snapshot немедленно повторяется при remount", async () => {
    let fail = true;
    const fetcher = vi.fn(async () => {
      if (fail) throw new Error("HTTP 503");
      return "восстановлено";
    });
    const poller = getPoller<string>("error-remount", fetcher, { interval: 30_000 });
    const unsubscribe = poller.subscribe(() => {});
    await flush();
    expect(poller.getSnapshot().error?.message).toBe("HTTP 503");
    unsubscribe();

    fail = false;
    const unsubscribeRecovered = poller.subscribe(() => {});
    await flush();
    expect(fetcher).toHaveBeenCalledTimes(2);
    expect(poller.getSnapshot()).toMatchObject({ data: "восстановлено", error: undefined });
    unsubscribeRecovered();
  });

  it("ошибка попадает в snapshot, старые данные сохраняются", async () => {
    let fail = false;
    const fetcher = vi.fn(async () => {
      if (fail) throw new Error("HTTP 503");
      return "данные";
    });
    const poller = getPoller<string>("err", fetcher);
    poller.subscribe(() => {});
    await flush();
    expect(poller.getSnapshot().data).toBe("данные");

    fail = true;
    poller.refresh();
    await flush();
    const snapshot = poller.getSnapshot();
    expect(snapshot.error?.message).toBe("HTTP 503");
    expect(snapshot.data).toBe("данные");
    expect(snapshot.isLoading).toBe(false);
  });

  it("revalidateAll перезагружает только poller'ы с живыми подписчиками", async () => {
    const a = vi.fn(async () => "a");
    const b = vi.fn(async () => "b");
    const unsubscribeA = getPoller("ra-a", a).subscribe(() => {});
    getPoller("ra-b", b).subscribe(() => {});
    await flush();
    expect(a).toHaveBeenCalledTimes(1);
    expect(b).toHaveBeenCalledTimes(1);

    unsubscribeA();
    revalidateAll();
    await flush();
    expect(a).toHaveBeenCalledTimes(1);
    expect(b).toHaveBeenCalledTimes(2);
  });
});

describe("реестр ошибок источников (ErrorCenter)", () => {
  it("успешный retry живого источника снимает ошибку и увеличивает recovery version", async () => {
    let fail = true;
    const fetcher = vi.fn(async () => {
      if (fail) throw new Error("HTTP 503");
      return "данные";
    });
    const poller = getPoller<string>("src-a", fetcher);
    poller.subscribe(() => {});
    await flush();
    expect(getErrors()).toEqual([{ key: "src-a", message: "HTTP 503", dismissed: false }]);
    expect(getErrorRecoveryVersion()).toBe(0);

    fail = false;
    poller.refresh();
    await flush();
    expect(getErrors()).toEqual([]);
    expect(getErrorRecoveryVersion()).toBe(1);
  });

  it("dismissError скрывает ошибку; повторное падение и успех не создают recovery", async () => {
    let fail = true;
    const fetcher = vi.fn(async () => {
      if (fail) throw new Error("HTTP 500");
      return "данные";
    });
    const poller = getPoller<string>("src-b", fetcher);
    poller.subscribe(() => {});
    await flush();
    expect(getErrors()).toHaveLength(1);

    dismissError("src-b");
    expect(getErrors()).toEqual([]);

    poller.refresh();
    await flush();
    // Ошибка обновилась в реестре, но остаётся скрытой (как в admin-panel.js).
    expect(getErrors()).toEqual([]);
    fail = false;
    poller.refresh();
    await flush();
    expect(getErrorRecoveryVersion()).toBe(0);
  });

  it("subscribeErrors уведомляет о появлении и снятии ошибок", async () => {
    const listener = vi.fn();
    const unsubscribe = subscribeErrors(listener);
    let fail = true;
    const fetcher = vi.fn(async () => {
      if (fail) throw new Error("сбой");
      return 1;
    });
    const poller = getPoller<number>("src-c", fetcher);
    poller.subscribe(() => {});
    await flush();
    fail = false;
    poller.refresh();
    await flush();
    expect(listener).toHaveBeenCalledTimes(2);
    unsubscribe();
  });

  it("refreshPoller перезапрашивает только смонтированный источник (кнопка ↻)", async () => {
    const fetcher = vi.fn(async () => "x");
    const unsubscribe = getPoller("src-d", fetcher).subscribe(() => {});
    await flush();
    expect(fetcher).toHaveBeenCalledTimes(1);
    refreshPoller("src-d");
    await flush();
    expect(fetcher).toHaveBeenCalledTimes(2);

    unsubscribe();
    refreshPoller("src-d");
    refreshPoller("нет-такого");
    await flush();
    expect(fetcher).toHaveBeenCalledTimes(2);
  });

  it("deactivate скрывает ошибку без recovery, remount показывает cache и повторяет fetch", async () => {
    const fetcher = vi.fn(async () => {
      throw new Error("скрытый сбой");
    });
    const poller = getPoller("src-unmounted", fetcher);
    const unsubscribe = poller.subscribe(() => {});
    await flush();
    expect(getErrors()).toHaveLength(1);

    unsubscribe();
    expect(getErrors()).toEqual([]);
    expect(getErrorRecoveryVersion()).toBe(0);
    expect(poller.getSnapshot().error?.message).toBe("скрытый сбой");

    const unsubscribeRemount = poller.subscribe(() => {});
    expect(getErrors()).toHaveLength(1);
    await flush();
    expect(fetcher).toHaveBeenCalledTimes(2);
    expect(getErrors()).toHaveLength(1);
    expect(getErrorRecoveryVersion()).toBe(0);
    unsubscribeRemount();
  });

  it("pending rejection после последней отписки обновляет snapshot, но не публикует ошибку", async () => {
    let rejectFetch!: (error: Error) => void;
    const poller = getPoller(
      "src-race",
      () => new Promise<never>((_resolve, reject) => (rejectFetch = reject)),
    );
    const unsubscribe = poller.subscribe(() => {});
    unsubscribe();

    rejectFetch(new Error("поздний сбой"));
    await flush();
    expect(poller.getSnapshot().error?.message).toBe("поздний сбой");
    expect(getErrors()).toEqual([]);
    expect(getErrorRecoveryVersion()).toBe(0);
  });
});
