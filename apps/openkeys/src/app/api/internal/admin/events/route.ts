import { getOpenkeysAdminChangeFeed, type OpenkeysAdminChangeEvent } from "@/lib/admin-events";
import { internalAdminActor } from "@/lib/internal-admin";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const encoder = new TextEncoder();
const HEARTBEAT_MS = 25_000;

function frame(type: "change" | "resync", event: OpenkeysAdminChangeEvent): Uint8Array {
  return encoder.encode(`event: ${type}\ndata: ${JSON.stringify(event)}\n\n`);
}

export async function GET(request: Request): Promise<Response> {
  if (!internalAdminActor(request)) {
    return Response.json({ error: "not_found" }, { status: 404 });
  }

  let unsubscribe = (): void => undefined;
  let heartbeat: ReturnType<typeof setInterval> | undefined;
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      unsubscribe = getOpenkeysAdminChangeFeed().subscribe((event) => {
        controller.enqueue(frame(event.resync ? "resync" : "change", event));
      });
      heartbeat = setInterval(
        () => controller.enqueue(encoder.encode(": heartbeat\n\n")),
        HEARTBEAT_MS,
      );
    },
    cancel() {
      unsubscribe();
      if (heartbeat) clearInterval(heartbeat);
    },
  });

  return new Response(stream, {
    headers: {
      "cache-control": "no-cache, no-transform",
      "content-type": "text/event-stream; charset=utf-8",
      "x-accel-buffering": "no",
    },
  });
}
