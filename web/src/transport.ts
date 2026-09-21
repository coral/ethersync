import * as Moq from '@moq/net';
import type { Follower } from '../pkg/ethersync_wasm';

export interface Diagnostics {
  message: string;
  transport: string;
  rtt?: number;
  lost?: number;
}
export function endpoint(address: string): URL {
  const url = new URL(address.includes('://') ? address : `https://${address}`);
  if (url.protocol !== 'https:' || url.username || url.password || url.search || url.hash || url.pathname !== '/') {
    throw new Error('Enter an IP address and port, e.g. 192.168.1.10:4443 (HTTPS only).');
  }
  return url;
}
export function fingerprint(pin: string): string {
  const clean = pin.replaceAll(':', '').trim().toLowerCase();
  if (!/^[0-9a-f]{64}$/.test(clean)) throw new Error('Paste the leader’s 64-character SHA-256 fingerprint.');
  return clean;
}
function delay(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    signal.throwIfAborted();
    const abort = () => { clearTimeout(timer); reject(signal.reason); };
    const timer = setTimeout(() => { signal.removeEventListener('abort', abort); resolve(); }, ms);
    signal.addEventListener('abort', abort, { once: true });
  });
}
function reason(e: unknown): string {
  if (e instanceof AggregateError) return e.errors.map(reason).join('; ');
  return e instanceof Error ? e.message : String(e);
}

// Only transport orchestration belongs here. WASM owns all Ethersync message parsing,
// probe matching, clock fitting, stale-state rejection, corrections and extrapolation.
export async function follow(
  core: Follower, url: URL, pin: string, signal: AbortSignal,
  update: (d: Diagnostics) => void,
): Promise<void> {
  let backoff = 100;
  while (!signal.aborted) {
    const broadcast = new Moq.Broadcast.Producer();
    const requests = broadcast.createTrack('clock/request');
    const attempt = new AbortController();
    const abort = () => attempt.abort(signal.reason);
    signal.addEventListener('abort', abort, { once: true });
    let connection: Awaited<ReturnType<typeof Moq.Connection.connect>> | undefined;
    let incoming: ReturnType<NonNullable<typeof connection>['consume']> | undefined;
    let states: Moq.Track.Subscriber | undefined;
    let replies: Moq.Track.Subscriber | undefined;
    try {
      core.connecting();
      update({ message: 'Connecting…', transport: 'WebTransport / HTTP/3' });
      connection = await Moq.Connection.connect(url, {
        websocket: { enabled: false },
        webtransport: { protocols: ['moq-lite-05'], serverCertificateHashes: [{ value: pin }] },
        signal: AbortSignal.any([attempt.signal, AbortSignal.timeout(5000)]),
      });
      if (signal.aborted) break;
      if (connection.version !== 'moq-lite-05') throw new Error(`Unsupported MoQ version: ${connection.version}`);
      connection.publish(Moq.Path.from('ethersync/v1'), broadcast);
      core.connected();
      backoff = 100;
      incoming = connection.consume(Moq.Path.from('ethersync/v1'));
      states = incoming.subscribe('state');
      replies = incoming.subscribe('clock/reply');
      const conn = connection;
      const stateTrack = states;
      const replyTrack = replies;
      let lastMessage = performance.now();
      const stateLoop = async () => {
        while (!attempt.signal.aborted) {
          const group = await stateTrack.recvGroup();
          if (!group) throw new Error('State track ended');
          try {
            const frame = await group.readFrame();
            const received = performance.now();
            if (attempt.signal.aborted) return;
            if (!frame) throw new Error('Empty state group');
            core.snapshot(frame.payload, received);
            lastMessage = received;
          } finally { group.close(); }
        }
      };
      const replyLoop = async () => {
        while (!attempt.signal.aborted) {
          const d = await replyTrack.recvDatagram();
          const received = performance.now();
          if (attempt.signal.aborted) return;
          if (!d) throw new Error('Clock track ended');
          core.reply_timed(d.payload, received, performance.now());
        }
      };
      const probeLoop = async () => {
        while (!attempt.signal.aborted) {
          const stamp = Moq.Time.Timestamp.now();
          const payload = core.probe(performance.now());
          requests.appendDatagram(stamp, payload);
          core.probe_published(performance.now());
          await delay(core.probe_interval_ms(), attempt.signal);
        }
      };
      const statsLoop = async () => {
        while (!attempt.signal.aborted) {
          const stats = await conn.stats();
          if (attempt.signal.aborted) return;
          update({ message: 'Following leader', transport: `${conn.transport} / ${conn.version}`, rtt: stats.rtt, lost: stats.packetsLost });
          if (performance.now() - lastMessage > 5000) throw new Error('Leader state timed out');
          await delay(1000, attempt.signal);
        }
      };
      const stopped = new Promise<void>((resolve) => {
        if (attempt.signal.aborted) resolve();
        else attempt.signal.addEventListener('abort', () => resolve(), { once: true });
      });
      await Promise.race([stateLoop(), replyLoop(), probeLoop(), statsLoop(), conn.closed.then(e => { throw e ?? new Error('Connection closed'); }), stopped]);
    } catch (e) {
      if (!signal.aborted) update({ message: `${reason(e)} — retrying in ${backoff} ms`, transport: 'Disconnected' });
    } finally {
      attempt.abort();
      signal.removeEventListener('abort', abort);
      states?.close(); replies?.close(); incoming?.close();
      connection?.close(); requests.close(); broadcast.close();
      core.disconnected();
    }
    if (!signal.aborted) {
      try { await delay(backoff, signal); } catch { break; }
      backoff = Math.min(backoff * 2, 5000);
    }
  }
}
