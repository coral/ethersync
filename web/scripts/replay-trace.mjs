import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import { Follower, core_build_id } from '../pkg-node/tidkod_wasm.js';
export function replay(trace) {
  assert.equal(trace.version, 1);
  if (trace.coreBuildId !== undefined) assert.equal(trace.coreBuildId, core_build_id(), 'Replay requires the same core build');
  assert.equal(trace.enabled, true, 'This export has diagnostics only: replay capture was disabled. Reload without ?trace=0 for a complete capture.');
  assert.ok(Array.isArray(trace.records) && trace.records.length <= 16384);
  const allowed = new Set(['connecting', 'connected', 'disconnected', 'snapshot', 'probe',
    'probe_published', 'reply', 'reply_timed', 'read', 'read_for_presentation', 'next_boundary']);
  const core = new Follower();
  try {
    for (const [i, record] of trace.records.entries()) {
      assert.ok(allowed.has(record.method), `Unknown operation at ${i}`);
      const args = record.args.map(a => a && typeof a === 'object' && 'bytes' in a ? Uint8Array.from(a.bytes) : a);
      let result, error;
      try { result = core[record.method](...args); } catch (e) { error = String(e); }
      assert.equal(error, record.error, `Exception at operation ${i}`);
      if (result instanceof Uint8Array) result = { bytes: Array.from(result) };
      // JSON normalizes undefined and nonfinite startup uncertainty the same way as export.
      assert.equal(JSON.stringify(result), JSON.stringify(record.result), `Result at operation ${i}`);
    }
    return { operations: trace.records.length, omitted: trace.omitted ?? 0 };
  } finally { core.free(); }
}
if (process.argv[1]?.endsWith('/replay-trace.mjs')) {
  const file = process.argv[2];
  if (!file) throw new Error('Usage: pnpm replay /path/to/tidkod-timing.json');
  console.log(replay(JSON.parse(readFileSync(file, 'utf8'))));
}
