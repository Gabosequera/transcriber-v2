"""Explicit preparation, never called by the editor or an inference request."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import time
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[1]
URL = "https://dl.fbaipublicfiles.com/mms/torchaudio/ctc_alignment_mling_uroman/model.pt"
SIZE = 1262047414
LICENSE = "https://github.com/facebookresearch/fairseq/tree/100cd91db19bb27277a06a25eb4154c805b10189/examples/mms#license"


def download():
    destination = ROOT / ".local/models/mms-fa"
    destination.mkdir(parents=True, exist_ok=True)
    destination.resolve().relative_to(ROOT.resolve())
    model = destination / "model.pt"
    if model.exists():
        raise RuntimeError("Existing model preserved; verify its manifest instead of replacing it")
    if shutil.disk_usage(destination).free < SIZE + 1024**3:
        raise RuntimeError("Insufficient space for MMS and its staging file")
    partial = destination / (".download-" + uuid.uuid4().hex + ".partial")
    started = time.monotonic()
    digest = hashlib.sha256()
    count = 0
    try:
        # Public HTTPS resource, no repository/global credentials are consulted.
        with urllib.request.urlopen(URL, timeout=60) as response, partial.open("xb") as output:
            reported = int(response.headers.get("Content-Length", "0"))
            if reported != SIZE:
                raise RuntimeError(f"Upstream size changed: {reported}; review before download")
            etag = response.headers.get("ETag")
            modified = response.headers.get("Last-Modified")
            while block := response.read(1024**2):
                count += len(block)
                if count > SIZE:
                    raise RuntimeError("Upstream sent more bytes than declared")
                output.write(block)
                digest.update(block)
            output.flush()
            os.fsync(output.fileno())
        if count != SIZE:
            raise RuntimeError("Incomplete MMS download")
        # Exclusive publication: a concurrent producer's model stays intact.
        os.link(partial, model)
        metadata = {"model": "torchaudio.pipelines.MMS_FA", "source_url": URL,
                    "size": count, "sha256": digest.hexdigest(), "etag": etag,
                    "last_modified": modified, "license": "CC-BY-NC-4.0",
                    "license_source": LICENSE, "torchaudio_version": "2.8.0+cpu",
                    "elapsed_seconds": round(time.monotonic()-started, 3),
                    "digest_origin": "measured after public HTTPS download; not an upstream signed digest"}
        with (destination / "model-manifest.json").open("x", encoding="utf-8") as output:
            json.dump(metadata, output, ensure_ascii=False, sort_keys=True, indent=2)
        print(json.dumps(metadata, ensure_ascii=False, sort_keys=True))
    finally:
        partial.unlink(missing_ok=True)


if __name__ == "__main__":
    download()
