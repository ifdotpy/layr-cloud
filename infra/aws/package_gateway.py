#!/usr/bin/env python3
"""Package a hash-verified gateway and existing site into a validated release.

Copy the executable as a file (for example orb cp), never through command stdout.
Supply the SHA-256 measured on the build machine, before copying it.
"""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import tarfile

from sync import release_files


def package(binary, binary_sha256, binary_bytes, site_archive, site_sha256, output, site_dir=None):
    program = Path(binary).read_bytes()
    if len(program) != binary_bytes or hashlib.sha256(program).hexdigest() != binary_sha256:
        raise ValueError("Copied gateway differs from the build-machine size or SHA-256")
    old = Path(site_archive).read_bytes()
    previous = release_files(old, site_sha256, "layr-gateway")
    files = {name: data for name, data in previous.items() if name.startswith("site/")}
    if site_dir is not None:
        directory = Path(site_dir)
        if directory.is_symlink() or not directory.is_dir():
            raise ValueError("Site build must be a directory without symlinks")
        files = {}
        for path in sorted(directory.rglob('*')):
            if path.is_symlink():
                raise ValueError("Site build contains a symlink")
            if path.is_file():
                files['site/' + path.relative_to(directory).as_posix()] = path.read_bytes()
    if "site/index.html" not in files:
        raise ValueError("Site archive lacks index.html")
    site_count = len(files)
    files.update({"layr-gateway": program, "sync.py": Path(__file__).with_name("sync.py").read_bytes()})
    stream = io.BytesIO()
    with gzip.GzipFile(fileobj=stream, mode="wb", mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode="w") as archive:
            for name, data in sorted(files.items()):
                item = tarfile.TarInfo("./" + name)
                item.size = len(data)
                item.mode = 0o755 if name == "layr-gateway" else 0o644
                archive.addfile(item, io.BytesIO(data))
    raw = stream.getvalue()
    digest = hashlib.sha256(raw).hexdigest()
    release_files(raw, digest, "layr-gateway", binary_sha256, binary_bytes)
    Path(output).write_bytes(raw)
    return {"archiveSha256": digest, "binarySha256": binary_sha256,
            "binaryBytes": len(program), "siteArchiveSha256": site_sha256,
            "siteFiles": site_count, "syncSha256": hashlib.sha256(files["sync.py"]).hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--binary-sha256", required=True)
    parser.add_argument("--binary-bytes", type=int, required=True)
    parser.add_argument("--site-archive", required=True)
    parser.add_argument("--site-sha256", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--site-dir", help="Replace site files with this reviewed build; retain the verified executable")
    args = parser.parse_args()
    print(json.dumps(package(args.binary, args.binary_sha256, args.binary_bytes, args.site_archive,
                             args.site_sha256, args.output, args.site_dir), indent=2))


if __name__ == "__main__":
    main()
