"""Owned transport fixture: hello, then no stdin reads; no models or inference."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time

parser = argparse.ArgumentParser()
parser.add_argument("--work-root", type=Path, required=True)
args = parser.parse_args()
args.work_root.mkdir(parents=True, exist_ok=True)
ready = args.work_root / "blocked-stdin-ready.json"
if ready.exists():
    raise RuntimeError("Fixture requires a fresh owned work root")
hello = json.loads(sys.stdin.readline())
if hello.get("protocol") != "tv2-worker/1" or hello.get("method") != "hello":
    raise RuntimeError("Fixture expects hello")
# The supervisor has attached this process to its owned Job Object before hello.
# This child sleeps only; it never accesses models, credentials, media or projects.
child = subprocess.Popen(
    [sys.executable, "-I", "-c", "import time; time.sleep(60)"],
    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
)
temporary = ready.with_suffix(".tmp")
temporary.write_text(json.dumps({"worker_pid": os.getpid(), "child_pid": child.pid}), encoding="utf-8")
os.replace(temporary, ready)
print(json.dumps({"protocol": "tv2-worker/1", "id": hello["id"], "result": {"protocol": "tv2-worker/1", "fixture_only": True}}), flush=True)
# Deliberately never read run/cancel/shutdown: this fills the host's stdin pipe.
time.sleep(60)
