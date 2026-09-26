#!/usr/bin/env python3
"""Empaqueta el mod al zip que espera el Mod Portal.

El portal exige que el zip contenga una única carpeta raíz llamada
`<nombre>_<version>`, exactamente igual que los campos de info.json.
"""

import json
import pathlib
import sys
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
MOD = ROOT / "mod"
DIST = ROOT / "dist"


def main() -> int:
    info = json.loads((MOD / "info.json").read_text(encoding="utf-8"))
    folder = f"{info['name']}_{info['version']}"

    changelog = (MOD / "changelog.txt").read_text(encoding="utf-8")
    if f"Version: {info['version']}" not in changelog:
        print(f"error: changelog.txt no menciona la versión {info['version']}", file=sys.stderr)
        return 1

    DIST.mkdir(exist_ok=True)
    archive = DIST / f"{folder}.zip"

    files = sorted(p for p in MOD.rglob("*") if p.is_file())
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as zf:
        for path in files:
            zf.write(path, pathlib.Path(folder) / path.relative_to(MOD))

    print(f"{archive.relative_to(ROOT)}  ({len(files)} ficheros, {archive.stat().st_size} bytes)")
    for path in files:
        print(f"  {folder}/{path.relative_to(MOD).as_posix()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
