import { spawn, spawnSync } from 'node:child_process';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { Follower } from '../pkg-node/tidkod_wasm.js';
const root = fileURLToPath(new URL('../..', import.meta.url));
const build = spawnSync('cargo', ['build', '-p', 'tidkod-wasm', '--example', 'accuracy_peer'], {cwd:root,stdio:'inherit'});
if(build.status) process.exit(build.status);
const child=spawn(`${root}/target/debug/examples/accuracy_peer`,[],{stdio:['pipe','pipe','inherit']});
const f=new Follower();
const pending=new Map(); let sequence=0, readyResolve, failure, probeTimer;
const ready=new Promise(r=>readyResolve=r);
const lineReader=createInterface({input:child.stdout});
const send=s=>child.stdin.write(`${s}\n`);
const closed=new Promise(resolve=>child.on('exit',code=>{ failure=new Error(`fixture exited ${code}`); for(const p of pending.values())p.reject(failure); resolve(code); }));
lineReader.on('line',line=>{
  const received=performance.now();
  const [kind,...parts]=line.split(' ');
  try {
    if(kind==='READY') { f.connected(); readyResolve(); }
    else if(kind==='STATE') f.snapshot(Buffer.from(parts[0],'hex'),received);
    else if(kind==='REPLY') f.reply(Buffer.from(parts[0],'hex'),received);
    else { const p=pending.get(parts[0]); if(p){pending.delete(parts[0]);p.resolve({sent:p.sent,received,values:parts.slice(1)});} }
  } catch(e) {failure=e;}
});
function command(name,...args) {
  if(failure) return Promise.reject(failure);
  const id=String(++sequence);
  return new Promise((resolve,reject)=>{pending.set(id,{resolve,reject,sent:performance.now()});send(`${name} ${id} ${args.join(' ')}`);});
}
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function calibrate() {
  let low=-Infinity, high=Infinity;
  for(let i=0;i<100;i++) {
    const r=await command('CAL'); const t=Number(r.values[0])/1e6;
    // The leader's sample is strictly between sending CAL and receiving its reply.
    // Intersect every bound; do not assume symmetric IPC delay or use the estimator.
    low=Math.max(low,t-r.received); high=Math.min(high,t-r.sent);
  }
  assert.ok(high>=low,'independent clock calibration bounds disagree');
  assert.ok(high-low<1,`calibration too imprecise: ${high-low}ms`);
  return {offset:(low+high)/2,uncertainty:(high-low)/2};
}
const summary=values=>{const s=[...values].sort((a,b)=>a-b),a=values.map(Math.abs).sort((a,b)=>a-b);return {medianMs:s[Math.floor(s.length/2)],p95AbsMs:a[Math.floor(a.length*.95)],maxAbsMs:a.at(-1)};};
try {
  await Promise.race([ready,closed.then(()=>{throw failure;}),sleep(8000).then(()=>{throw Error('fixture startup timeout');})]);
  const initial=await calibrate();
  const probe=()=>{send(`PROBE ${Buffer.from(f.probe(performance.now())).toString('hex')}`);probeTimer=setTimeout(probe,f.probe_interval_ms());};probe();
  const acquisition=performance.now();
  while(f.read(performance.now()).synchronization!=='Synchronized') {if(performance.now()-acquisition>5000)throw Error('acquisition timeout');await sleep(10);}
  console.log(JSON.stringify({acquisitionMs:performance.now()-acquisition,calibration:initial}));
  for(const [num,den] of [[1,1],[-1,1],[1,2],[2,1],[0,1]]) {
    const disc=f.read(performance.now()).discontinuity;
    await command('RATE',num,den);
    const deadline=performance.now()+3000;
    while(f.read(performance.now()).discontinuity===disc){assert.ok(performance.now()<deadline);await sleep(5);}
    // Both sides have now applied the control; sample at one independently calibrated instant.
    const calibration=await calibrate();
    const clockErrors=[],frameErrors=[],corrections=[];
    for(let i=0;i<600;i++) {
      const at=performance.now(), r=f.read(at);
      const native=await command('SAMPLE',Math.round((at+calibration.offset)*1e6));
      assert.equal(r.discontinuity,native.values[1]);
      clockErrors.push(r.mappedLeaderMs-(at+calibration.offset));
      frameErrors.push((r.frames-Number(native.values[0]))/30*1000);
      corrections.push(r.correctionFrames/30*1000);
      await sleep(5);
    }
    const result={rate:num/den,calibration,clock:summary(clockErrors),timecode:summary(frameErrors),slew:summary(corrections)};
    console.log(JSON.stringify(result));
    assert.ok(result.clock.p95AbsMs < 2+calibration.uncertainty,'clock mapping exceeds 2ms');
    assert.ok(result.timecode.p95AbsMs < 2*Math.max(1,Math.abs(num/den))+calibration.uncertainty,'timecode exceeds 2ms at 1x');
  }
  const final=await calibrate();
  console.log(JSON.stringify({calibrationChangeMs:final.offset-initial.offset,finalCalibration:final}));
  assert.ok(Math.abs(final.offset-initial.offset)<initial.uncertainty+final.uncertainty+.1,'reference clock changed');
} finally {
  clearTimeout(probeTimer); send('QUIT'); child.stdin.end();
  const timer=setTimeout(()=>child.kill(),2000);
  await closed;clearTimeout(timer);lineReader.close();f.free();
}
