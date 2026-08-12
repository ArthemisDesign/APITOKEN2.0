import { Controller, Header, Sse, UseGuards, type MessageEvent } from "@nestjs/common";
import type { Observable } from "rxjs";
import { AdminEventsService } from "./admin-events.service.js";
import { AdminGuard } from "./admin.guard.js";

@Controller("admin")
@UseGuards(AdminGuard)
export class AdminEventsController {
  constructor(private readonly events: AdminEventsService) {}

  @Sse("events")
  @Header("Cache-Control", "no-cache, no-transform")
  @Header("X-Accel-Buffering", "no")
  stream(): Observable<MessageEvent> {
    return this.events.stream();
  }
}
