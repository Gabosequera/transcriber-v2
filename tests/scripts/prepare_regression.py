"""Prepara guiones E1/E2 con destinos nuevos; nunca borra ni sobrescribe exports."""
import json
from datetime import datetime, timezone
from pathlib import Path
import sys

root = Path(__file__).resolve().parents[2]
run = root / "implementation/evidence/e2" / ("regression-" + datetime.now(timezone.utc).strftime("%Y%m%d-%H%M%S"))
run.mkdir()
paths = {}
for scenario, stage, expected in [("e1-recorrido", "e1", "e1-expect"), ("e2-escenario1", "e2", "e2-expect-escenario1")]:
    old = (root / "implementation/evidence" / stage).as_posix()
    target = run / scenario
    target.mkdir()
    for name in [scenario, expected]:
        text = (root / "tests/scripts" / (name + ".json")).read_text(encoding="utf-8")
        # Todos los destinos del escenario están bajo su carpeta de evidencia.
        text = text.replace(old, target.as_posix())
        json.loads(text)
        path = target / (name + ".json")
        path.write_text(text, encoding="utf-8")
        paths[name] = str(path)
(run / "runs.json").write_text(json.dumps(paths, indent=2), encoding="utf-8")
(root / "implementation/evidence/e2/latest-regression.json").write_text(json.dumps(paths, indent=2), encoding="utf-8")
sys.stdout.reconfigure(encoding="utf-8")
print(json.dumps(paths, ensure_ascii=False, indent=2))
