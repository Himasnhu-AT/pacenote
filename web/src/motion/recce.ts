// The signature driver: ONE number per stage, metres along the road (s), mapped to every visual
// that carries the concept: the car on the map, the driven road, the next 500 m lit (as on the
// guide's map in the game), the elevation marker, the odometer and the co-driver's call sheet.
//
//   [data-live="idle"]    s advances on the GSAP ticker at a stage pace (slows for tight corners)
//   [data-live="scroll"]  s follows scroll progress through the pinned section (smoothed)
//
// Reduced motion: no ticker drive and no pin; the scroll stage gets a range input instead.
// Calls are the game's wording (src/lib/stage.ts ports src/codriver/calls.rs). Speech is the
// browser's Web Speech API and only after a click.
import { gsap } from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';
import { onPage, prefersReducedMotion } from '../engine';
import { distance, gradeWidth } from '../lib/stage';

gsap.registerPlugin(ScrollTrigger);

type Part = { t: string; g: number | null };
type CallData = { at: number; end: number; text: string; grade: number | null; parts: Part[]; corners: number[] };
type Data = { step: number; length: number; points: [number, number][]; calls: CallData[] };

const LIT = 500; // metres lit ahead, as on the guide's map
const CALL_LEAD = 150; // the co-driver calls a corner this far out (calls.rs: speed × 4.5 s, 70..280)
// Idle pace by the grade of the corner ahead (m/s), roughly how the game's car takes them.
const GRADE_SPEED = [9, 9, 13, 17, 21, 26, 31];
const STRAIGHT = 33;

const esc = (s: string) => s.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]!);

function callHTML(c: CallData, d: number) {
  const dist = d > 40 ? `<span class="live__dist">${distance(d)}</span> ` : '';
  const lines = c.parts
    .map((p, i) => `<span class="live__line">${i ? '<span class="live__into">into</span> ' : dist}<span class="live__part${p.g !== null ? ` g${Math.max(1, p.g)}` : ''}" style="font-stretch:${gradeWidth(p.g)}%">${esc(p.t)}</span></span>`)
    .join('');
  return lines;
}

class Live {
  el: HTMLElement;
  data: Data;
  mode: 'idle' | 'scroll';
  s: number;
  target: number;
  speed = 0;
  idx = -1;
  calledIdx = -1;
  lastDist = '';
  visible = false;
  voice = false;
  car: SVGGElement | null;
  lit: SVGPathElement | null;
  driven: SVGPathElement | null;
  corners: SVGPathElement[];
  odo: HTMLElement | null;
  call: HTMLElement | null;
  next: HTMLElement | null;
  pDone: SVGRectElement | null;
  pNow: SVGLineElement | null;
  scale: number;

  constructor(el: HTMLElement) {
    this.el = el;
    this.data = JSON.parse(el.querySelector('[data-live-data]')!.textContent || '{}');
    this.mode = el.dataset.live as 'idle' | 'scroll';
    this.s = this.target = parseFloat(el.dataset.start || '0');
    this.car = el.querySelector('[data-car]');
    this.lit = el.querySelector('[data-lit]');
    this.driven = el.querySelector('[data-driven]');
    this.corners = [...el.querySelectorAll<SVGPathElement>('[data-corner]')];
    this.odo = el.querySelector('[data-odo]');
    this.call = el.querySelector('[data-call]');
    this.next = el.querySelector('[data-next]');
    this.pDone = el.querySelector('[data-profile-done]');
    this.pNow = el.querySelector('[data-profile-now]');
    this.scale = this.pNow ? 1000 : 0;
    this.render(true);
  }

  get length() { return (this.data.points.length - 1) * this.data.step; }

  pos(s: number) {
    const pts = this.data.points;
    const f = Math.max(0, Math.min(pts.length - 1.001, s / this.data.step));
    const i = Math.floor(f), t = f - i;
    const a = pts[i], b = pts[i + 1];
    return { x: a[0] + (b[0] - a[0]) * t, z: a[1] + (b[1] - a[1]) * t, h: Math.atan2(b[1] - a[1], b[0] - a[0]) };
  }

  /** Idle drive: brake for the corner ahead, accelerate out of it, loop at the finish. */
  drive(dt: number) {
    const ahead = this.data.calls.find((c) => c.end >= this.s && c.at - this.s < 90);
    const want = ahead && ahead.grade !== null ? GRADE_SPEED[Math.max(1, ahead.grade)] : STRAIGHT;
    const a = want > this.speed ? 5 : -10;
    this.speed = Math.max(6, this.speed + a * dt * (want > this.speed ? 1 : Math.min(1, (this.speed - want) / 3)));
    this.s += this.speed * dt * 1.6; // a little faster than real time: a hero, not a replay
    if (this.s >= this.length) { this.s = 0; this.speed = 0; this.idx = -1; this.calledIdx = -1; }
  }

  render(force = false) {
    const { s } = this;
    const L = this.length;
    const p = this.pos(s);
    this.car?.setAttribute('transform', `translate(${p.x.toFixed(1)} ${p.z.toFixed(1)}) rotate(${((p.h * 180) / Math.PI).toFixed(1)})`);
    if (this.lit) { this.lit.style.strokeDasharray = `${LIT} ${L * 2}`; this.lit.style.strokeDashoffset = `${-s}`; }
    if (this.driven) { this.driven.style.strokeDasharray = `${s} ${L * 2}`; }
    if (this.pDone) this.pDone.setAttribute('width', `${(s / L) * 1000}`);
    if (this.pNow) { const x = ((s / L) * 1000).toFixed(1); this.pNow.setAttribute('x1', x); this.pNow.setAttribute('x2', x); }
    if (this.odo) this.odo.textContent = (s / 1000).toFixed(2);

    const calls = this.data.calls;
    let idx = calls.findIndex((c) => c.end >= s);
    if (idx < 0) idx = calls.length; // past the last corner: the finish
    const c = calls[idx];
    const d = c ? Math.max(0, c.at - s) : L - s;
    const dist = c ? (d > 40 ? distance(d) : '') : '';
    if (idx !== this.idx || force) {
      this.idx = idx;
      this.corners.forEach((el) => el.classList.remove('is-next'));
      if (c) c.corners.forEach((k) => this.corners[k]?.classList.add('is-next'));
      if (this.call) {
        this.call.innerHTML = c ? callHTML(c, d) : `<span class="live__line"><span class="live__part">${L - s > 40 ? `${distance(L - s)} to flying finish, flat out` : 'flying finish'}</span></span>`;
        this.call.dataset.n = String(c ? c.parts.length : 1);
      }
      if (this.next) {
        this.next.innerHTML = calls
          .slice(idx + 1, idx + 4)
          .map((n) => `<li><span class="live__next-d">${Math.round(n.at - s)}</span><span class="live__next-t">${esc(n.text)}</span></li>`)
          .join('');
      }
      if (this.call && !force && !prefersReducedMotion()) {
        gsap.fromTo(this.call.children, { yPercent: 60, autoAlpha: 0 }, { yPercent: 0, autoAlpha: 1, duration: 0.6, ease: 'expo.out', stagger: 0.04, overwrite: true });
      }
      this.lastDist = dist;
    } else if (dist !== this.lastDist) {
      this.lastDist = dist;
      const el = this.call?.querySelector('.live__dist');
      if (el && dist) el.textContent = dist;
      else if (c && this.call) this.call.innerHTML = callHTML(c, d);
    }
    if (this.next) {
      this.next.querySelectorAll<HTMLElement>('.live__next-d').forEach((el, i) => {
        const n = calls[idx + 1 + i];
        if (n) el.textContent = String(Math.max(0, Math.round((n.at - s) / 10) * 10));
      });
    }
    // The co-driver's moment: the call is made when the corner comes inside the lead.
    if (c && d <= CALL_LEAD && this.calledIdx < idx) {
      this.calledIdx = idx;
      this.el.classList.add('is-calling');
      gsap.delayedCall(0.9, () => this.el.classList.remove('is-calling'));
      if (this.voice) this.speak(`${d > 40 ? distance(d) + ' ' : ''}${c.text}`);
    }
  }

  speak(text: string) {
    const synth = window.speechSynthesis;
    if (!synth) return;
    synth.cancel();
    const u = new SpeechSynthesisUtterance(text);
    u.rate = 1.15;
    u.lang = 'en-GB';
    // Prefer an on-device English voice (some browsers' default voices are remote).
    const voices = synth.getVoices().filter((v) => v.lang.startsWith('en'));
    const local = voices.find((v) => v.localService && v.lang === 'en-GB') || voices.find((v) => v.localService);
    if (local) u.voice = local;
    synth.speak(u);
  }
}

export function initLive() {
  onPage(() => {
    const els = [...document.querySelectorAll<HTMLElement>('[data-live]')];
    if (!els.length) return;
    const reduced = prefersReducedMotion();
    const lives = els.map((el) => new Live(el));
    const io = new IntersectionObserver((es) => es.forEach((e) => {
      const l = lives.find((x) => (x.el.closest('[data-recce]') || x.el) === e.target);
      if (l) l.visible = e.isIntersecting;
    }), { rootMargin: '10% 0px' });
    // Observe the box that holds the stage (a scroll stage's .live is display: contents).
    const boxOf = (l: Live) => l.el.closest<HTMLElement>('[data-recce]') || l.el;
    lives.forEach((l) => io.observe(boxOf(l)));

    // Voice: a click turns it on (never autoplay); off on navigation.
    lives.forEach((l) => {
      const btn = l.el.querySelector<HTMLButtonElement>('[data-speak]');
      if (!btn || !('speechSynthesis' in window)) return;
      btn.hidden = false;
      l.el.querySelector<HTMLElement>('[data-speak-note]')!.hidden = false;
      const label = btn.querySelector<HTMLElement>('[data-speak-label]')!;
      const off = label.textContent || '';
      btn.addEventListener('click', () => {
        l.voice = !l.voice;
        btn.setAttribute('aria-pressed', String(l.voice));
        label.textContent = l.voice ? 'Reading out loud' : off;
        if (l.voice) {
          const c = l.data.calls[l.idx];
          if (c) l.speak(`${c.at - l.s > 40 ? distance(c.at - l.s) + ' ' : ''}${c.text}`);
          l.calledIdx = l.idx;
        } else window.speechSynthesis.cancel();
      });
    });

    // Scroll stages: pinned, scroll progress is the throttle.
    const triggers: ScrollTrigger[] = [];
    lives.filter((l) => l.mode === 'scroll').forEach((l) => {
      const section = l.el.closest<HTMLElement>('[data-recce]') || l.el;
      if (reduced) {
        const range = section.querySelector<HTMLInputElement>('[data-range]');
        if (range) {
          range.closest<HTMLElement>('[data-range-wrap]')!.hidden = false;
          range.max = String(Math.floor(l.length));
          range.addEventListener('input', () => { l.s = l.target = +range.value; l.render(); });
        }
        return;
      }
      const pin = section.querySelector<HTMLElement>('[data-pin]') || section;
      triggers.push(ScrollTrigger.create({
        trigger: section,
        start: 'top top',
        end: () => `+=${Math.round(innerHeight * 5)}`,
        pin,
        pinSpacing: true,
        invalidateOnRefresh: true,
        onUpdate: (st) => { l.target = st.progress * (l.length - 1); },
      }));
    });

    if (reduced) return () => { io.disconnect(); window.speechSynthesis?.cancel(); };

    const tick = (_t: number, dtMs: number) => {
      const dt = Math.min(0.05, dtMs / 1000);
      for (const l of lives) {
        if (!l.visible) continue;
        if (l.mode === 'idle') { l.drive(dt); l.render(); }
        else if (Math.abs(l.target - l.s) > 0.05) { l.s += (l.target - l.s) * Math.min(1, dt * 6); l.render(); }
      }
    };
    gsap.ticker.add(tick);
    return () => {
      gsap.ticker.remove(tick);
      io.disconnect();
      triggers.forEach((t) => t.kill());
      window.speechSynthesis?.cancel();
    };
  });
}
