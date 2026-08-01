import { API_BASE_URL } from "@/lib/api";

const backendOrigin = new URL(API_BASE_URL).origin;

/**
 * Routes that call the commerce API as soon as they hydrate warm the connection
 * setup cost in advance. This includes every public page with SiteHeader: the
 * header fires the /auth/me identity check at hydration, so the backend
 * connection is always used there.
 */
export function BackendPreconnect() {
  return <>
    <link rel="dns-prefetch" href={backendOrigin} />
    <link rel="preconnect" href={backendOrigin} crossOrigin="use-credentials" />
  </>;
}
