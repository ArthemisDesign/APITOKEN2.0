(function(){
  'use strict';

  function initDocsTabs(){
    const tabs = document.querySelectorAll('.docs-tabs .tab');
    const panels = document.querySelectorAll('.docs-panel');
    if(!tabs.length) return;

    tabs.forEach(function(tab){
      tab.addEventListener('click', function(){
        const target = tab.dataset.tab;

        tabs.forEach(function(t){ t.classList.remove('on'); });
        tab.classList.add('on');

        panels.forEach(function(p){
          p.classList.toggle('active', p.dataset.panel === target);
        });
      });
    });
  }

  if(document.readyState === 'loading'){
    document.addEventListener('DOMContentLoaded', initDocsTabs);
  } else {
    initDocsTabs();
  }
})();
