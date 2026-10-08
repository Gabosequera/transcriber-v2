"""Explicit public resource preparation; never called by a worker/run/editor.

Model use is research-only per upstream. Code MIT; base configuration Apache2.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import time
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[1]
MODEL_REVISION = "cb10e3920766372f06bbd9657724f24dc39fa3e4"
BASE_REVISION = "569a6236e92bd5f7652a0420bfe9bb94c5664080"
CODE_REVISION = "a525292d26f744e14624e3a2f1fb5e3c7858d7b3"
RESOURCES = {
    "model.safetensors": (f"https://huggingface.co/omine-me/LaughterSegmentation/resolve/{MODEL_REVISION}/model.safetensors",
                          1261816628, "449b14f73c70db26da9b4a59ee77d9a9b29fbcaceb083dd7ea27cdfaa68442a0"),
    "config.json": (f"https://huggingface.co/jonatasgrosman/wav2vec2-large-xlsr-53-english/resolve/{BASE_REVISION}/config.json",
                    1531, "ffcc5c417fe11433447975d5053b2279fbeafd6bca03dd2753082e72ad2d36b7"),
}


def download():
    destination = ROOT / ".local/models/laughter-omine"
    destination.mkdir(parents=True, exist_ok=True)
    destination.resolve().relative_to(ROOT.resolve())
    if any((destination/name).exists() for name in (*RESOURCES, "model-manifest.json")):
        raise RuntimeError("Existing laughter resources preserved; inspect instead of replacing")
    if shutil.disk_usage(destination).free < 1261816628+1024**3:
        raise RuntimeError("Insufficient space for model/staging")
    started = time.monotonic()
    metadata = {}
    for name, (url, size, expected) in RESOURCES.items():
        partial = destination / (".download-"+uuid.uuid4().hex+".partial")
        count, sha = 0, hashlib.sha256()
        try:
            with urllib.request.urlopen(url, timeout=60) as response, partial.open("xb") as output:
                declared = response.headers.get("Content-Length")
                if declared is not None and int(declared) != size:
                    raise RuntimeError("Declared resource size changed")
                while block := response.read(1024**2):
                    count += len(block)
                    if count > size:
                        raise RuntimeError("Resource exceeds pinned size")
                    sha.update(block)
                    output.write(block)
                output.flush()
                os.fsync(output.fileno())
            if count != size or sha.hexdigest() != expected:
                raise RuntimeError("Pinned resource size/SHA256 mismatch")
            os.link(partial, destination/name)
            metadata[name] = {"source_url": url, "size": size, "sha256": expected}
        finally:
            partial.unlink(missing_ok=True)
    manifest = {"model": "omine-me/LaughterSegmentation", "model_revision": MODEL_REVISION,
                "base_config_revision": BASE_REVISION, "code_revision": CODE_REVISION,
                "files": metadata, "code_license": "MIT Copyright(c)2024 Taisei Omine", "model_license": "research-only",
                "base_config_license": "Apache-2.0", "license_source": f"https://github.com/omine-me/LaughterSegmentation/blob/{CODE_REVISION}/README.md#license",
                "code_source": f"https://github.com/omine-me/LaughterSegmentation/blob/{CODE_REVISION}/train/model.py",
                "elapsed_seconds": round(time.monotonic()-started, 3), "digest_origin": "HF LFS model SHA; locally verified all bytes"}
    with (destination/"model-manifest.json").open("x", encoding="utf-8") as output:
        json.dump(manifest, output, ensure_ascii=False, sort_keys=True, indent=2)
    print(json.dumps(manifest, ensure_ascii=False, sort_keys=True))


if __name__ == "__main__":
    download()
