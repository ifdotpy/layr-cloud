import { access } from 'node:fs/promises';
try {
  for (const name of ['layr_web.js', 'layr_web.d.ts', 'layr_web_bg.wasm']) {
    await access(new URL(`../wasm/${name}`, import.meta.url));
  }
} catch {
  console.error('Run npm run build:wasm first (needs the wasm32 target and wasm-pack).');
  process.exit(1);
}
