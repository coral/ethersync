import { spawnSync } from 'node:child_process';
import { existsSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';
const web = fileURLToPath(new URL('..', import.meta.url));
const root = resolve(web, '..');
function run(cmd, args) {
  const r = spawnSync(cmd, args, { cwd: root, stdio: 'inherit' });
  if (r.error) throw r.error;
  if (r.status !== 0) process.exit(r.status ?? 1);
}
const bindgen = resolve(web, '.tools/bin/wasm-bindgen');
if (!existsSync(bindgen) || spawnSync(bindgen, ['--version'], { encoding: 'utf8' }).stdout?.trim() !== 'wasm-bindgen 0.2.128') run('cargo', ['install', 'wasm-bindgen-cli', '--version', '0.2.128', '--locked', '--root', resolve(web, '.tools')]);
run('cargo', ['build', '--locked', '-p', 'tidkod-wasm', '--target', 'wasm32-unknown-unknown', '--release']);
for (const [target, dir] of [['web', 'pkg'], ['nodejs', 'pkg-node']]) {
  run(bindgen, [resolve(root, 'target/wasm32-unknown-unknown/release/tidkod_wasm.wasm'), '--target', target, '--out-dir', resolve(web, dir)]);
  if (target === 'nodejs') writeFileSync(resolve(web, dir, 'package.json'), '{"type":"commonjs"}\n');
}
