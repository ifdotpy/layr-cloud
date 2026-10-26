#!/usr/bin/env python3
"""Install a Layr release and converge an IPv6 host. See infra/aws/README.md."""

from __future__ import annotations

import hashlib
import fcntl
import io
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

AWS_ENV = {
    "AWS_EC2_METADATA_SERVICE_ENDPOINT_MODE": "IPv6",
    "AWS_USE_DUALSTACK_ENDPOINT": "true",
    "AWS_ENDPOINT_URL_ROUTE_53": "https://route53.global.api.aws",
    "AWS_DEFAULT_REGION": "us-east-1",
}
for name, value in AWS_ENV.items():
    os.environ.setdefault(name, value)
import boto3  # noqa: E402

UNITS = Path("/etc/systemd/system")
ENV = "".join(f"Environment={name}={value}\n" for name, value in AWS_ENV.items())


def run(*args: str, check: bool = True, pass_fds: tuple[int, ...] = ()) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, check=check, text=True, capture_output=True, pass_fds=pass_fds)


def write_if_changed(path: Path, text: str, mode: int = 0o644) -> bool:
    if path.exists() and path.read_text() == text:
        return False
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile("w", dir=path.parent, delete=False) as tmp:
        tmp.write(text)
    os.chmod(tmp.name, mode)
    os.replace(tmp.name, path)
    return True


def release_files(data: bytes, digest: str, binary_name: str, binary_sha256: str | None = None,
                  binary_bytes: int | None = None) -> dict[str, bytes]:
    """Reject an invalid hash, extra files, links and non-static ARM64 programs."""
    if not re.fullmatch(r"[0-9a-f]{64}", digest) or hashlib.sha256(data).hexdigest() != digest:
        raise ValueError("release SHA-256 mismatch")
    if binary_name not in ("layr", "layr-gateway"):
        raise ValueError("unknown release binary")
    files = {}
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        for member in archive:
            if member.name == "." and member.isdir():
                continue
            name = member.name.removeprefix("./")
            site_file = (binary_name == "layr-gateway" and (name == "site" or name.startswith("site/"))
                         and all(re.fullmatch(r"[A-Za-z0-9_-][A-Za-z0-9_.-]*", part)
                                 for part in name.split("/")))
            if site_file and member.isdir():
                continue
            host_script = binary_name == "layr" and name == "checks.py"
            if (name not in (binary_name, "sync.py") and not site_file and not host_script) or not member.isfile() or name in files:
                raise ValueError(f"release must contain only regular {binary_name} and sync.py files")
            if member.size > 32 * 1024 * 1024 or sum(map(len, files.values())) + member.size > 128 * 1024 * 1024:
                raise ValueError("release exceeds size limit")
            files[name] = archive.extractfile(member).read()
    if not {binary_name, "sync.py"}.issubset(files):
        raise ValueError(f"release lacks {binary_name} or sync.py")
    binary = files[binary_name]
    validate_binary(binary, binary_sha256, binary_bytes)
    compile(files["sync.py"], "sync.py", "exec")
    if "checks.py" in files:
        compile(files["checks.py"], "checks.py", "exec")
    return files


def validate_binary(binary: bytes, expected_sha256: str | None = None,
                    expected_bytes: int | None = None) -> None:
    if expected_sha256 is not None or expected_bytes is not None:
        if (not isinstance(expected_sha256, str) or not re.fullmatch(r"[0-9a-f]{64}", expected_sha256)
                or type(expected_bytes) is not int or not 0 < expected_bytes <= 32 * 1024 * 1024):
            raise ValueError("invalid build-machine binary metadata")
        if len(binary) != expected_bytes or hashlib.sha256(binary).hexdigest() != expected_sha256:
            raise ValueError("binary differs from build-machine size or SHA-256")
    if (len(binary) < 64 or binary[:6] != b"\x7fELF\x02\x01"
            or int.from_bytes(binary[18:20], "little") != 183):
        raise ValueError("layr must be a Linux ARM64 ELF program")
    offset = int.from_bytes(binary[32:40], "little")
    size = int.from_bytes(binary[54:56], "little")
    count = int.from_bytes(binary[56:58], "little")
    if size < 56 or count == 0 or offset + size * count > len(binary):
        raise ValueError("invalid ELF program headers")
    for index in range(count):
        at = offset + index * size
        kind = int.from_bytes(binary[at:at + 4], "little")
        if kind == 3:
            raise ValueError("layr must be static (no ELF interpreter)")
        segment_offset = int.from_bytes(binary[at + 8:at + 16], "little")
        file_size = int.from_bytes(binary[at + 32:at + 40], "little")
        memory_size = int.from_bytes(binary[at + 40:at + 48], "little")
        if segment_offset + file_size > len(binary):
            raise ValueError("truncated ELF segment")
        if kind == 1 and file_size > memory_size:
            raise ValueError("invalid ELF load segment")


class Sync:
    def __init__(self, source: Path) -> None:
        self.source_path = source
        self.source = json.loads(source.read_text())
        self.kind = self.source["kind"]
        if self.kind not in ("gateway", "executor"):
            raise ValueError("unknown host kind")
        self.home = source.parent
        self.app = Path(f"/opt/layr-{self.kind}")
        self.s3 = boto3.client("s3", region_name=self.source["region"])
        self.bucket = self.source["bucket"]
        self.restart = False

    def get(self, key: str) -> bytes:
        return self.s3.get_object(Bucket=self.bucket, Key=key)["Body"].read()

    def tailnet(self) -> str:
        state = json.loads(run("tailscale", "status", "--json", check=False).stdout or "{}")
        if state.get("BackendState") != "Running":
            ssm = boto3.client("ssm", region_name=self.source["region"])
            key = ssm.get_parameter(Name=self.source["authKeyParameter"], WithDecryption=True)
            read_fd, write_fd = os.pipe()
            try:
                value = key["Parameter"]["Value"].encode()
                if len(value) > 4096:
                    raise ValueError("tailnet key exceeds size limit")
                os.write(write_fd, value)
                os.close(write_fd)
                write_fd = -1
                run("tailscale", "up", f"--auth-key=file:/proc/self/fd/{read_fd}",
                    f"--hostname={self.source.get('hostname', 'layr-' + self.kind)}", f"--advertise-tags={self.source.get('tailnetTag', 'tag:layr-' + self.kind)}", "--ssh",
                    pass_fds=(read_fd,))
            finally:
                os.close(read_fd)
                if write_fd >= 0:
                    os.close(write_fd)
        address = run("tailscale", "ip", "-4").stdout.strip()
        # Never start a server without an explicit tailnet address.
        import ipaddress
        if ipaddress.ip_address(address) not in ipaddress.ip_network("100.64.0.0/10"):
            raise ValueError("no tailnet IPv4 address")
        return address

    def address(self) -> str:
        if self.source.get("network") != "public-ipv6":
            return self.tailnet()
        import ipaddress
        interfaces = json.loads(run("ip", "-j", "-6", "address", "show", "scope", "global").stdout)
        for interface in interfaces:
            for address in interface.get("addr_info", []):
                ip = ipaddress.ip_address(address["local"])
                if ip.version == 6 and ip.is_global:
                    return str(ip)
        raise ValueError("no global IPv6 address")

    def release(self) -> None:
        release = json.loads(self.get(f"{self.kind}/release.json"))
        digest = release["sha256"]
        if not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise ValueError("invalid release SHA-256")
        if release["key"] != f"{self.kind}/releases/{digest}.tgz":
            raise ValueError("invalid release key")
        current = self.app / "current"
        binary_name = "layr-gateway" if self.kind == "gateway" else "layr"
        binary_sha256, binary_bytes = release.get("binarySha256"), release.get("binaryBytes")
        if (current / ".sha256").exists() and (current / ".sha256").read_text() == digest:
            if binary_sha256 is not None or binary_bytes is not None:
                validate_binary((current / binary_name).read_bytes(), binary_sha256, binary_bytes)
            return
        files = release_files(self.get(release["key"]), digest, binary_name, binary_sha256, binary_bytes)
        target = self.app / "releases" / digest
        if target.exists():
            shutil.rmtree(target)
        target.mkdir(parents=True, mode=0o755)
        for name, content in files.items():
            (target / name).parent.mkdir(parents=True, exist_ok=True)
            (target / name).write_bytes(content)
            (target / name).chmod(0o755 if name == binary_name else 0o644)
        (target / ".sha256").write_text(digest)
        link = self.app / "current.new"
        link.unlink(missing_ok=True)
        link.symlink_to(target)
        # Update the stable entry point before the atomic program switch.
        write_if_changed(Path("/usr/local/lib/layr-cloud-sync.py"), files["sync.py"].decode())
        os.replace(link, current)
        releases = sorted((self.app / "releases").iterdir(), key=lambda p: p.stat().st_mtime)
        for old in releases[:-2]:
            if old != target:
                shutil.rmtree(old)
        self.restart = True
        print("installed release", digest, flush=True)

    def executor_identity(self) -> None:
        run("systemctl", "start", "layr-data.service")
        result = json.loads(run("/usr/bin/env", "LAYR_ROOT=/var/lib/layr",
                                str(self.app / "current/layr"), "remote", "self", "--json").stdout)
        # Copy only the public fields into the file and console log.
        import uuid
        machine = str(uuid.UUID(result["id"]))
        public_key = result["public_key"]
        if not re.fullmatch(r"[0-9a-f]{64}", public_key):
            raise ValueError("invalid executor public key")
        identity = json.dumps({"id": machine, "public_key": public_key}, sort_keys=True) + "\n"
        if write_if_changed(self.home / "machine.json", identity):
            print("executor public identity:", identity.strip(), flush=True)

    def configure(self, listen: str) -> dict:
        gateway = json.loads(self.get("gateway/gateway.json"))
        remote_gateway = dict(gateway)
        projects = {}
        for page in self.s3.get_paginator("list_objects_v2").paginate(Bucket=self.bucket, Prefix="gateway/projects/"):
            for item in page.get("Contents", []):
                key = item["Key"]
                if not key.endswith(".json"):
                    continue
                name = key.removeprefix("gateway/projects/").removesuffix(".json")
                if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,43}", name):
                    raise ValueError("invalid project name")
                projects[name] = json.loads(self.get(key))
        if gateway.get("tls") == "tailscale":
            state = json.loads(run("tailscale", "status", "--json").stdout)
            gateway["host"] = (state.get("Self") or {}).get("DNSName", "").rstrip(".")
            if not gateway["host"].endswith(".ts.net"):
                raise ValueError("enable MagicDNS for the tailnet")
        remote_digest = hashlib.sha256(json.dumps(
            {"gateway": remote_gateway, "projects": projects}, sort_keys=True).encode()).hexdigest()
        descriptor = os.open(self.home / ".config.json.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
        with os.fdopen(descriptor, "a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            marker = self.home / ".remote-config-sha256"
            path = self.home / "config.json"
            # Keep locally issued hashes while the remote config is unchanged.
            if marker.exists() and marker.read_text() == remote_digest and path.exists():
                old_projects = json.loads(path.read_text()).get("projects", {})
                for name, project in projects.items():
                    for field in ("writeTokenHashes", "gcTokenHashes"):
                        if field in old_projects.get(name, {}):
                            project[field] = old_projects[name][field]
            config = {"host": gateway["host"], "listen": f"[{listen}]:443" if ":" in listen else f"{listen}:443", "projects": projects,
                      "people": {"bucket": self.bucket, "region": self.source["region"], "prefix": "people/"}}
            if self.source.get("network") == "public-ipv6":
                config["sessionSecretRegion"] = self.source["region"]
            if gateway.get("cloudfront"):
                config["originSecretRegion"] = self.source["region"]
                config["publicHost"] = gateway["publicHost"]
            if gateway.get("cdn"):
                if not gateway.get("cloudfront"):
                    raise ValueError("CDN requires the guarded CloudFront origin")
                config["cdn"] = gateway["cdn"]
            app = getattr(self, "app", Path("/opt/layr-gateway"))
            if (app / "current/site/index.html").is_file():
                config["siteDir"] = str(app / "current/site")
            if write_if_changed(path, json.dumps(config, indent=2, sort_keys=True) + "\n"):
                self.restart = True
            write_if_changed(marker, remote_digest)
        return gateway

    def session_secret(self) -> None:
        ssm = boto3.client("ssm", region_name=self.source["region"])
        name = "/layr-store/gateway/session-secret"
        try:
            value = ssm.get_parameter(Name=name, WithDecryption=True)["Parameter"]["Value"]
        except ssm.exceptions.ParameterNotFound:
            import secrets
            try:
                ssm.put_parameter(Name=name, Type="SecureString", Value=secrets.token_hex(32), Overwrite=False)
            except ssm.exceptions.ParameterAlreadyExists:
                pass
            value = ssm.get_parameter(Name=name, WithDecryption=True)["Parameter"]["Value"]
        if not re.fullmatch(r"[0-9a-f]{64}", value):
            raise ValueError("invalid gateway session secret")

    def tailscale_certificate(self, host: str) -> tuple[str, str]:
        tls = self.home / "tls"
        tls.mkdir(mode=0o700, parents=True, exist_ok=True)
        cert, key = tls / f"{host}.crt", tls / f"{host}.key"
        before = cert.read_bytes() if cert.exists() else b""
        run("tailscale", "cert", "--cert-file", str(cert), "--key-file", str(key), host)
        os.chmod(key, 0o600)
        if cert.read_bytes() != before:
            self.restart = True
        return str(cert), str(key)

    def dns(self, gateway: dict, listen: str) -> None:
        route53 = boto3.client("route53")
        record_type = "AAAA" if ":" in listen else "A"
        host = gateway["host"].rstrip(".") + "."
        found = route53.list_resource_record_sets(
            HostedZoneId=gateway["zone"], StartRecordName=host, StartRecordType=record_type, MaxItems="1"
        )["ResourceRecordSets"]
        if found and found[0]["Name"] == host and found[0]["Type"] == record_type:
            if [r["Value"] for r in found[0]["ResourceRecords"]] == [listen]:
                return
        route53.change_resource_record_sets(HostedZoneId=gateway["zone"], ChangeBatch={
            "Comment": "Layr gateway address", "Changes": [{"Action": "UPSERT",
            "ResourceRecordSet": {"Name": host, "Type": record_type, "TTL": 300,
                                  "ResourceRecords": [{"Value": listen}]}}]})

    def certificate(self, host: str) -> tuple[str, str]:
        live = f"/etc/letsencrypt/live/{host}"
        if not Path(f"{live}/fullchain.pem").exists():
            run("certbot", "certonly", "--dns-route53", "--non-interactive", "--agree-tos",
                "--register-unsafely-without-email", "--key-type", "ecdsa", "-d", host)
        write_if_changed(Path("/etc/letsencrypt/renewal-hooks/deploy/layr-gateway"),
                         "#!/bin/sh\nsystemctl try-restart layr-gateway.service\n", 0o755)
        return f"{live}/fullchain.pem", f"{live}/privkey.pem"

    def services(self, pair: tuple[str, str] | None = None) -> None:
        name = f"layr-{self.kind}-sync"
        changed = write_if_changed(UNITS / f"{name}.service", f"""[Unit]
Description=Layr {self.kind} release sync
After=network-online.target
Wants=network-online.target
[Service]
Type=oneshot
TimeoutStartSec=10min
{ENV}ExecStart=/usr/bin/python3 /usr/local/lib/layr-cloud-sync.py {self.source_path}
""")
        changed |= write_if_changed(UNITS / f"{name}.timer", """[Timer]
OnBootSec=1min
OnUnitActiveSec=5min
[Install]
WantedBy=timers.target
""")
        if pair:
            cert, key = pair
            changed |= write_if_changed(UNITS / "certbot.service.d/layr-gateway.conf", f"[Service]\n{ENV}")
            if write_if_changed(UNITS / "layr-gateway.service", f"""[Unit]
Description=Layr gateway
After=network-online.target
Wants=network-online.target
[Service]
ExecStart={self.app}/current/layr-gateway serve --config /etc/layr-gateway/config.json
{ENV}LoadCredential=tls.crt:{cert}
LoadCredential=tls.key:{key}
DynamicUser=yes
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
RestrictNamespaces=yes
LockPersonality=yes
SystemCallArchitectures=native
Restart=always
RestartSec=5
[Install]
WantedBy=multi-user.target
"""):
                changed = self.restart = True
        if changed:
            run("systemctl", "daemon-reload")
        run("systemctl", "enable", "--now", f"{name}.timer")
        unit = "layr-gateway.service" if self.kind == "gateway" else "layr.service"
        if self.kind == "gateway" and pair is None:
            return
        if self.kind == "executor" and not Path("/etc/layr-executor/ready").exists():
            return
        digest = (self.app / "current/.sha256").read_text()
        active_release = self.home / ".active-release-sha256"
        release_changed = not active_release.exists() or active_release.read_text() != digest
        if self.restart or release_changed or run("systemctl", "is-active", unit, check=False).stdout.strip() != "active":
            run("systemctl", "enable", unit)
            run("systemctl", "restart", unit)
            write_if_changed(active_release, digest)

    def execute(self) -> int:
        # A missing explicit network address fails closed before a server can bind.
        listen = self.address()
        self.release()
        pair = None
        if self.kind == "gateway":
            if self.source.get("network") == "public-ipv6":
                self.session_secret()
            gateway = self.configure(listen)
            if gateway.get("tls") == "tailscale":
                pair = self.tailscale_certificate(gateway["host"])
            else:
                self.dns(gateway, listen)
                pair = self.certificate(gateway["host"])
        else:
            self.executor_identity()
        self.services(pair)
        return 0


if __name__ == "__main__":
    try:
        sys.exit(Sync(Path(sys.argv[1] if len(sys.argv) > 1 else "/etc/layr-gateway/source.json")).execute())
    except Exception as error:
        print(f"sync failed: {type(error).__name__}: {error}", file=sys.stderr)
        sys.exit(1)
