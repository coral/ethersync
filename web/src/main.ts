import init, { Follower } from '../pkg/ethersync_wasm';
import { endpoint, fingerprint, follow, type Diagnostics } from './transport';
import './style.css';
import { instrument } from './trace';
import { compareTod } from './wall-reference';

const element = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const put = (id: string, text: string) => { const e = element(id); if (e.textContent !== text) e.textContent = text; };
const address = element<HTMLInputElement>('address');
const pin = element<HTMLInputElement>('pin');
const connect = element<HTMLButtonElement>('connect');
const disconnect = element<HTMLButtonElement>('disconnect');
let active: AbortController | undefined;
let finished: Promise<void> = Promise.resolve();
let operation = 0;
connect.disabled = true;

interface Reading {
  label: string; frames: number; speed: number; fps: number;
  connection: string; synchronization: string; source: string; health: string;
  uncertaintyMs: number; sampleAgeMs: number; offsetMs: number; driftPpm: number;
  mappedLeaderMs: number; correctionFrames: number;
  offsetEvidence?: { lowerMs: number; upperMs: number; consistent: boolean; samples: number };
  acceptedObservations: string; discontinuity: string; event: string;
}
const fmt = (n: number, unit: string, digits = 3) => Number.isFinite(n) ? `${n.toFixed(digits)} ${unit}` : '—';
try {
  await init();
  const capture = instrument(new Follower(), new URLSearchParams(location.search).get('trace') !== '0');
  const core = capture.follower;
  element('export-trace').addEventListener('click', () => {
    const url = URL.createObjectURL(new Blob([JSON.stringify(capture.export())], { type: 'application/json' }));
    const a = document.createElement('a'); a.href = url; a.download = 'ethersync-timing.json'; a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  });
  if (!window.isSecureContext || !('WebTransport' in window)) {
    throw new Error('WebTransport requires a supported browser and localhost or HTTPS.');
  }
  connect.disabled = false;
  put('message', 'Ready. Enter the address and fingerprint from your native leader.');
  const update = (d: Diagnostics) => {
    put('message', d.message); put('transport', d.transport);
    put('stats', `${d.rtt === undefined ? 'Unavailable' : fmt(d.rtt, 'ms')} / ${d.lost ?? 'Unavailable'}`);
  };
  element('connect-form').addEventListener('submit', async e => {
    e.preventDefault();
    const current = ++operation;
    try {
      const url = endpoint(address.value.trim());
      const hash = fingerprint(pin.value);
      active?.abort();
      connect.disabled = true;
      await finished;
      if (current !== operation) return;
      active = new AbortController();
      disconnect.disabled = false;
      connect.textContent = 'Reconnect';
      finished = follow(core, url, hash, active.signal, update).catch(e => put('message', String(e)));
    } catch (e) { put('message', String(e)); }
    finally { if (current === operation) connect.disabled = false; }
  });
  disconnect.addEventListener('click', async () => {
    const current = ++operation;
    active?.abort();
    await finished;
    if (current !== operation) return;
    connect.disabled = false;
    disconnect.disabled = true;
    put('message', 'Disconnected. Holding the last trajectory.');
  });
  window.addEventListener('pagehide', () => active?.abort());
  document.addEventListener('visibilitychange', () => {
    put('visibility', document.hidden ? 'Tab hidden: browser timers may be throttled.' : 'Keep this tab visible for timing tests.');
  });
  function render() {
    const before = performance.now();
    const wallUtcMs = Date.now();
    const after = performance.now();
    const localMs = (before + after) / 2;
    const r = core.read(localMs) as Reading;
    const wallDate = new Date(wallUtcMs);
    const timezoneOffsetMinutes = wallDate.getTimezoneOffset();
    put('system-time', `${String(wallDate.getHours()).padStart(2,'0')}:${String(wallDate.getMinutes()).padStart(2,'0')}:${String(wallDate.getSeconds()).padStart(2,'0')}.${String(wallDate.getMilliseconds()).padStart(3,'0')}`);
    const { differenceMs } = compareTod(r.frames, r.fps, wallUtcMs, timezoneOffsetMinutes);
    capture.wallReference({ localMs, wallUtcMs, timezoneOffsetMinutes, bracketMs: after - before,
      frames: r.frames, fps: r.fps, speed: r.speed, synchronization: r.synchronization,
      discontinuity: r.discontinuity, differenceMs });
    put('tod-reference', r.synchronization !== 'Uninitialized' && r.speed === 1
      ? `${differenceMs >= 0 ? '+' : ''}${differenceMs.toFixed(3)} ms (positive = timecode ahead)` : 'Requires initialized +1× playback');
    put('timecode', r.label);
    put('state', `${r.connection} / ${r.synchronization}`);
    element('state').dataset.synced = String(r.synchronization === 'Synchronized');
    put('trajectory', `${r.fps.toFixed(3).replace(/\.000$/, '')} fps · ${r.speed.toFixed(3)}×`);
    put('uncertainty', fmt(r.uncertaintyMs, 'ms')); put('drift', fmt(r.driftPpm, 'ppm', 1));
    put('age', fmt(r.sampleAgeMs / 1000, 's', 2)); put('offset', fmt(r.offsetMs, 'ms'));
    put('adjustment', `${r.correctionFrames >= 0 ? '+' : ''}${r.correctionFrames.toFixed(6)} frames (${fmt(r.correctionFrames / r.fps * 1000, 'ms at 1×')})`);
    const evidence = r.offsetEvidence;
    put('evidence', evidence ? `${evidence.consistent ? `${evidence.lowerMs.toFixed(3)} … ${evidence.upperMs.toFixed(3)} ms` : 'Inconsistent timing evidence'} (${evidence.samples} samples)` : '—');
    put('frames', r.frames.toFixed(6)); put('source', `${r.source} / ${r.health}`);
    put('discontinuity', r.discontinuity); put('event', r.event || '—');
    requestAnimationFrame(render);
  }
  render();
} catch (e) { put('message', String(e)); }
