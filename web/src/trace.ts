import type { WallReference } from './wall-reference';
import type { Follower } from '../pkg/tidkod_wasm';

// Retain a prefix, never silently evict the initial state needed for exact replay.
// The example enables capture by default (?trace=0 opts out). Recording follows the timed operation.
export function instrument(core: Follower, enabled: boolean) {
  const records: unknown[] = [];
  const wallReferences: WallReference[] = [];
  let stopped = false;
  let omitted = 0;
  const methods = new Set(['connecting', 'connected', 'disconnected', 'snapshot', 'probe',
    'probe_published', 'reply', 'reply_timed', 'read', 'read_for_presentation', 'next_boundary']);
  const encode = (value: unknown): unknown => value instanceof Uint8Array
    ? { bytes: Array.from(value) } : value;
  const follower = new Proxy(core, {
    get(target, property) {
      const value = Reflect.get(target, property);
      if (typeof value !== 'function') return value;
      return (...args: unknown[]) => {
        let result: unknown;
        let failure: unknown;
        try { result = value.apply(target, args); return result; }
        catch (e) { failure = e; throw e; }
        finally {
          if (enabled && methods.has(String(property))) {
            if (!stopped && records.length < 16384 && !args.some(a => a instanceof Uint8Array && a.length > 512)) {
              records.push({ method: property, args: args.map(encode), result: encode(result),
                error: failure === undefined ? undefined : String(failure) });
            } else { stopped = true; omitted++; }
          }
        }
      };
    },
  });
  return { follower: enabled ? follower : core,
    wallReference: (sample: WallReference) => {
      if (!enabled) return;
      if (wallReferences.length === 256) wallReferences.shift();
      wallReferences.push(sample);
    }, export: () => ({ version: 1, timestampDomain: 'performance.now milliseconds',
    wallReferences,
    enabled, prefixComplete: omitted === 0, omitted, records,
    // The suffix is diagnostic only; replay uses the retained complete prefix above.
    latestClockObservations: core.clock_trace() }) };
}
