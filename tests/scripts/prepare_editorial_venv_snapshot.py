"""Copy existing pinned site-packages without changing or linking the source environment."""
import hashlib
import json
from pathlib import Path
import shutil
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / ".local/e5-arousal-venv/Lib/site-packages"
DEST = ROOT / ".local/e5-editorial-venv/Lib/site-packages"


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def main():
    start = time.monotonic()
    source_manifest = {}
    copied = 0
    total = 0
    skipped_pyc = 0
    for source in sorted(SOURCE.rglob("*")):
        if not source.is_file():
            continue
        if source.is_symlink():
            raise ValueError("Source symlink requires explicit review")
        relative = source.relative_to(SOURCE)
        if source.suffix == ".pyc" or "__pycache__" in relative.parts:
            skipped_pyc += 1
            continue
        sha = digest(source)
        source_manifest[relative.as_posix()] = sha
        target = DEST / relative
        if target.exists():
            if digest(target) != sha:
                raise ValueError("Fresh environment scaffold differs: " + str(relative))
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)  # Physical independent files, no hardlinks.
        if digest(target) != sha:
            raise ValueError("Copied file differs: " + str(relative))
        copied += 1
        total += source.stat().st_size
    # Verify every source byte remains unchanged after copying.
    for relative, sha in source_manifest.items():
        if digest(SOURCE / relative) != sha:
            raise ValueError("Source changed during snapshot: " + relative)
    pth = {}
    for path in DEST.glob("*.pth"):
        content = path.read_text(encoding="utf-8")
        if str(SOURCE.parent.parent).lower() in content.lower() or "e5-arousal-venv" in content.lower():
            raise ValueError("PTH references original environment")
        for line in content.splitlines():
            line = line.strip()
            if line and not line.startswith(("#", "import ", "import\t")) and Path(line).is_absolute():
                raise ValueError("Absolute PTH dependency")
        pth[path.name] = content
    result = {"schema": "tv2-editorial-runtime-snapshot/1", "source_unchanged": True,
              "copy_mode": "independent physical files", "copied_file_count": copied,
              "copied_bytes": total, "skipped_pyc_count": skipped_pyc,
              "fresh_index_reproduction_verified": False, "pth": pth,
              "duration_seconds": time.monotonic() - start, "files": source_manifest}
    output = ROOT / "implementation/evidence/e5-editorial-runtime-snapshot.json"
    if output.exists():
        raise ValueError("Preserve previous snapshot evidence")
    output.write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps({k: v for k, v in result.items() if k != "files"}), flush=True)


if __name__ == "__main__":
    main()
