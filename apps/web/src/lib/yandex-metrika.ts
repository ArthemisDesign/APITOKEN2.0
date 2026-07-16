export const YANDEX_METRIKA_ID = 110788366;

// Keep the official loader in the server-rendered document head. Yandex's
// installation checker inspects the initial page source before Next.js hydrates.
export const yandexMetrikaBootstrap = `
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
    url:location.origin+location.pathname,
    accurateTrackBounce:true,
    trackLinks:true
  });
`;
