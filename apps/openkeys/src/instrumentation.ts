/** Next.js node runtime hook: starts crash-recovery reconciliation once per service process. */
export async function register(): Promise<void> {
  if (process.env.NEXT_RUNTIME !== "nodejs") return;
  const { startIssuanceReconciler } = await import("./lib/keys");
  startIssuanceReconciler();
}
