import { spawnSync } from 'node:child_process';
const root = new URL('../../', import.meta.url);
const result = spawnSync('wasm-pack', ['build', 'crates/layr-web', '--target', 'web', '--out-dir', '../../web/wasm', '--release', '--no-opt'], { cwd: root, stdio: 'inherit' });
process.exit(result.status ?? 1);
