import { API_BASE_URL } from "@/lib/api";

const backendOrigin = new URL(API_BASE_URL).origin;

/**
 * Authenticated and authentication-entry routes call the commerce API as soon
 * as they hydrate. Let those routes pay the connection setup cost without
 * opening an unused backend connection on public content pages.
 */
export function BackendPreconnect() {
  return <>
    <link rel="dns-prefetch" href={backendOrigin} />
    <link rel="preconnect" href={backendOrigin} crossOrigin="use-credentials" />
  </>;
}
