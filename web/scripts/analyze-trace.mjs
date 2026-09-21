import { readFileSync } from 'node:fs';
function summary(values) {
  if (!values.length) return null;
  const a=[...values].sort((a,b)=>a-b);
  return {count:a.length,min:a[0],median:a[Math.floor(a.length/2)],p95:a[Math.floor((a.length-1)*.95)],max:a.at(-1)};
}
export function analyze(trace) {
  const observations=trace.latestClockObservations ?? [];
  const reads=(trace.records ?? []).filter(r=>r.method==='read' && r.result?.synchronization==='Synchronized');
  const walls=(trace.wallReferences ?? []).filter(r=>r.synchronization==='Synchronized' && r.speed===1);
  return {
    operations:trace.records?.length ?? 0,omitted:trace.omitted ?? 0,
    matchedExchanges:observations.length,acceptedExchanges:observations.filter(r=>r.accepted).length,
    pathRttMs:summary(observations.map(r=>Number(BigInt(r.t4)-BigInt(r.t1)-BigInt(r.t3)+BigInt(r.t2))/1e6)),
    absoluteTimelineAdjustmentMs:summary(reads.map(r=>Math.abs(r.result.correctionFrames)/r.result.fps*1000)),
    browserReadIntervalMs:summary(reads.slice(1).map((r,i)=>r.args[0]-reads[i].args[0])),
    sameComputerTodDifferenceMs:summary(walls.map(r=>r.differenceMs)),
    wallPairingBracketMs:summary(walls.map(r=>r.bracketMs)),
    interpretation:'TOD difference is valid only after tod on the same computer/timezone, with no later wall-clock adjustment. Wall timestamps have approximately 1ms resolution. Replay and RTT are not independent accuracy measurements. Neither this trace nor the TOD check measures pixel presentation time.',
  };
}
if(process.argv[1]?.endsWith('/analyze-trace.mjs')) {
  if(!process.argv[2]) throw Error('Usage: pnpm analyze /path/to/ethersync-timing.json');
  console.log(JSON.stringify(analyze(JSON.parse(readFileSync(process.argv[2],'utf8'))),null,2));
}
