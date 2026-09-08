"""Prepare portable E1/E2 scripts; preparation does not certify a test run."""
import json
from datetime import datetime, timezone
from prepare_script import ROOT, prepare

run = ROOT / "implementation/evidence/e2" / ("regression-" + datetime.now(timezone.utc).strftime("%Y%m%d-%H%M%S-%f"))
run.mkdir()
paths = {}
for scenario, expected in [("e1-recorrido", "e1-expect"), ("e2-escenario1", "e2-expect-escenario1")]:
    target = run / scenario
    target.mkdir()
    for name in [scenario, expected]:
        paths[name] = str(prepare(ROOT / "tests/scripts" / f"{name}.json", target))
text = json.dumps(paths, indent=2)
(run / "runs.json").write_text(text, encoding="utf-8")
(ROOT / "implementation/evidence/e2/latest-regression.json").write_text(text, encoding="utf-8")
print(text)
