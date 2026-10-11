// The person key on the site, behind `layr-desk serve` with the fake AWS and a fake tailscaled
// (tests/fakes.mjs), in headless Chromium with a virtual authenticator that has PRF. Ann makes
// her person key with `layr-desk person keys`; the owner grants her two vaults with
// `layr-desk grant`. On the site she unlocks the person key with the words, accepts both invites,
// opens both vaults, adds a passkey, and after a reload one passkey touch opens them again. A
// vault.json whose writer key or recipient differs from the pin is refused. The gateway shows the
// vaults to nobody else, and `layr-desk person vaults` reads the index that the site wrote.
//
//   cargo build -p layr-desk && pnpm --filter layr-desk-web run build &&
//     LAYR_DESK=../target/debug/layr-desk node tests/person.e2e.mjs
import { chromium } from "playwright";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import http from "node:http";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import {
  generateIdentity,
  recipientOf,
  sealSlot,
  wordsOfIdentity,
} from "../../js/vault/src/keys.mjs";
import { sign, vectors } from "../../js/vault/test/signed.mjs";
import { KEYS, fakeTailscaled, startRust } from "./fakes.mjs";

const program = process.env.LAYR_DESK;
if (!program) throw new Error("set LAYR_DESK to the layr-desk program");
const ANN = "ann@example.com";
const OWNER = "owner@example.com";
const dist = new URL("../dist/", import.meta.url).pathname;
const fixture = new URL("../../js/vault/test/fixture/", import.meta.url)
  .pathname;
const vaultIdentity = readFileSync(
  join(fixture, "identity.txt"),
  "utf8",
).trim();
const recipient = await recipientOf(vaultIdentity);

// Two vaults of the writer's fixture: "proj" signs from state 2 on, "other" has no signatures.
const objects = new Map();
const walk = (dir) =>
  readdirSync(dir).forEach((n) => {
    if (statSync(join(dir, n)).isDirectory()) return walk(join(dir, n));
    const key = relative(join(fixture, "objects"), join(dir, n));
    objects.set(key, readFileSync(join(dir, n)));
    objects.set(key.replace(/^proj\//, "other/"), readFileSync(join(dir, n)));
  });
walk(join(fixture, "objects"));
await sign(objects, "proj");
const paper = await generateIdentity();
const paperSlot = Buffer.from(
  await sealSlot(vaultIdentity, await recipientOf(paper)),
);
const records = {
  proj: { writer: vectors.writer, signedFrom: vectors.signedFrom },
  other: {},
};
const vaultJson = (name, more = {}) =>
  Buffer.from(
    JSON.stringify({
      format: 1,
      recipient,
      created: 1,
      ...records[name],
      ...more,
    }),
  );
for (const name of ["proj", "other"]) {
  objects.set(`${name}/slots/paper.age`, paperSlot);
  objects.set(`${name}/vault.json`, vaultJson(name));
}

const people = new Map();
const dir = mkdtempSync(join(tmpdir(), "layr-desk-person-"));
let login = ANN;
const tailscaled = await fakeTailscaled(join(dir, "ts.sock"), () => login);
const { port, endpoint, stop } = await startRust(program, {
  objects,
  people,
  names: ["proj", "other"],
  recipient,
  socket: join(dir, "ts.sock"),
  site: dist,
});
const site = `http://localhost:${port}`;

// The owner's Mac: an AWS profile with keys, and the vault folders that `keys` and `deploy` make.
const home = join(dir, "home");
mkdirSync(join(home, ".aws"), { recursive: true });
writeFileSync(
  join(home, ".aws/credentials"),
  `[owner]\naws_access_key_id = ${KEYS.owner.id}\naws_secret_access_key = ${KEYS.owner.secret}\n`,
);
writeFileSync(
  join(home, ".aws/config"),
  "[profile owner]\nregion = eu-central-1\n",
);
for (const name of ["proj", "other"]) {
  const folder = join(home, ".config/studio-vault", name);
  mkdirSync(folder, { recursive: true });
  writeFileSync(join(folder, "recipient.txt"), `${recipient}\n`);
  writeFileSync(join(folder, "paper.age"), paperSlot);
  writeFileSync(
    join(folder, "cloud.json"),
    JSON.stringify({
      profile: "owner",
      region: "eu-central-1",
      bucket: "b",
      site: `${site}/${name}/`,
    }),
  );
}
/** Run layr-desk; asynchronously, as the fakes of this process answer its calls. */
function cli(...args) {
  const child = spawn(program, args, {
    env: { PATH: process.env.PATH, HOME: home, AWS_ENDPOINT_URL_S3: endpoint },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (c) => (stdout += c));
  child.stderr.on("data", (c) => (stderr += c));
  return new Promise((resolve) =>
    child.on("close", (status) =>
      resolve({ status, stdout, stderr, ok: status === 0 }),
    ),
  );
}
async function ok(...args) {
  const run = await cli(...args);
  assert.ok(run.ok, `${args.join(" ")}: ${run.stderr}`);
  return run.stdout;
}
/** The 24 numbered words that a command showed, in their order. */
function wordsIn(text) {
  const found = [...text.matchAll(/(\d+)\. ([a-z]+)/g)]
    .map(([, n, word]) => [Number(n), word])
    .sort((a, b) => a[0] - b[0]);
  assert.equal(found.length, 24, text);
  return found.map(([, word]) => word).join(" ");
}
/** A request on a connection of its own: the gateway asks tailscaled once per connection. */
const projectsAs = (who) =>
  new Promise((resolve, reject) => {
    login = who;
    http
      .get(`${site}/api/projects`, { agent: false }, (res) => {
        let text = "";
        res.on("data", (c) => (text += c));
        res.on("end", () => resolve(JSON.parse(text).projects));
      })
      .on("error", reject);
  });

const browser = await chromium.launch();
try {
  // Ann makes her person key in her terminal; the owner grants her both vaults.
  const shown = await ok("person", "keys", "--site", site);
  const annWords = wordsIn(shown);
  const annRecipient = shown.match(/^recipient: (age1\S+)$/m)[1];
  assert.match(shown, new RegExp(`registered at ${site} as ${ANN}`));
  const vaultWords = join(dir, "vault-words.txt");
  writeFileSync(vaultWords, wordsOfIdentity(paper));
  login = OWNER;
  for (const name of ["proj", "other"])
    await ok(
      "grant",
      "--project",
      name,
      "--person",
      ANN,
      "--recipient",
      annRecipient,
      "--paper-file",
      vaultWords,
    );
  assert.deepEqual(await projectsAs(ANN), ["other", "proj"]);
  assert.deepEqual(await projectsAs("bob@example.com"), []);
  login = ANN;

  const context = await browser.newContext();
  const page = await context.newPage();
  const cdp = await context.newCDPSession(page);
  await cdp.send("WebAuthn.enable");
  await cdp.send("WebAuthn.addVirtualAuthenticator", {
    options: {
      protocol: "ctap2",
      transport: "internal",
      hasResidentKey: true,
      hasUserVerification: true,
      isUserVerified: true,
      hasPrf: true,
      automaticPresenceSimulation: true,
    },
  });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const versions = () =>
    page.$$eval("#state option", (options) =>
      options.map((o) => o.textContent),
    );
  const opened = () =>
    page.waitForSelector("#columns:not([hidden]), #list:not([hidden])");

  // The person box is locked; the vault has no passkey, so nothing asks without a click.
  await page.goto(`${site}/proj/`);
  await page.waitForSelector("#person:not([hidden])");
  assert.equal(await page.textContent("#person-login"), ANN);
  assert.equal(await page.textContent("#person-state"), "Locked");
  assert.equal(await page.isHidden("#person-unlock"), true);
  await page.waitForSelector("#unlock:not([hidden])");

  // Another person key's words are refused; Ann's words unlock it, and two invites wait.
  await page.click("#person-paper-open");
  await page.fill("#person-paper", wordsOfIdentity(await generateIdentity()));
  await page.click("#person-paper-form button[type=submit]");
  await page.waitForFunction(
    () => document.querySelector("#person-paper-error").textContent !== "",
  );
  assert.equal(await page.textContent("#person-state"), "Locked");
  await page.fill("#person-paper", annWords);
  await page.click("#person-paper-form button[type=submit]");
  await page.waitForSelector("#person-paper-panel", { state: "hidden" });
  await page.waitForSelector("#invites-open:has-text('Invites (2)')");
  assert.equal(await page.textContent("#person-state"), "Unlocked: 0 vaults");

  // Accept both: the site checks each against the gateway first. The open vault opens at once.
  await page.click("#invites-open");
  await page.waitForSelector(
    "#invite-list li:has-text('from owner@example.com')",
  );
  for (const name of ["proj", "other"]) {
    await page.click(
      `#invite-list li:has-text('${name}, from') button:has-text('Accept')`,
    );
    await page.waitForFunction(
      (n) =>
        ![...document.querySelectorAll("#invite-list li")].some((li) =>
          li.textContent.includes(`${n}, from`),
        ),
      name,
    );
  }
  assert.equal(await page.textContent("#invite-error"), "");
  await page.click("#close-invites");
  await opened();
  assert.deepEqual(await versions(), [
    "Latest (#4)",
    "#3",
    "#2",
    "#1 · unsigned",
  ]);
  assert.equal(await page.isHidden("#invites-open"), true);
  assert.equal(await page.textContent("#person-state"), "Unlocked: 2 vaults");
  assert.ok(people.has(`people/${ANN}/index.age`));

  // The other vault opens in the same page, without another unlock.
  await page.click("#vaults a:has-text('other')");
  await page.waitForURL(`${site}/other/`);
  await page.waitForFunction(() =>
    [...document.querySelectorAll("#state option")].every((o) =>
      o.textContent.endsWith(" · unsigned"),
    ),
  );
  await opened();
  assert.equal(await page.isHidden("#unlock"), true);

  // A passkey for the person key; after a reload, one touch opens the vault again.
  await page.click("#person-keys");
  await page.fill("#person-label", "test device");
  await page.click("#person-add-form button");
  await page.waitForSelector("#person-passkey-list li button");
  assert.ok(
    [...people.keys()].some((k) =>
      k.startsWith(`people/${ANN}/slots/passkey-`),
    ),
  );
  await page.click("#close-person");
  await page.reload();
  await opened();
  assert.equal(await page.textContent("#person-state"), "Unlocked: 2 vaults");
  assert.equal(await page.isHidden("#unlock"), true);

  // A vault.json with another writer key than the pin is refused, and nothing else opens it.
  objects.set(
    "proj/vault.json",
    vaultJson("proj", { writer: Buffer.alloc(32, 8).toString("base64") }),
  );
  await page.click("#vaults a:has-text('proj')");
  await page.waitForSelector(
    "#empty-text:has-text('differs from your pin (writer key)')",
  );
  assert.equal(await page.isHidden("#unlock"), true);
  assert.equal(await page.isHidden("#columns"), true);
  assert.equal(await page.isHidden("#list"), true);
  objects.set("proj/vault.json", vaultJson("proj"));
  await page.click("#vaults a:has-text('other')");
  await opened();
  await page.click("#vaults a:has-text('proj')");
  await page.waitForFunction(
    () =>
      document.querySelector("#state option")?.textContent === "Latest (#4)",
  );
  await opened();

  // A vault.json with another recipient: refused too.
  objects.set(
    "other/vault.json",
    vaultJson("other", {
      recipient: await recipientOf(await generateIdentity()),
    }),
  );
  await page.click("#vaults a:has-text('other')");
  await page.waitForSelector(
    "#empty-text:has-text('differs from your pin (recipient)')",
  );
  objects.set("other/vault.json", vaultJson("other"));

  // The command line reads the index that the site wrote, with the same MAC and pins.
  const annPaper = join(dir, "ann-words.txt");
  writeFileSync(annPaper, annWords);
  const checked = await ok(
    "person",
    "vaults",
    "--site",
    site,
    "--paper-file",
    annPaper,
  );
  assert.match(checked, /^other ok$/m);
  assert.match(checked, /^proj ok$/m);

  // Lock forgets the person key: the vault waits for an unlock again.
  await page.click("#lock");
  await page.waitForSelector("#person-unlock:not([hidden])");
  assert.equal(await page.textContent("#person-state"), "Locked");
  assert.deepEqual(errors, []);
  console.log("person e2e: passed (layr-desk serve)");
} finally {
  await browser.close();
  stop();
  tailscaled.close();
  rmSync(dir, { recursive: true, force: true });
}
