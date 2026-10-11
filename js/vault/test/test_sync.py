"""The gateway's sync round without AWS, tailscale or systemd: a gateway with a domain gets
its certificate from certbot, one without a domain serves its ts.net name with a certificate
from the tailnet, and both run layr-desk serve with that certificate."""
from __future__ import annotations

import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import types
import unittest
from unittest.mock import patch

sys.modules.setdefault("boto3", types.ModuleType("boto3"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "gateway"))
import sync  # noqa: E402


class FakeS3:
    def __init__(self, objects: dict[str, bytes]):
        self.objects = objects

    def get_object(self, Bucket: str, Key: str) -> dict:
        return {"Body": io.BytesIO(self.objects[Key])}

    def get_paginator(self, name: str):
        objects = self.objects

        class Pages:
            def paginate(self, Bucket: str, Prefix: str):
                return [{"Contents": [{"Key": k} for k in objects if k.startswith(Prefix)]}]
        return Pages()


class Round(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        root = Path(self.temp.name)
        source = root / "source.json"
        source.write_text(json.dumps({"bucket": "b", "region": "eu-central-1", "authKeyParameter": "/k"}))
        self.units, self.tls, self.calls = root / "units", root / "tls", []
        self.patches = [
            patch.object(sync, "SOURCE", source), patch.object(sync, "CONFIG", root / "config.json"),
            patch.object(sync, "UNITS", self.units), patch.object(sync, "TLS", self.tls),
            patch.object(sync, "HOOK", root / "hook"), patch.object(sync, "run", self.run_command),
        ]
        for p in self.patches:
            p.start()

    def tearDown(self) -> None:
        for p in self.patches:
            p.stop()
        self.temp.cleanup()

    def run_command(self, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
        self.calls.append(args)
        out = ""
        if args[:3] == ("tailscale", "status", "--json"):
            out = json.dumps({"BackendState": "Running", "Self": {"DNSName": "vault-gateway.tail1234.ts.net."}})
        elif args[:2] == ("tailscale", "ip"):
            out = "100.64.0.7\n"
        elif args[:2] == ("tailscale", "cert"):
            cert, key = args[args.index("--cert-file") + 1], args[args.index("--key-file") + 1]
            Path(cert).write_text("CERT")
            Path(key).write_text("KEY")
        elif args[:2] == ("systemctl", "is-active"):
            out = "inactive\n"
        return subprocess.CompletedProcess(args, 0, out, "")

    def sync_with(self, gateway: dict) -> sync.Sync:
        round_ = sync.Sync.__new__(sync.Sync)
        round_.source = json.loads(sync.SOURCE.read_text())
        round_.bucket, round_.restart = "b", False
        round_.s3 = FakeS3({"gateway/gateway.json": json.dumps(gateway).encode(),
                            "gateway/projects/p.json": b'{"bucket": "vb"}'})
        round_.release = lambda: None
        return round_

    def test_a_gateway_without_a_domain_serves_its_tailnet_name(self) -> None:
        round_ = self.sync_with({"tls": "tailscale"})
        round_.dns = lambda *a: self.fail("no Route 53 without a domain")
        self.assertEqual(round_.run(), 0)
        host = "vault-gateway.tail1234.ts.net"
        self.assertEqual(json.loads(sync.CONFIG.read_text())["host"], host)
        unit = (self.units / "vault-gateway.service").read_text()
        self.assertIn(f"LoadCredential=tls.crt:{self.tls}/{host}.crt", unit)
        self.assertIn(f"LoadCredential=tls.key:{self.tls}/{host}.key", unit)
        self.assertIn(("systemctl", "restart", "vault-gateway.service"), self.calls)
        self.assertEqual((self.tls / f"{host}.key").stat().st_mode & 0o777, 0o600)
        # The next round with the same certificate restarts nothing.
        self.calls.clear()
        round_.restart = False
        with patch.object(sync, "run", lambda *a, check=True: (
                subprocess.CompletedProcess(a, 0, "active\n", "") if a[:2] == ("systemctl", "is-active")
                else self.run_command(*a, check=check))):
            self.assertEqual(round_.run(), 0)
        self.assertNotIn(("systemctl", "restart", "vault-gateway.service"), self.calls)

    def test_a_gateway_with_a_domain_keeps_route53_and_certbot(self) -> None:
        round_ = self.sync_with({"host": "desk.example.com", "zone": "Z1"})
        pointed = []
        round_.dns = lambda gateway, listen: pointed.append((gateway["host"], listen))
        exists = sync.Path.exists
        # The certificate from certbot is there already.
        with patch.object(sync.Path, "exists",
                          lambda self: str(self).startswith("/etc/letsencrypt/") or exists(self)):
            self.assertEqual(round_.run(), 0)
        self.assertEqual(pointed, [("desk.example.com", "100.64.0.7")])
        unit = (self.units / "vault-gateway.service").read_text()
        self.assertIn("LoadCredential=tls.crt:/etc/letsencrypt/live/desk.example.com/fullchain.pem", unit)
        self.assertFalse(any(c[:2] == ("tailscale", "cert") for c in self.calls))


if __name__ == "__main__":
    unittest.main()
