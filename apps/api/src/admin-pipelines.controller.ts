import { Controller, Get, Header, UseGuards } from "@nestjs/common";
import { AdminGuard } from "./admin.guard.js";
import { AdminPipelinesService } from "./admin-pipelines.service.js";

@Controller("admin")
@UseGuards(AdminGuard)
export class AdminPipelinesController {
  constructor(private readonly pipelines: AdminPipelinesService) {}

  // Без query-параметров: списки сбоев жёстко ограничены (≤10 вебхуков, ≤5 писем, ≤5 ошибок
  // pricing-джоб), окно «24 часа» и пороги вердикта зафиксированы в AdminPipelinesService.
  @Get("pipeline-health")
  @Header("Cache-Control", "no-store")
  pipelineHealth(): Promise<Record<string, unknown>> {
    return this.pipelines.pipelineHealth();
  }
}
