"""Static UI inventory only. Parse V1 files; never import or execute their modules."""
import ast
import json
from pathlib import Path

FILES = ["app.py", "automatico_ui.py", "editor_medios.py", "editorial_layers_ui.py",
         "editorial_montaje_ui.py", "toolbar_ui.py", "keymap_ui.py"]
WIDGETS = {"Button", "CTkButton", "CTkCheckBox", "CTkSwitch", "CTkOptionMenu",
           "CTkSegmentedButton", "add_command", "add_checkbutton", "add_radiobutton"}
GESTURES = {"bind", "tag_bind", "protocol"}

class Inventory(ast.NodeVisitor):
    def __init__(self, filename):
        self.filename = filename
        self.scope = []
        self.entries = []

    def visit_FunctionDef(self, node):
        self.scope.append(node.name)
        self.generic_visit(node)
        self.scope.pop()

    def visit_Call(self, node):
        name = node.func.attr if isinstance(node.func, ast.Attribute) else getattr(node.func, "id", "")
        if name in WIDGETS | GESTURES:
            args = {kw.arg: ast.unparse(kw.value) for kw in node.keywords if kw.arg in {"text", "label", "command", "values"}}
            if name in GESTURES:
                args["binding"] = [ast.unparse(a) for a in node.args]
            self.entries.append({"file": self.filename, "line": node.lineno,
                                 "function": ".".join(self.scope), "kind": name, "arguments": args})
        self.generic_visit(node)

def main():
    root = Path(__file__).resolve().parents[1]
    entries = []
    for filename in FILES:
        visitor = Inventory(filename)
        visitor.visit(ast.parse((root.parent / "transcriber" / filename).read_text(encoding="utf-8-sig")))
        entries.extend(visitor.entries)
    report = {"method": "AST read-only; static constructors/bindings, not runtime behavior or functional acceptance",
              "files": FILES, "count": len(entries), "entries": entries}
    (root / "implementation/evidence/continuacion-08-ui-inventory.json").write_text(
        json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps({"count": len(entries), "by_file": {name: sum(e["file"] == name for e in entries) for name in FILES}}))

if __name__ == "__main__":
    main()
