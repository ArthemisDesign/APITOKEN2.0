export const YANDEX_METRIKA_ID = 110788366;

const YANDEX_ATTRIBUTION_QUERY_PARAMETERS = [
  "utm_source",
  "utm_medium",
  "utm_campaign",
  "utm_content",
  "utm_term",
  "utm_referrer",
  "openstat_service",
  "openstat_campaign",
  "openstat_ad",
  "openstat_source",
  "yclid",
  "ymclid",
  "ysclid",
  "gclid",
  "yqrid",
  "yzclid",
  "_ym_debug",
] as const;

export function yandexMetrikaPageUrl(rawUrl: string): string {
  const url = new URL(rawUrl);
  const allowedParameters = new Set<string>(YANDEX_ATTRIBUTION_QUERY_PARAMETERS);

  for (const parameter of Array.from(url.searchParams.keys())) {
    if (!allowedParameters.has(parameter)) url.searchParams.delete(parameter);
  }
  url.hash = "";

  return url.href;
}

const serializedAttributionParameters = JSON.stringify(YANDEX_ATTRIBUTION_QUERY_PARAMETERS);

// Keep the official loader in the server-rendered document head. Yandex's
// installation checker inspects the initial page source before Next.js hydrates.
export const yandexMetrikaBootstrap = `
  var pageUrl=new URL(location.href),attributionParameters=${serializedAttributionParameters};
  Array.from(pageUrl.searchParams.keys()).forEach(function(parameter){
    if(attributionParameters.indexOf(parameter)===-1){pageUrl.searchParams.delete(parameter);}
  });
  pageUrl.hash='';

  (function(m,e,t,r,i,k,a){
    m[i]=m[i]||function(){(m[i].a=m[i].a||[]).push(arguments)};
    m[i].l=1*new Date();
    for(var j=0;j<document.scripts.length;j++){if(document.scripts[j].src===r){return;}}
    k=e.createElement(t),a=e.getElementsByTagName(t)[0],k.async=1,k.src=r,a.parentNode.insertBefore(k,a)
  })(window,document,'script','https://mc.yandex.ru/metrika/tag.js?id=${YANDEX_METRIKA_ID}','ym');

  ym(${YANDEX_METRIKA_ID},'init',{
    ssr:true,
    webvisor:true,
    clickmap:true,
    ecommerce:'dataLayer',
    referrer:document.referrer.split('#',1)[0].split('?',1)[0],
    url:pageUrl.href,
    accurateTrackBounce:true,
    trackLinks:true
  });
`;
