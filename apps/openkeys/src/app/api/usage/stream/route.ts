import { NextResponse } from "next/server";
import { loadUsageByViewToken } from "@/lib/keys";
import { validViewToken } from "@/lib/usage-session";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

/**
 * Живой канал расхода ключа: Server-Sent Events вместо периодической перезагрузки страницы.
 * Каждые несколько секунд сервер сверяет снапшот и шлёт кадр, только когда что-то
 * действительно изменилось; между кадрами — heartbeat-комментарии, чтобы прокси не резали
 * соединение. guardRequest здесь не применяется: браузерный GET не несёт Origin (sameOrigin
 * дал бы 403 легитимным клиентам), а credential'ом остаётся сама персональная ссылка —
 * тот же контур доступа, что и у страницы /profile/[token].
 */

const TICK_MS = 5_000;
const HEARTBEAT_MS = 15_000;
/** Сколько ошибокных тиков подряд молча переживаем, прежде чем закрыть поток с ошибкой. */
const MAX_CONSECUTIVE_FAILURES = 6;

export async function GET(request: Request): Promise<Response> {
  const token = new URL(request.url).searchParams.get("token") ?? "";
  if (!validViewToken(token)) {
    return NextResponse.json({ error: "not_found" }, { status: 404 });
  }

  const encoder = new TextEncoder();
  let poll: ReturnType<typeof setInterval> | undefined;
  let heartbeat: ReturnType<typeof setInterval> | undefined;

  const stream = new ReadableStream<Uint8Array>({
    async start(controller) {
      let closed = false;
      let lastFrame = "";
      let failures = 0;

      const stop = () => {
        if (closed) return;
        closed = true;
        if (poll) clearInterval(poll);
        if (heartbeat) clearInterval(heartbeat);
        try {
          controller.close();
        } catch {
          // поток уже закрыт другой стороной
        }
      };
      request.signal.addEventListener("abort", stop);

      const send = (chunk: string): void => {
        if (closed) return;
        try {
          controller.enqueue(encoder.encode(chunk));
        } catch {
          stop();
        }
      };

      const tick = async (): Promise<void> => {
        if (closed) return;
        try {
          const view = await loadUsageByViewToken(token);
          failures = 0;
          if (!view) {
            stop();
            return;
          }
          // Кадр уходит только при реальном изменении рендеримых данных.
          const frame = JSON.stringify(view);
          if (frame !== lastFrame) {
            lastFrame = frame;
            send(`data: ${frame}\n\n`);
          }
        } catch {
          // Движок/БД чихнули — молча ждём следующий тик, но не вечно.
          failures += 1;
          if (failures >= MAX_CONSECUTIVE_FAILURES) stop();
        }
      };

      await tick();
      if (closed) return;
      poll = setInterval(() => void tick(), TICK_MS);
      heartbeat = setInterval(() => send(`: ping\n\n`), HEARTBEAT_MS);
    },
    cancel() {
      if (poll) clearInterval(poll);
      if (heartbeat) clearInterval(heartbeat);
    },
  });

  return new Response(stream, {
    headers: {
      "content-type": "text/event-stream; charset=utf-8",
      "cache-control": "no-cache, no-transform",
      // Подсказка прокси не буферизовать SSE (Caddy и так стримит, но путь не должен зависеть от дефолтов).
      "x-accel-buffering": "no",
    },
  });
}
