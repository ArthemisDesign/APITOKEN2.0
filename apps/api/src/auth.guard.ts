import {
  CanActivate,
  createParamDecorator,
  ExecutionContext,
  Injectable,
  UnauthorizedException,
} from "@nestjs/common";
import type { AuthUserView } from "@claude-api/contracts";
import { AuthService } from "./auth.service.js";

export interface RequestAuth {
  sessionId: string;
  user: AuthUserView;
}

export interface AuthenticatedRequest {
  headers: Record<string, string | string[] | undefined>;
  auth?: RequestAuth;
}

@Injectable()
export class SessionAuthGuard implements CanActivate {
  constructor(private readonly auth: AuthService) {}

  async canActivate(context: ExecutionContext): Promise<boolean> {
    const request = context.switchToHttp().getRequest<AuthenticatedRequest>();
    const token = readCookie(typeof request.headers.cookie === "string" ? request.headers.cookie : "", sessionCookieName());
    if (!token) throw new UnauthorizedException("authentication required");
    const resolved = await this.auth.authenticate(token);
    if (!resolved) throw new UnauthorizedException("authentication required");
    request.auth = resolved;
    return true;
  }
}

export const CurrentAuth = createParamDecorator((_data: unknown, context: ExecutionContext): RequestAuth => {
  const auth = context.switchToHttp().getRequest<AuthenticatedRequest>().auth;
  if (!auth) throw new UnauthorizedException("authentication required");
  return auth;
});

export function sessionCookieName(): string {
  return process.env.NODE_ENV === "production" ? "__Host-apitoken_session" : "apitoken_session";
}

function readCookie(header: string, name: string): string | null {
  for (const item of header.split(";")) {
    const separator = item.indexOf("=");
    if (separator < 0) continue;
    if (item.slice(0, separator).trim() === name) return item.slice(separator + 1).trim();
  }
  return null;
}
