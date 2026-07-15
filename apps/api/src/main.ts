import "reflect-metadata";
import helmet from "@fastify/helmet";
import { Logger } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { NestFactory } from "@nestjs/core";
import { FastifyAdapter, type NestFastifyApplication } from "@nestjs/platform-fastify";
import { AppModule } from "./app.module.js";
import type { Environment } from "./config.js";

async function bootstrap(): Promise<void> {
  const app = await NestFactory.create<NestFastifyApplication>(AppModule, new FastifyAdapter({
    logger: false,
    bodyLimit: 1_048_576,
  }), { rawBody: true });
  await app.register(helmet, { contentSecurityPolicy: false });
  const config = app.get(ConfigService<Environment, true>);
  // @fastify/cors по умолчанию отдаёт methods="GET,HEAD,POST" — без DELETE/PATCH браузер
  // заворачивает preflight на revoke-ключа и правку профиля ("Failed to fetch"). Перечисляем
  // явно все методы, которыми пользуется браузерный клиент.
  app.enableCors({
    origin: new URL(config.get("PUBLIC_APP_BASE_URL", { infer: true })).origin,
    credentials: true,
    methods: ["GET", "HEAD", "POST", "PATCH", "DELETE", "OPTIONS"],
  });
  app.enableShutdownHooks();
  app.setGlobalPrefix("v1");

  const host = config.get("HOST", { infer: true });
  const port = config.get("PORT", { infer: true });
  await app.listen(port, host);
  Logger.log(`commercial API listening on http://${host}:${port}`, "Bootstrap");
}

void bootstrap();
