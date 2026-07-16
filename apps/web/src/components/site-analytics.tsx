"use client";

import { Analytics, type BeforeSendEvent } from "@vercel/analytics/next";
import { usePathname } from "next/navigation";
import Script from "next/script";
import { useEffect, useRef, useState } from "react";

const YANDEX_METRIKA_ID = 110788366;

type YandexMetrika = (counterId: number, method: string, ...args: unknown[]) => void;

declare global {
  interface Window {
    ym?: YandexMetrika;
  }
}

const yandexMetrikaBootstrap = `
  (function(m,e,t,r,i,k,a){
    m[i]=m[i]||function(){(m[i].a=m[i].a||[]).push(arguments)};
    m[i].l=1*new Date();
    for (var j=0;j<document.scripts.length;j++){if(document.scripts[j].src===r){return;}}
    k=e.createElement(t),a=e.getElementsByTagName(t)[0],k.async=1,k.src=r,a.parentNode.insertBefore(k,a)
  })(window,document,'script','https://mc.yandex.ru/metrika/tag.js?id=${YANDEX_METRIKA_ID}','ym');

  ym(${YANDEX_METRIKA_ID},'init',{
    ssr:true,
    defer:true,
    webvisor:true,
    clickmap:true,
    ecommerce:'dataLayer',
    referrer:document.referrer.split('#',1)[0].split('?',1)[0],
    url:location.origin+location.pathname,
    accurateTrackBounce:true,
    trackLinks:true
  });
`;

export function withoutSensitiveUrlData(url: string): string {
  return url.split("#", 1)[0]!.split("?", 1)[0]!;
}

export function SiteAnalytics() {
  const pathname = usePathname();
  const previousPathname = useRef<string | null>(null);
  const [yandexReady, setYandexReady] = useState(false);

  useEffect(() => {
    if (!yandexReady || previousPathname.current === pathname) return;

    window.ym?.(YANDEX_METRIKA_ID, "hit", pathname, {
      referer: previousPathname.current ?? withoutSensitiveUrlData(document.referrer),
      title: document.title,
    });
    previousPathname.current = pathname;
  }, [pathname, yandexReady]);

  return <>
    <Analytics beforeSend={(event: BeforeSendEvent) => ({
      ...event,
      url: withoutSensitiveUrlData(event.url),
    })} />
    <Script
      id="yandex-metrika"
      strategy="afterInteractive"
      onReady={() => setYandexReady(true)}
    >
      {yandexMetrikaBootstrap}
    </Script>
    <noscript>
      <div>
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img src={`https://mc.yandex.ru/watch/${YANDEX_METRIKA_ID}`} style={{ position: "absolute", left: "-9999px" }} alt="" />
      </div>
    </noscript>
  </>;
}
