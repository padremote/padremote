/**
 * Chrome on iPhone can retain the wrong native viewport after a Camera launch.
 * A document navigation after Chrome is visible works on the affected device;
 * resizing the existing page and resetting its scroll position did not.
 */
export function launchPad(start: () => void): () => void {
  const url = new URL(window.location.href);
  const fragment = new URLSearchParams(url.hash.slice(1));
  const navigation = performance.getEntriesByType("navigation")[0] as PerformanceNavigationTiming | undefined;
  const iphoneChrome = /iPhone|iPod/.test(navigator.userAgent) && /CriOS\//.test(navigator.userAgent);
  const marker = "__pad_launch";

  // Keep the marker in the URL: the second document must be able to skip this
  // step even when both localStorage and sessionStorage are unavailable.
  if (!iphoneChrome || !fragment.get("h") || url.searchParams.get(marker) === "1" ||
    navigation?.type === "reload" || navigation?.type === "back_forward") {
    start();
    return () => {};
  }

  let quietTimer: number | undefined;
  let deadline: number | undefined;
  const viewport = window.visualViewport;
  const clearTimers = () => {
    window.clearTimeout(quietTimer);
    window.clearTimeout(deadline);
    quietTimer = deadline = undefined;
  };
  const stop = () => {
    clearTimers();
    window.removeEventListener("load", schedule);
    window.removeEventListener("focus", schedule);
    window.removeEventListener("resize", schedule);
    window.removeEventListener("pageshow", schedule);
    document.removeEventListener("visibilitychange", schedule);
    viewport?.removeEventListener("resize", schedule);
  };
  const navigate = () => {
    stop();
    url.searchParams.set(marker, "1");
    // Changing the query makes this a real document navigation. Keep all
    // pairing data in the fragment and replace the landing history entry.
    window.location.replace(url.href);
  };
  function schedule(): void {
    if (document.readyState !== "complete" || document.visibilityState !== "visible") {
      clearTimers();
      return;
    }
    window.clearTimeout(quietTimer);
    quietTimer = window.setTimeout(navigate, 700);
    // Browser-bar animation may keep firing resize; never wait indefinitely.
    deadline ??= window.setTimeout(navigate, 3000);
  }
  window.addEventListener("load", schedule);
  window.addEventListener("focus", schedule);
  window.addEventListener("resize", schedule);
  window.addEventListener("pageshow", schedule);
  document.addEventListener("visibilitychange", schedule);
  viewport?.addEventListener("resize", schedule);
  schedule();
  return stop;
}
