"""Bounded NumPy initialization diagnostic, no Torch, model, ASR or V1."""
import argparse
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import time

REPO = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("arousal_tests", REPO/"tests/scripts/test_e5_arousal.py")
tests = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tests)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--timeout", type=int, default=45)
    parser.add_argument("--background", action="store_true")
    parser.add_argument("--order", choices=("numpy-only", "torch-first", "numpy-first"), default="numpy-only")
    parser.add_argument("--blocked-stdin", action="store_true")
    parser.add_argument("--reader-thread", action="store_true")
    parser.add_argument("--peek-reader", action="store_true")
    args = parser.parse_args()
    args.root.mkdir(parents=True, exist_ok=True)
    code = """import faulthandler,importlib.metadata,json,time
faulthandler.dump_traceback_later(15)
print(json.dumps({'stage':'before-numpy','version':importlib.metadata.version('numpy')}),flush=True)
started=time.monotonic()
import numpy
print(json.dumps({'stage':'numpy-loaded','version':numpy.__version__,'seconds':time.monotonic()-started,'sum':float(numpy.array([1.,2.]).sum())}),flush=True)
faulthandler.cancel_dump_traceback_later()
"""
    if args.order == "torch-first":
        code = code.replace("import numpy\n", "print(json.dumps({'stage':'before-torch'}),flush=True)\nimport torch\nprint(json.dumps({'stage':'torch-loaded','version':torch.__version__}),flush=True)\nimport numpy\n")
    elif args.order == "numpy-first":
        code = code.replace("faulthandler.cancel_dump_traceback_later()", "print(json.dumps({'stage':'before-torch'}),flush=True)\nimport torch\nprint(json.dumps({'stage':'torch-loaded','version':torch.__version__}),flush=True)\nfaulthandler.cancel_dump_traceback_later()")
    if args.background:
        code = "import threading\ndef probe():\n" + "\n".join("    "+line for line in code.splitlines()) + "\nthread=threading.Thread(target=probe)\nthread.start()\n"
        if args.blocked_stdin:
            code += "import sys,json\nprint(json.dumps({'stage':'main-blocking-stdin'}),flush=True)\nsys.stdin.buffer.readline()\nprint(json.dumps({'stage':'main-stdin-unblocked'}),flush=True)\n"
        code += "thread.join()\n"
    if args.reader_thread:
        code = "import threading,sys,time,json\nreader=threading.Thread(target=lambda:sys.stdin.buffer.readline(),daemon=True)\nreader.start()\ntime.sleep(.1)\nprint(json.dumps({'stage':'reader-thread-blocking-stdin'}),flush=True)\n" + code + "\nreader.join()\n"
    if args.peek_reader:
        code = "import sys\nsys.path.insert(0," + repr(str(REPO/"workers/python")) + ")\nimport arousal_worker\n" + code.replace("sys.stdin.buffer.readline()", "next(arousal_worker.request_lines(sys.stdin.buffer))")
    with (args.root/"stdout.jsonl").open("xb") as output, (args.root/"stderr.trace").open("xb") as diagnostic:
        child = subprocess.Popen([sys.executable, "-I", "-u", "-c", code], stdout=output, stderr=diagnostic,
                                 stdin=subprocess.PIPE if args.blocked_stdin or args.reader_thread else None,
                                 creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        start = time.monotonic()
        timeout = False
        unblocked_at = None
        try:
            while child.poll() is None:
                tests.memory_tree(child.pid)
                if (args.blocked_stdin or args.reader_thread) and unblocked_at is None and time.monotonic()-start >= 20:
                    unblocked_at = time.monotonic()-start
                    child.stdin.write(b"\n")
                    child.stdin.flush()
                if time.monotonic()-start >= args.timeout:
                    timeout = True
                    subprocess.run(["taskkill", "/PID", str(child.pid), "/T", "/F"], capture_output=True, timeout=10, check=True)
                    child.wait(timeout=10)
                    break
                time.sleep(.2)
        finally:
            if child.poll() is None:
                subprocess.run(["taskkill", "/PID", str(child.pid), "/T", "/F"], capture_output=True, timeout=10)
                child.wait(timeout=10)
    result = {"numpy_only": args.order == "numpy-only", "order": args.order, "background": args.background,
              "blocked_stdin": args.blocked_stdin, "stdin_unblocked_seconds": unblocked_at,
              "reader_thread": args.reader_thread,
              "peek_reader": args.peek_reader,
              "torch_import_requested": args.order != "numpy-only", "model_loaded": False, "root_pid": child.pid, "timeout": timeout,
              "elapsed_seconds": time.monotonic()-start, "exit_code": child.returncode, "memory": tests.MEMORY,
              "stdout": (args.root/"stdout.jsonl").read_text(), "trace": (args.root/"stderr.trace").read_text()}
    (args.root/"result.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps({key: value for key, value in result.items() if key not in ("memory", "trace")}), flush=True)


if __name__ == "__main__":
    main()
