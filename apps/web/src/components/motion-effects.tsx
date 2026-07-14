"use client";

import { useEffect, useRef, type CSSProperties } from "react";

const waveWidth = 2_000;
const waveHeight = 900;
const waveLayers = [
  { y: 250, amplitude: 70, periods: 6, phase: 0, opacity: .20, stroke: 1.6, duration: 34 },
  { y: 420, amplitude: 95, periods: 4, phase: 1.2, opacity: .14, stroke: 1.4, duration: 46 },
  { y: 600, amplitude: 60, periods: 8, phase: 2.4, opacity: .16, stroke: 1.2, duration: 28 },
  { y: 760, amplitude: 110, periods: 4, phase: .6, opacity: .10, stroke: 1.8, duration: 56 },
] as const;

function wavePath(y: number, amplitude: number, periods: number, phase: number): string {
  const points = 240;
  let path = `M0,${y.toFixed(1)}`;
  for (let index = 1; index <= points; index += 1) {
    const x = waveWidth * index / points;
    const offset = y
      + Math.sin(phase + index / points * Math.PI * 2 * periods) * amplitude
      + Math.sin(phase * 1.7 + index / points * Math.PI * 2 * (periods / 2)) * amplitude * .35;
    path += ` L${x.toFixed(1)},${offset.toFixed(1)}`;
  }
  return path;
}

type WaveStyle = CSSProperties & Record<"--sw" | "--o" | "--dur" | "--dl", string | number>;

export function MotionEffects() {
  const decorRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const hero = document.querySelector(".hero");
    const frame = window.requestAnimationFrame(() => hero?.classList.add("loaded"));
    const nav = document.querySelector("header.nav");
    const onScroll = () => nav?.classList.toggle("scrolled", window.scrollY > 8);
    onScroll(); window.addEventListener("scroll", onScroll, { passive: true });

    const targets = document.querySelectorAll<HTMLElement>("[data-reveal], [data-reveal-stagger], .reveal");
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    let observer: IntersectionObserver | null = null;
    if (reduced || !("IntersectionObserver" in window)) targets.forEach((target) => target.classList.add("in"));
    else {
      observer = new IntersectionObserver((entries) => entries.forEach((entry) => {
        if (entry.isIntersecting) { entry.target.classList.add("in"); observer?.unobserve(entry.target); }
      }), { threshold: 0.12, rootMargin: "0px 0px -8% 0px" });
      targets.forEach((target) => observer?.observe(target));
    }

    const decor = decorRef.current;
    let parallaxFrame = 0;
    const onPointerMove = (event: PointerEvent) => {
      if (!decor || reduced || window.matchMedia("(max-width: 760px), (pointer: coarse)").matches) return;
      const x = (event.clientX / window.innerWidth - .5) * 14;
      const y = (event.clientY / window.innerHeight - .5) * 14;
      if (parallaxFrame) return;
      parallaxFrame = window.requestAnimationFrame(() => {
        decor.style.transform = `translate3d(${x}px, ${y}px, 0)`;
        parallaxFrame = 0;
      });
    };
    window.addEventListener("pointermove", onPointerMove, { passive: true });

    const safety = window.setTimeout(() => targets.forEach((target) => target.classList.add("in")), 2_500);
    return () => {
      window.cancelAnimationFrame(frame); window.cancelAnimationFrame(parallaxFrame);
      window.removeEventListener("scroll", onScroll); window.removeEventListener("pointermove", onPointerMove);
      observer?.disconnect(); window.clearTimeout(safety);
    };
  }, []);
  return <div ref={decorRef} className="bg-decor" aria-hidden="true">
    <svg className="wave-field" viewBox={`0 0 ${waveWidth} ${waveHeight}`} preserveAspectRatio="xMidYMid slice">
      <g className="wf-scroll">{waveLayers.map((layer, index) => <path
        className={`wl wl${index}`}
        d={wavePath(layer.y, layer.amplitude, layer.periods, layer.phase)}
        key={layer.y}
        style={{ "--sw": layer.stroke, "--o": layer.opacity, "--dur": `${layer.duration}s`, "--dl": `${-index * 4}s` } as WaveStyle}
      />)}</g>
    </svg>
    <span className="glow g1" /><span className="glow g2" /><span className="glow g3" />
  </div>;
}
