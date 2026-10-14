// Build the authenticated age reader for the browser worker.
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
const repo = fileURLToPath(new URL("../../", import.meta.url));
execFileSync(
  "cargo",
  [
    "build",
    "--locked",
    "--release",
    "-p",
    "desk-web",
    "--target",
    "wasm32-unknown-unknown",
  ],
  { cwd: repo, stdio: "inherit" },
);
execFileSync(
  "wasm-bindgen",
  [
    "--target",
    "web",
    "--out-dir",
    "web/src/generated",
    "target/wasm32-unknown-unknown/release/desk_web.wasm",
  ],
  { cwd: repo, stdio: "inherit" },
);
