"""Paquete provisional local; sin publicación. Python solo se usa al empaquetar.
python packaging/build_windows.py [--skip-build]
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
from datetime import datetime, timezone
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    parser = argparse.ArgumentParser()
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--public-release", action="store_true", help="editor sin binarios FFmpeg; descarga separada con hashes")
    args = parser.parse_args()
    cargo = shutil.which("cargo") or str(Path.home() / ".cargo/bin/cargo.exe")
    if not args.skip_build:
        subprocess.run([cargo, "build", "--release", "--locked", "-p", "transcriptor"], cwd=ROOT, check=True)
    meta = json.loads(subprocess.check_output([cargo, "metadata", "--locked", "--offline", "--format-version", "1", "--filter-platform", "x86_64-pc-windows-msvc"], cwd=ROOT))
    stamp = datetime.now(timezone.utc).strftime("%Y%m%d-%H%M%S")
    version = next(p["version"] for p in meta["packages"] if p["name"] == "transcriptor")
    name = f"Transcriptor-v{version}-windows-x64-{stamp}" if args.public_release else f"Transcriptor V2 prueba ñ {stamp}"
    output = ROOT / "dist" / name
    output.mkdir(parents=True, exist_ok=False)
    shutil.copy2(ROOT / "target/release/Transcriptor.exe", output)
    if not args.public_release:
        shutil.copytree(ROOT / "packaging/third-party/ffmpeg", output / "third-party/ffmpeg")
    else:
        shutil.copy2(ROOT / "packaging/Install-FFmpeg.ps1", output)
    shutil.copy2(ROOT / "packaging/THIRD-PARTY.md", output)
    shutil.copy2(ROOT / "LICENSE.md", output)
    shutil.copy2(ROOT / "packaging" / ("INICIO-PUBLICO.md" if args.public_release else "INICIO.md"), output / "INICIO.md")
    shutil.copy2(ROOT / "packaging/VERIFY.ps1", output)
    shutil.copy2(ROOT / "Cargo.lock", output)
    notices = output / "third-party/rust"
    notices.mkdir(parents=True)
    inventory = []
    resolved = {node["id"] for node in meta["resolve"]["nodes"]}
    for package in sorted(meta["packages"], key=lambda p: (p["name"], p["version"])):
        if package["source"] is None or package["id"] not in resolved:
            continue
        folder = Path(package["manifest_path"]).parent
        files = sorted(p for p in folder.iterdir() if p.is_file() and p.name.upper().startswith(("LICENSE", "COPYING", "NOTICE")))
        if package.get("license_file"):
            extra = folder / package["license_file"]
            if extra.is_file() and extra not in files:
                files.append(extra)
        destination = notices / f'{package["name"]}-{package["version"]}'
        destination.mkdir()
        for source in files:
            shutil.copy2(source, destination / source.name)
        supplemental = ROOT / "packaging/third-party/rust-notices" / f'{package["name"]}-{package["version"]}'
        if supplemental.is_dir():
            for notice in supplemental.iterdir():
                if notice.is_file():
                    shutil.copy2(notice, destination / notice.name)
        # Font/other notices sometimes live below the root of the registry crate.
        for notice in folder.rglob("*"):
            if notice.is_file() and notice.parent != folder and notice.name.upper().startswith(("LICENSE", "COPYING", "NOTICE", "OFL", "UFL")):
                nested = destination / notice.relative_to(folder)
                nested.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(notice, nested)
        inventory.append({"name": package["name"], "version": package["version"], "license": package["license"],
                          "repository": package["repository"], "notices": [str(p.relative_to(output)).replace("\\", "/") for p in destination.rglob("*") if p.is_file() and p.name != "PROVENANCE.json"]})
    (notices / "inventory.json").write_text(json.dumps(inventory, ensure_ascii=False, indent=2), encoding="utf-8")
    # Fixture sintética ya guardada por el recorrido de la GUI, con media dentro del demo.
    demo_source = ROOT / "implementation/evidence/e2/cache-markers-demo.transcriptor/project.json"
    demo = json.loads(demo_source.read_text(encoding="utf-8"))
    demo["name"] = "Demo: cortes, cachés y marcador"
    demo_dir = output / "Demo.transcriptor"
    (demo_dir / "media").mkdir(parents=True)
    for asset in demo["assets"]:
        source = Path(asset["path"])
        if not source.is_absolute():
            source = demo_source.parent.parent / source
        name = asset["id"] + source.suffix
        shutil.copy2(source, demo_dir / "media" / name)
        asset["path"] = "Demo.transcriptor/media/" + name
    (demo_dir / "project.json").write_text(json.dumps(demo, ensure_ascii=False, indent=2), encoding="utf-8")
    manifest = {"schema": "transcriptor-package/1", "build": stamp, "stage": "E2 provisional", "platform": "windows-x86_64",
                "version": version, "ffmpeg_bundled": not args.public_release, "license": "Proprietary; third-party licenses separate",
                "clean_windows_tested": False, "rust_dependencies_inventoried": len(inventory),
                "dependency_notices_missing": [p["name"] for p in inventory if not p["notices"]]}
    if args.public_release and manifest["dependency_notices_missing"]:
        raise RuntimeError(f"No publicar: avisos ausentes {manifest['dependency_notices_missing']}")
    (output / "PACKAGE.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")
    checks = []
    for path in sorted(p for p in output.rglob("*") if p.is_file()):
        digest = hashlib.file_digest(path.open("rb"), "sha256").hexdigest()
        checks.append(f"{digest}  {path.relative_to(output).as_posix()}")
    (output / "CHECKSUMS.sha256").write_text("\n".join(checks) + "\n", encoding="utf-8")
    archive = Path(str(output) + ".zip")
    staging_archive = archive.with_suffix(".partial.zip")
    with zipfile.ZipFile(staging_archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6, strict_timestamps=False) as z:
        for path in sorted(p for p in output.rglob("*") if p.is_file()):
            z.write(path, path.relative_to(output.parent))
    staging_archive.replace(archive)
    report = {"directory": str(output), "archive": str(archive), "archive_bytes": archive.stat().st_size,
              "editor_bytes": (output / "Transcriptor.exe").stat().st_size, "package_bytes": sum(p.stat().st_size for p in output.rglob("*") if p.is_file()),
              "sha256": hashlib.file_digest(archive.open("rb"), "sha256").hexdigest(), **manifest}
    (ROOT / "implementation/evidence/e2/package-build.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
