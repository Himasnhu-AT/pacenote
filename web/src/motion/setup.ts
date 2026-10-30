// Global motion for Pacenote. Runs once on the client before the first page initializes.
// Motion verb: read ahead, then commit. The transition is the livery's orange slash crossing
// the screen with the destination called like a stage ("Stage 9 · Frostline"); the loader is
// the start countdown.
import { gsap } from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';
import { ease, onPage, onScroll, prefersReducedMotion, setLoaderExit, setLoaderFormat, setTransition, startScroll, stopScroll } from '../engine';
import { initLive } from './recce';

gsap.registerPlugin(ScrollTrigger);

export function setupMotion() {
  // Name the destination on the curtain.
  document.addEventListener('click', (e) => {
    const a = (e.target as HTMLElement).closest?.('a');
    const label = document.querySelector<HTMLElement>('#curtain .curtain__label');
    if (!a || !label) return;
    label.textContent = a.dataset.dest || a.getAttribute('aria-label')?.replace(/, home$/, '') || a.querySelector('span:not(.slash)')?.textContent?.trim() || a.textContent?.trim() || '';
  }, true);

  setTransition({
    leave: (c) => {
      const panel = c.querySelector('.curtain__panel');
      return gsap.timeline()
        .set(c, { autoAlpha: 1 })
        .fromTo(panel, { xPercent: -130 }, { xPercent: 0, duration: 0.62, ease: ease.inOut })
        .fromTo(c.querySelector('.curtain__label'), { autoAlpha: 0, xPercent: -8 }, { autoAlpha: 1, xPercent: 0, duration: 0.45, ease: ease.out }, 0.32);
    },
    enter: (c) => gsap.timeline()
      .to(c.querySelector('.curtain__label'), { autoAlpha: 0, xPercent: 8, duration: 0.25, ease: ease.soft }, 0)
      .to(c.querySelector('.curtain__panel'), { xPercent: 130, duration: 0.7, ease: ease.inOut }, 0.05)
      .set(c, { autoAlpha: 0 }),
  });

  // Loader: the start countdown, home only, first visit per session, desktop only.
  let seen = false;
  try { seen = sessionStorage.getItem('pacenote-start') === '1'; sessionStorage.setItem('pacenote-start', '1'); } catch {}
  if (innerWidth < 768 || seen || location.pathname !== '/') document.getElementById('loader')?.remove();
  setLoaderFormat((p) => (p >= 100 ? 'GO' : String(5 - Math.min(4, Math.floor(p / 20)))));
  setLoaderExit((root) => {
    const tl = gsap.timeline();
    tl.to(root.querySelectorAll('[data-loader-out]'), { xPercent: 12, autoAlpha: 0, duration: 0.3, ease: ease.soft, stagger: 0.03 }, 0.15)
      .to(root, { clipPath: 'polygon(100% 0%, 100% 0%, 100% 100%, 100% 100%)', duration: 0.6, ease: ease.inOut }, 0.2);
    return tl;
  });

  // The frame steps aside while scrolling down and comes back on the way up.
  let last = 0;
  onScroll(({ y }) => {
    if (Math.abs(y - last) < 4) return;
    document.documentElement.classList.toggle('frame-away', y > 200 && y > last);
    document.documentElement.classList.toggle('frame-solid', y > 80);
    last = y;
  });

  initLive();
  onPage(menu);
  onPage(bars);
}

// Bars on the numbers page fill from zero when they enter.
function bars() {
  if (prefersReducedMotion()) return;
  document.querySelectorAll<HTMLElement>('[data-bars]').forEach((group) => {
    const items = group.querySelectorAll<HTMLElement>('[data-bar]');
    gsap.from(items, { scaleX: 0, transformOrigin: 'left center', duration: 1.1, ease: ease.out, stagger: 0.03, scrollTrigger: { trigger: group, start: 'top 85%', once: true } });
  });
}

// Phone menu: full-screen overlay, focus trapped, Escape closes, focus restored.
function menu() {
  const open = document.querySelector<HTMLButtonElement>('[data-menu-open]');
  const panel = document.querySelector<HTMLElement>('[data-menu]');
  const close = panel?.querySelector<HTMLButtonElement>('[data-menu-close]');
  if (!open || !panel || !close) return;
  const links = () => [...panel.querySelectorAll<HTMLElement>('a, button')];
  const key = (e: KeyboardEvent) => {
    if (e.key === 'Escape') hide();
    if (e.key !== 'Tab') return;
    const f = links();
    if (e.shiftKey && document.activeElement === f[0]) { e.preventDefault(); f[f.length - 1].focus(); }
    else if (!e.shiftKey && document.activeElement === f[f.length - 1]) { e.preventDefault(); f[0].focus(); }
  };
  const show = () => {
    panel.hidden = false;
    open.setAttribute('aria-expanded', 'true');
    stopScroll();
    document.addEventListener('keydown', key);
    if (!prefersReducedMotion()) {
      gsap.fromTo(panel, { clipPath: 'polygon(0% 0%, 0% 0%, 0% 100%, 0% 100%)' }, { clipPath: 'polygon(0% 0%, 100% 0%, 100% 100%, 0% 100%)', duration: 0.6, ease: ease.inOut });
      gsap.from(panel.querySelectorAll('li a span'), { xPercent: -30, autoAlpha: 0, duration: 0.7, ease: ease.out, stagger: 0.05, delay: 0.2 });
    }
    panel.querySelector<HTMLElement>('a')?.focus();
  };
  const hide = () => {
    panel.hidden = true;
    open.setAttribute('aria-expanded', 'false');
    startScroll();
    document.removeEventListener('keydown', key);
    open.focus();
  };
  open.addEventListener('click', show);
  close.addEventListener('click', hide);
  panel.querySelectorAll('a').forEach((a) => a.addEventListener('click', () => { panel.hidden = true; open.setAttribute('aria-expanded', 'false'); startScroll(); document.removeEventListener('keydown', key); }));
  return () => { document.removeEventListener('keydown', key); };
}
