/** Keep the pad and its canvas inside the browser's visible rectangle. */
export type Unwatch = () => void;

/**
 * Mobile browser bars, the keyboard and rotation can change independently of
 * the layout viewport. Publish one rectangle for the app frame so its canvas,
 * touch surface and controls always share the same coordinates.
 */
export function trackViewport(): Unwatch {
  const root = document.documentElement;
  const properties = new Map<string, string>();
  const apply = () => {
    const vv = window.visualViewport;
    const layoutW = root.clientWidth || window.innerWidth;
    const layoutH = root.clientHeight || window.innerHeight;
    // Some initial/restored frames expose a zero visual viewport. Falling back
    // keeps the page usable until the browser publishes its settled geometry.
    const visible = vv && Number.isFinite(vv.width) && vv.width > 0 &&
      Number.isFinite(vv.height) && vv.height > 0;
    const w = visible ? vv.width : layoutW;
    const h = visible ? vv.height : layoutH;
    const top = visible && Number.isFinite(vv.offsetTop) ? Math.max(0, vv.offsetTop) : 0;
    const left = visible && Number.isFinite(vv.offsetLeft) ? Math.max(0, vv.offsetLeft) : 0;
    if (w <= 0 || h <= 0) return;

    const set = (name: string, px: number) => {
      const value = `${Math.max(0, Math.round(px * 100) / 100)}px`;
      if (properties.get(name) === value) return;
      properties.set(name, value);
      root.style.setProperty(name, value);
    };
    set("--app-w", w);
    set("--app-h", h);
    set("--app-top", top);
    set("--app-left", left);
    set("--app-bottom", layoutH - top - h);
    set("--app-right", layoutW - left - w);
  };

  let timers: number[] = [];
  const settle = () => {
    timers.forEach(window.clearTimeout);
    apply();
    // iOS can report the previous size during pageshow/orientationchange and
    // settle without a final resize event. Repeat briefly after each lifecycle
    // transition, including restoration from the back/forward cache.
    timers = [60, 150, 300, 600, 1200, 2000].map((ms) => window.setTimeout(apply, ms));
  };
  const onVisible = () => {
    if (document.visibilityState === "visible") settle();
  };
  settle();
  const stop = watchSize(root, settle);
  window.addEventListener("pageshow", settle);
  window.addEventListener("orientationchange", settle);
  document.addEventListener("visibilitychange", onVisible);
  document.addEventListener("focusout", settle);
  return () => {
    timers.forEach(window.clearTimeout);
    stop();
    window.removeEventListener("pageshow", settle);
    window.removeEventListener("orientationchange", settle);
    document.removeEventListener("visibilitychange", onVisible);
    document.removeEventListener("focusout", settle);
  };
}

/** Re-measure a surface once per frame when any geometry source changes. */
export function watchSize(el: Element, onChange: () => void): Unwatch {
  let frame: number | null = null;
  let stopped = false;
  const fire = () => {
    if (stopped || frame !== null) return;
    frame = requestAnimationFrame(() => {
      frame = null;
      if (!stopped) onChange();
    });
  };
  const onVisible = () => {
    if (document.visibilityState === "visible") fire();
  };
  const observer = typeof ResizeObserver === "function" ? new ResizeObserver(fire) : null;
  observer?.observe(el);

  window.addEventListener("resize", fire);
  window.addEventListener("scroll", fire, { passive: true });
  window.addEventListener("orientationchange", fire);
  window.addEventListener("pageshow", fire);
  document.addEventListener("visibilitychange", onVisible);
  const vv = window.visualViewport;
  vv?.addEventListener("resize", fire);
  vv?.addEventListener("scroll", fire);
  // Also measure after the first layout when ResizeObserver is unavailable.
  fire();

  return () => {
    stopped = true;
    if (frame !== null) cancelAnimationFrame(frame);
    observer?.disconnect();
    window.removeEventListener("resize", fire);
    window.removeEventListener("scroll", fire);
    window.removeEventListener("orientationchange", fire);
    window.removeEventListener("pageshow", fire);
    document.removeEventListener("visibilitychange", onVisible);
    vv?.removeEventListener("resize", fire);
    vv?.removeEventListener("scroll", fire);
  };
}
