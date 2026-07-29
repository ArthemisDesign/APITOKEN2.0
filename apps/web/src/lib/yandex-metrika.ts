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

// Keep the loader URL and initialization call in the server-rendered document
// head for Yandex's installation checker, but do not compete with the critical
// CSS/fonts. Queued hits are flushed when the external script loads after LCP.
export const yandexMetrikaBootstrap = `
  var pageUrl=new URL(location.href),attributionParameters=${serializedAttributionParameters};
  Array.from(pageUrl.searchParams.keys()).forEach(function(parameter){
    if(attributionParameters.indexOf(parameter)===-1){pageUrl.searchParams.delete(parameter);}
  });
  pageUrl.hash='';

  (function(m,e,t,r,i){
    m[i]=m[i]||function(){(m[i].a=m[i].a||[]).push(arguments)};
    m[i].l=1*new Date();
    function load(){
      for(var j=0;j<document.scripts.length;j++){if(document.scripts[j].src===r){return;}}
      var k=e.createElement(t),a=e.getElementsByTagName(t)[0];
      k.async=1;k.src=r;a.parentNode.insertBefore(k,a);
    }
    function schedule(){
      var started=false;
      function start(){
        if(started){return;}
        started=true;
        if('requestIdleCallback' in m){m.requestIdleCallback(load,{timeout:1500});}
        else{load();}
      }
      ['pointerdown','keydown','touchstart'].forEach(function(eventName){
        m.addEventListener(eventName,start,{once:true,passive:true});
      });
      m.setTimeout(start,5000);
    }
    if(e.readyState==='complete'){schedule();}
    else{m.addEventListener('load',schedule,{once:true});}
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
