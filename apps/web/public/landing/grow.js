/* =========================================================
   apiToken landing — growing catalog field ("pool")
   Shared by the RU and EN pages; the page script provides
   GROW_ITEMS in its own language.

   Chips drift and bounce as rectangles (pills are wide, so a
   circle approximation let them slide into each other on
   narrow laptop screens). Sizes are measured once and on
   resize, positions go through the `translate` property, and
   the loop sleeps while the field is off screen.
   ========================================================= */
(() => {
  const field = document.getElementById('growField');
  if (!field || typeof GROW_ITEMS === 'undefined') return;

  // phones lay chips out with CSS flex; reduced motion keeps the scatter but no drift
  const NARROW = matchMedia('(max-width:640px)');
  const STILL = matchMedia('(prefers-reduced-motion: reduce)');
  const GAP = 10;
  let fw = 0, fh = 0, balls = [], raf = 0, last = 0, visible = false, scattered = false;

  const measure = () => {
    fw = field.clientWidth; fh = field.clientHeight;
    balls.forEach(b => { b.w = b.el.offsetWidth; b.h = b.el.offsetHeight; });
  };

  const overlaps = (a, b) =>
    a.x < b.x + b.w + GAP && b.x < a.x + a.w + GAP &&
    a.y < b.y + b.h + GAP && b.y < a.y + a.h + GAP;

  // scatter without overlap; fall back to a plain row-wrap if the field is too tight
  const scatter = () => {
    const placed = [];
    let cx = GAP, cy = GAP, rowH = 0;
    balls.forEach(b => {
      let ok = false;
      for (let t = 0; t < 60 && !ok; t++) {
        b.x = Math.random() * Math.max(0, fw - b.w);
        b.y = Math.random() * Math.max(0, fh - b.h);
        ok = !placed.some(p => overlaps(b, p));
      }
      if (!ok) {
        if (cx + b.w > fw) { cx = GAP; cy += rowH + GAP; rowH = 0; }
        b.x = cx; b.y = Math.min(cy, Math.max(0, fh - b.h));
        cx += b.w + GAP; rowH = Math.max(rowH, b.h);
      }
      placed.push(b);
    });
  };

  const paint = () => balls.forEach(b => { b.el.style.translate = `${b.x}px ${b.y}px`; });

  const tick = now => {
    raf = 0;
    if (!visible) return;
    const dt = last ? Math.min((now - last) / 1000, 0.05) : 0; last = now;

    for (const b of balls) {
      b.x += b.vx * dt; b.y += b.vy * dt;
      if (b.x <= 0) { b.x = 0; b.vx = Math.abs(b.vx); }
      if (b.x + b.w >= fw) { b.x = Math.max(0, fw - b.w); b.vx = -Math.abs(b.vx); }
      if (b.y <= 0) { b.y = 0; b.vy = Math.abs(b.vy); }
      if (b.y + b.h >= fh) { b.y = Math.max(0, fh - b.h); b.vy = -Math.abs(b.vy); }
    }

    // rectangle collision: separate along the axis of least penetration and
    // swap that velocity component (equal-mass elastic bounce)
    for (let i = 0; i < balls.length; i++) {
      for (let j = i + 1; j < balls.length; j++) {
        const a = balls[i], b = balls[j];
        const dx = (b.x + b.w / 2) - (a.x + a.w / 2);
        const dy = (b.y + b.h / 2) - (a.y + a.h / 2);
        const px = (a.w + b.w) / 2 - Math.abs(dx);
        const py = (a.h + b.h) / 2 - Math.abs(dy);
        if (px <= 0 || py <= 0) continue;
        if (px < py) {
          const s = dx < 0 ? -1 : 1;
          a.x -= s * px / 2; b.x += s * px / 2;
          if ((b.vx - a.vx) * s < 0) [a.vx, b.vx] = [b.vx, a.vx];
        } else {
          const s = dy < 0 ? -1 : 1;
          a.y -= s * py / 2; b.y += s * py / 2;
          if ((b.vy - a.vy) * s < 0) [a.vy, b.vy] = [b.vy, a.vy];
        }
      }
    }
    for (const b of balls) {
      b.x = Math.max(0, Math.min(b.x, fw - b.w));
      b.y = Math.max(0, Math.min(b.y, fh - b.h));
    }
    paint();
    raf = requestAnimationFrame(tick);
  };

  const run = () => { if (!raf && visible && !NARROW.matches && !STILL.matches) { last = 0; raf = requestAnimationFrame(tick); } };

  const start = () => {
    GROW_ITEMS.forEach((txt, i) => {
      const s = document.createElement('span');
      if (txt.startsWith('+')) s.className = 'new';
      s.textContent = txt;
      s.style.animationDelay = (i * .12) + 's';
      field.appendChild(s);
      const ang = Math.random() * Math.PI * 2, speed = 14 + Math.random() * 16;
      balls.push({ el: s, x: 0, y: 0, w: 0, h: 0, vx: Math.cos(ang) * speed, vy: Math.sin(ang) * speed });
    });
    measure();
    if (!NARROW.matches) { scatter(); scattered = true; paint(); }
    run();
  };

  let started = false;
  new IntersectionObserver(es => {
    visible = es[0].isIntersecting;
    if (visible && !started) { started = true; start(); }
    run();
  }, { threshold: .15 }).observe(field);

  // re-fit on resize: keep chips inside the new bounds, or hand layout to CSS on phones
  new ResizeObserver(() => {
    if (!started) return;
    if (NARROW.matches) { balls.forEach(b => { b.el.style.translate = ''; }); return; }
    measure();
    if (!scattered) { scatter(); scattered = true; }
    balls.forEach(b => {
      b.x = Math.max(0, Math.min(b.x, fw - b.w));
      b.y = Math.max(0, Math.min(b.y, fh - b.h));
    });
    paint(); run();
  }).observe(field);
})();
