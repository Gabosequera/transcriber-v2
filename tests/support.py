"""Local development paths; independent of the machine that created a fixture."""
import os
from pathlib import Path
import shutil

ROOT = Path(__file__).resolve().parents[1]


def media_tool(name: str) -> str:
    executable = name + (".exe" if os.name == "nt" else "")
    override = os.environ.get("TRANSCRIPTOR_FFMPEG_DIR")
    if override:
        candidate = Path(override) / executable
        if not candidate.is_file():
            raise FileNotFoundError(f"TRANSCRIPTOR_FFMPEG_DIR: missing {candidate}")
        return str(candidate.resolve())
    bundled = ROOT / "packaging/third-party/ffmpeg" / executable
    if bundled.is_file():
        return str(bundled)
    found = shutil.which(executable)
    if found:
        return found
    raise FileNotFoundError(f"Install {name} or set TRANSCRIPTOR_FFMPEG_DIR")


def expand_template(value, output: Path):
    """Expand only the two documented tokens, including nested JSON values."""
    if isinstance(value, str):
        return value.replace("${WORKSPACE}", ROOT.as_posix()).replace("${OUTPUT}", output.resolve().as_posix())
    if isinstance(value, list):
        return [expand_template(item, output) for item in value]
    if isinstance(value, dict):
        return {key: expand_template(item, output) for key, item in value.items()}
    return value
