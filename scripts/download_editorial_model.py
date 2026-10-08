"""Prepare pinned public Qwen weights inside V2, without authentication."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
import urllib.request
import uuid

MODEL = "Qwen/Qwen2.5-1.5B-Instruct"
REVISION = "989aa7980e4cf806f80c7fef2b1adb7bc71aa306"
WEIGHT_SHA = "dd924a11b4c220f385b51ffa522daea7c9f3d850e31b162bb5661df483c6d3ee"
FILES = {"LICENSE": 11343, "README.md": 4917, "config.json": 660,
         "generation_config.json": 242, "merges.txt": 1671839,
         "tokenizer.json": 7031645, "tokenizer_config.json": 7305,
         "vocab.json": 2776833, "model.safetensors": 3087467144}


def sha(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while block := source.read(1024 * 1024):
            digest.update(block)
    return digest.hexdigest()


def main():
    repo = Path(__file__).resolve().parent.parent
    root = Path(sys.argv[1]).resolve()
    if not root.is_relative_to(repo / ".local" / "models"):
        raise ValueError("Model destination must remain inside V2 .local/models")
    root.mkdir(parents=True, exist_ok=True)
    if shutil.disk_usage(root).free < sum(FILES.values()) + 2 * 1024 ** 3:
        raise ValueError("Insufficient free space: reserve weights plus 2 GiB")
    entries = {}
    for name, size in FILES.items():
        target = root / name
        expected = WEIGHT_SHA if name == "model.safetensors" else None
        if not target.exists():
            temporary = root / (name + ".download-" + uuid.uuid4().hex)
            count = 0
            try:
                url = f"https://huggingface.co/{MODEL}/resolve/{REVISION}/{name}"
                with urllib.request.urlopen(url, timeout=60) as response, temporary.open("xb") as output:
                    while block := response.read(1024 * 1024):
                        count += len(block)
                        if count > size:
                            raise ValueError("Download exceeds pinned size: " + name)
                        output.write(block)
                    output.flush()
                    os.fsync(output.fileno())
                if count != size or expected and sha(temporary) != expected:
                    raise ValueError("Pinned file size/SHA mismatch: " + name)
                os.link(temporary, target)
            finally:
                temporary.unlink(missing_ok=True)
        digest = sha(target)
        if target.stat().st_size != size or expected and digest != expected:
            raise ValueError("Existing file differs and was preserved: " + name)
        entries[name] = {"size": size, "sha256": digest}
        print(json.dumps({"file": name, **entries[name]}), flush=True)
    if "apache-2.0" not in (root / "README.md").read_text(encoding="utf-8").lower():
        raise ValueError("Pinned model card license changed")
    manifest = {"schema": "tv2-editorial-model/1", "model": MODEL, "revision": REVISION,
                "license": "apache-2.0", "files": entries,
                "source": "public pinned Hugging Face resolve; no authentication"}
    payload = json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode()
    target = root / "model-manifest.json"
    if target.exists():
        if target.read_bytes() != payload:
            raise ValueError("Existing manifest differs and was preserved")
    else:
        with target.open("xb") as output:
            output.write(payload)
    print(json.dumps({"manifest": str(target), "sha256": sha(target)}), flush=True)


if __name__ == "__main__":
    main()
