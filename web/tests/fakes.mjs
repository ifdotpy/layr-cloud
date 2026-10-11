// What the site e2e tests run `layr-desk serve` against: a fake AWS (STS and path-style S3) and a
// fake tailscaled. tests/site.e2e.mjs and tests/person.e2e.mjs use them.
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import http from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** The keys of the fake AWS: the gateway's own, its reader role's, and a vault owner's. */
export const KEYS = {
  gateway: { id: "AKIAGATEWAY", secret: "gateway-secret" },
  role: { id: "ASIAROLE", secret: "role-secret", token: "role-token" },
  owner: { id: "AKIAOWNER", secret: "owner-secret" },
};

/**
 * Path-style S3 and STS in one fake. STS takes the gateway's key. The vault bucket "b" holds
 * `objects` and takes the reader role and the owner; the bucket "artifacts" holds `people` (the
 * person records) and takes the gateway's own key. A header sent twice is refused, as AWS joins
 * its values and the signature then fails.
 */
export function fakeAws({ objects, people = new Map() }) {
  const { gateway, role, owner } = KEYS;
  return http.createServer(async (req, res) => {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    const body = Buffer.concat(chunks);
    const url = new URL(req.url, "http://aws");
    const auth = String(req.headers.authorization ?? "");
    const reply = (status, text = "") => {
      res.writeHead(status, { "content-type": "application/xml" });
      res.end(text);
    };
    const error = (status, code) =>
      reply(status, `<Error><Code>${code}</Code></Error>`);
    const names = [];
    for (let i = 0; i < req.rawHeaders.length; i += 2)
      names.push(req.rawHeaders[i].toLowerCase());
    if (new Set(names).size !== names.length)
      return error(400, "HeaderSentTwice");
    const by = (key) =>
      auth.startsWith(`AWS4-HMAC-SHA256 Credential=${key.id}/`) &&
      (req.headers["x-amz-security-token"] ?? "") === (key.token ?? "");
    if (req.method === "POST" && url.pathname === "/") {
      const form = new URLSearchParams(body.toString());
      if (
        !by(gateway) ||
        form.get("Action") !== "AssumeRole" ||
        form.get("RoleSessionName") !== "vault-gateway"
      )
        return error(403, "AccessDenied");
      const expiration = new Date(Date.now() + 3600e3)
        .toISOString()
        .replace(/\.\d+Z$/, "Z");
      return reply(
        200,
        `<AssumeRoleResponse><AssumeRoleResult><Credentials><AccessKeyId>${role.id}</AccessKeyId>` +
          `<SecretAccessKey>${role.secret}</SecretAccessKey><SessionToken>${role.token}</SessionToken>` +
          `<Expiration>${expiration}</Expiration></Credentials></AssumeRoleResult></AssumeRoleResponse>`,
      );
    }
    const [, bucket, ...rest] = url.pathname.split("/");
    const key = decodeURIComponent(rest.join("/"));
    const store = { b: objects, artifacts: people }[bucket];
    if (!store) return error(404, "NoSuchBucket");
    const allowed = bucket === "b" ? by(role) || by(owner) : by(gateway);
    if (!allowed) return error(403, "AccessDenied");
    if (req.method === "GET" && !key) {
      const prefix = url.searchParams.get("prefix") ?? "";
      const keys = [...store.keys()].filter((k) => k.startsWith(prefix)).sort();
      return reply(
        200,
        `<ListBucketResult>${keys.map((k) => `<Contents><Key>${k}</Key></Contents>`).join("")}</ListBucketResult>`,
      );
    }
    if (req.method === "GET") {
      const data = store.get(key);
      if (!data) return error(404, "NoSuchKey");
      res.writeHead(200, { "content-length": data.length });
      return res.end(data);
    }
    if (req.method === "PUT") {
      if (req.headers["if-none-match"] === "*" && store.has(key))
        return error(412, "PreconditionFailed");
      store.set(key, body);
      return reply(200);
    }
    if (req.method === "DELETE") {
      store.delete(key);
      return reply(204);
    }
    return error(405, "MethodNotAllowed");
  });
}

/** A fake tailscaled on a Unix socket: whois names `login()` for every address. */
export function fakeTailscaled(socket, login) {
  const server = http.createServer((req, res) => {
    const url = new URL(req.url, "http://local-tailscaled.sock");
    const valid =
      req.headers.host === "local-tailscaled.sock" &&
      url.pathname === "/localapi/v0/whois" &&
      url.searchParams.get("addr");
    res.writeHead(valid ? 200 : 403, { "content-type": "application/json" });
    res.end(
      JSON.stringify(
        valid
          ? { Node: {}, UserProfile: { LoginName: login() } }
          : "invalid localapi request",
      ),
    );
  });
  return new Promise((resolve) => server.listen(socket, () => resolve(server)));
}

/**
 * `layr-desk serve` on 127.0.0.1 with the fake AWS, serving the vaults `names` (all with
 * `recipient`) and, with `socket`, person records through that tailscaled.
 */
export async function startRust(
  program,
  { objects, people, names, recipient, socket, site },
) {
  const aws = fakeAws({ objects, people });
  await new Promise((resolve) => aws.listen(0, "127.0.0.1", resolve));
  const endpoint = `http://127.0.0.1:${aws.address().port}`;
  const dir = mkdtempSync(join(tmpdir(), "layr-desk-e2e-"));
  const project = (name) => ({
    bucket: "b",
    region: "eu-central-1",
    role: `arn:aws:iam::123456789012:role/reader-${name}`,
    recipient,
  });
  writeFileSync(
    join(dir, "config.json"),
    JSON.stringify({
      listen: "127.0.0.1",
      projects: Object.fromEntries(names.map((n) => [n, project(n)])),
      ...(people
        ? { people: { bucket: "artifacts", region: "eu-central-1" } }
        : {}),
    }),
  );
  const child = spawn(
    program,
    [
      "serve",
      "--config",
      join(dir, "config.json"),
      "--site",
      site,
      "--tls",
      "none",
      "--port",
      "0",
      "--tailscale-socket",
      socket ?? join(dir, "no-tailscaled.sock"),
    ],
    {
      env: {
        ...process.env,
        AWS_ACCESS_KEY_ID: KEYS.gateway.id,
        AWS_SECRET_ACCESS_KEY: KEYS.gateway.secret,
        AWS_SESSION_TOKEN: "",
        AWS_ENDPOINT_URL_S3: endpoint,
        AWS_ENDPOINT_URL_STS: endpoint,
      },
      stdio: ["ignore", "inherit", "pipe"],
    },
  );
  const port = await new Promise((resolve, reject) => {
    let text = "";
    child.stderr.on("data", (chunk) => {
      process.stderr.write(chunk);
      text += chunk;
      const found = text.match(/listening on http:\/\/127\.0\.0\.1:(\d+)/);
      if (found) resolve(Number(found[1]));
    });
    child.on("exit", (code) =>
      reject(new Error(`layr-desk serve stopped (${code})`)),
    );
  });
  return {
    port,
    endpoint,
    stop: () => {
      child.kill();
      aws.close();
      rmSync(dir, { recursive: true, force: true });
    },
  };
}
