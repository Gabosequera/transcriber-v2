"""Download public pinned audEERING files, preserving existing bytes. No tokens."""
import hashlib
import json
import os
from pathlib import Path
import sys
import urllib.request
import uuid

MODEL = "audeering/wav2vec2-large-robust-12-ft-emotion-msp-dim"
REVISION = "6eba34a2485ea31cb03600241787c3a5edab8626"
WEIGHT_SHA = "efa5ac1a13b2d2f42182738e44794b1eb4c0cdd221a8b4ae11304c3a5f5fae95"


def sha(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while block := source.read(1024*1024):
            digest.update(block)
    return digest.hexdigest()


def download(root, name, size=None, expected=None):
    target = root/name
    if target.exists():
        if size is not None and target.stat().st_size != size or expected is not None and sha(target) != expected:
            raise ValueError("Existing model file differs; preserved: " + name)
        return target
    temporary = root/(name + ".download-" + uuid.uuid4().hex)
    try:
        url = "https://huggingface.co/" + MODEL + "/resolve/" + REVISION + "/" + name
        with urllib.request.urlopen(url, timeout=60) as response, temporary.open("xb") as output:
            while block := response.read(1024*1024):
                output.write(block)
            output.flush()
            os.fsync(output.fileno())
        if size is not None and temporary.stat().st_size != size or expected is not None and sha(temporary) != expected:
            raise ValueError("Pinned upstream size/SHA mismatch: " + name)
        # Exclusive destination: never replace a concurrently created file.
        os.link(temporary, target)
    finally:
        temporary.unlink(missing_ok=True)
    print(json.dumps({"download": name, "size": target.stat().st_size, "sha256": sha(target)}), flush=True)
    return target


def main():
    root = Path(sys.argv[1]).resolve()
    root.mkdir(parents=True, exist_ok=True)
    files = {}
    for name, size, expected in (("config.json", 2344, "c0962c3d1f065972bbebbba0bbffb8016ef4e9aae4a9b07e5fec22f770d2cddb"),
                                 ("preprocessor_config.json", 214, "60ca5a31e13f69ee2fbf147504c8676db5f6398fd7a6b12294341dff838edfcf"),
                                 ("model.safetensors", 661375508, WEIGHT_SHA)):
        path = download(root, name, size, expected)
        files[name] = {"size": path.stat().st_size, "sha256": sha(path)}
    readme = download(root, "README.md")
    text = readme.read_text(encoding="utf-8")
    if "cc-by-nc-sa-4.0" not in text.lower():
        raise ValueError("Pinned model card license changed")
    manifest = {"schema": "tv2-arousal-model/1", "model": MODEL, "revision": REVISION, "license": "cc-by-nc-sa-4.0", "files": files,
                "model_card": {"path": "README.md", "sha256": sha(readme)}, "source": "public pinned Hugging Face resolve; no authentication"}
    payload = json.dumps(manifest, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    destination = root/"model-manifest.json"
    if destination.exists():
        if destination.read_bytes() != payload:
            raise ValueError("Existing manifest differs; preserved")
    else:
        with destination.open("xb") as output:
            output.write(payload)
    print(json.dumps({"manifest": str(destination), "sha256": sha(destination), "files": files}), flush=True)


if __name__ == "__main__":
    main()
