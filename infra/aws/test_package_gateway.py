"""Reproducibility and copied-executable integrity of deployment packaging."""
import hashlib
from pathlib import Path
import tempfile
import unittest

import package_gateway
from test_sync import program, release


class PackagingTests(unittest.TestCase):
    def test_site_only_release_replaces_assets_and_keeps_the_verified_executable(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / 'gateway'
            binary.write_bytes(program())
            raw, digest = release(extra={'./site/index.html': b'old', './site/old.js': b'old'})
            archive = root / 'previous.tgz'
            archive.write_bytes(raw)
            site = root / 'dist'
            site.mkdir()
            (site / 'index.html').write_bytes(b'new signed-in Accept')
            (site / 'new.js').write_bytes(b'new script')
            output = root / 'release.tgz'
            value = package_gateway.package(binary, hashlib.sha256(program()).hexdigest(), len(program()),
                                            archive, digest, output, site)
            files = package_gateway.release_files(output.read_bytes(), value['archiveSha256'], 'layr-gateway')
            self.assertEqual(files['layr-gateway'], program())
            self.assertEqual(files['site/index.html'], b'new signed-in Accept')
            self.assertNotIn('site/old.js', files)

    def test_wrong_copied_hash_refuses_publication(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "gateway"
            binary.write_bytes(program()[:-1])
            output = root / "release.tgz"
            with self.assertRaisesRegex(ValueError, "build-machine size or SHA-256"):
                package_gateway.package(binary, hashlib.sha256(program()).hexdigest(), len(program()),
                                        root / "unused", "unused", output)
            self.assertFalse(output.exists())

    def test_packaging_is_reproducible_and_preserves_site_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "gateway"
            binary.write_bytes(program())
            site, digest = release(extra={"./site/index.html": b"tested site"})
            archive = root / "site.tgz"
            archive.write_bytes(site)
            values = []
            for name in ("one.tgz", "two.tgz"):
                values.append(package_gateway.package(binary, hashlib.sha256(program()).hexdigest(), len(program()),
                                                       archive, digest, root / name))
            self.assertEqual(values[0], values[1])
            self.assertEqual((root / "one.tgz").read_bytes(), (root / "two.tgz").read_bytes())
            files = package_gateway.release_files((root / "one.tgz").read_bytes(),
                                                  values[0]["archiveSha256"], "layr-gateway")
            self.assertEqual(files["site/index.html"], b"tested site")


if __name__ == "__main__":
    unittest.main()
