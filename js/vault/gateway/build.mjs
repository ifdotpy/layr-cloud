// Build a gateway release (docs/design.md, "Gateway"): the layr-desk program for the gateway host
// (static, Linux on arm64), the site and sync.py in dist/gateway.tgz. Prints its SHA-256.
//
// cargo-zigbuild builds the program on a Mac or on Linux, once these are in place:
//   rustup target add aarch64-unknown-linux-musl
//   brew install zig    (or a zig release on PATH)
//   cargo install --locked cargo-zigbuild
// LAYR_DESK_GATEWAY_PROGRAM=<file> takes a program that was built elsewhere instead.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmodSync, cpSync, mkdirSync, readFileSync, readdirSync, rmSync } from "node:fs";

const TARGET = "aarch64-unknown-linux-musl";
const here = new URL(".", import.meta.url).pathname;
const out = new URL("../dist/gateway/", import.meta.url).pathname;
const site = new URL("../../../web/", import.meta.url).pathname;
const repo = new URL("../../../", import.meta.url).pathname;
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
execFileSync("pnpm", ["run", "build"], {
  cwd: site,
  stdio: ["ignore", "ignore", "inherit"],
});

let program = process.env.LAYR_DESK_GATEWAY_PROGRAM;
if (!program) {
  // Only the SHA-256 goes to stdout; cargo's output goes to stderr.
  execFileSync(
    "cargo",
    [
      "zigbuild",
      "--release",
      "--locked",
      "--target",
      TARGET,
      "-p",
      "layr-desk",
    ],
    {
      cwd: repo,
      stdio: ["ignore", 2, "inherit"],
      env: {
        ...process.env,
        CARGO_PROFILE_RELEASE_DEBUG: "0",
        CARGO_PROFILE_RELEASE_STRIP: "symbols",
      },
    },
  );
  program = `${repo}target/${TARGET}/release/layr-desk`;
}
checkProgram(readFileSync(program));
cpSync(program, `${out}layr-desk`);
chmodSync(`${out}layr-desk`, 0o755);
cpSync(`${site}dist`, `${out}site`, { recursive: true });
cpSync(`${here}sync.py`, `${out}sync.py`);
// The service uses DynamicUser. Build output must remain readable with a private umask.
function siteModes(path) {
  chmodSync(path, 0o755);
  for (const entry of readdirSync(path, { withFileTypes: true })) {
    const child = `${path}/${entry.name}`;
    if (entry.isDirectory()) siteModes(child);
    else chmodSync(child, 0o644);
  }
}
chmodSync(out, 0o755);
siteModes(`${out}site`);
chmodSync(`${out}sync.py`, 0o644);
const archive = new URL("../dist/gateway.tgz", import.meta.url).pathname;
execFileSync("tar", ["-czf", archive, "-C", out, "."], {
  env: { ...process.env, COPYFILE_DISABLE: "1" },
});
console.log(createHash("sha256").update(readFileSync(archive)).digest("hex"));

/** The host runs a 64-bit arm64 Linux ELF without an interpreter (a static program). */
function checkProgram(data) {
  const elf =
    data.length > 64 &&
    data.readUInt32BE(0) === 0x7f454c46 &&
    data[4] === 2 &&
    data[5] === 1 &&
    data.readUInt16LE(18) === 183;
  if (!elf) throw new Error(`${program} is not a Linux program for arm64`);
  const [offset, size, count] = [
    Number(data.readBigUInt64LE(32)),
    data.readUInt16LE(54),
    data.readUInt16LE(56),
  ];
  for (let i = 0; i < count; i++)
    if (data.readUInt32LE(offset + i * size) === 3)
      throw new Error(`${program} is not static: it names an interpreter`);
}
