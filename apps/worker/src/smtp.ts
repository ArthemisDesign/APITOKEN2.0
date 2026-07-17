import type { Environment } from "./config.js";

type SmtpSecurityEnvironment = Pick<Environment, "NODE_ENV" | "SMTP_SECURE">;

export function smtpSecurityOptions(environment: SmtpSecurityEnvironment) {
  return {
    secure: environment.SMTP_SECURE,
    requireTLS: environment.NODE_ENV === "production" && !environment.SMTP_SECURE,
    tls: { minVersion: "TLSv1.2" },
  } as const;
}
