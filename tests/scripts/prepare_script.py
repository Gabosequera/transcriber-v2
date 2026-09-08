"""Materialize a script template in a fresh output directory, without running it."""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from support import ROOT, expand_template


def prepare(template: Path, output: Path) -> Path:
    value = json.loads(template.read_text(encoding="utf-8-sig"))
    destination = output / template.name
    with destination.open("x", encoding="utf-8") as stream:
        json.dump(expand_template(value, output), stream, ensure_ascii=False, indent=2)
        stream.write("\n")
    return destination


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("template", type=Path, nargs="+")
    args = parser.parse_args()
    stamp = datetime.now(timezone.utc).strftime("%Y%m%d-%H%M%S-%f")
    output = ROOT / "implementation/evidence/local" / f"prepared-{stamp}"
    output.mkdir(parents=True, exist_ok=False)
    for template in args.template:
        print(prepare(template.resolve(), output))


if __name__ == "__main__":
    main()
