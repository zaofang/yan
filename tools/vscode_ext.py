"""Stage and package the VS Code extension (editors/vscode). Nothing here publishes.

uv run python -m tools.vscode_ext stage    assemble editors/vscode/build from the single sources
uv run python -m tools.vscode_ext package  stage, then write build/yan-<version>.vsix

The same .vsix serves the VS Code Marketplace and Open VSX.
"""

import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXT = ROOT / "editors/vscode"
BUILD = EXT / "build"

# (source, path inside the extension)
COPIES = [
    (ROOT / "editors/textmate/yan.tmLanguage.json", "syntaxes/yan.tmLanguage.json"),
    (ROOT / "editors/textmate/yag.tmLanguage.json", "syntaxes/yag.tmLanguage.json"),
    (ROOT / "editors/textmate/yd.tmLanguage.json", "syntaxes/yd.tmLanguage.json"),
    (ROOT / "editors/themes/yan-dark.color-theme.json", "themes/yan-dark.color-theme.json"),
    (ROOT / "editors/themes/yan-light.color-theme.json", "themes/yan-light.color-theme.json"),
    (ROOT / "brand/file-icon-yn.svg", "icons/file-icon-yn.svg"),
    (ROOT / "brand/file-icon-yag.svg", "icons/file-icon-yag.svg"),
    (ROOT / "brand/icon-512.png", "icons/icon-512.png"),
    (ROOT / "LICENSE", "LICENSE"),
    (EXT / "package.json", "package.json"),
    (EXT / "README.md", "README.md"),
    (EXT / "CHANGELOG.md", "CHANGELOG.md"),
    (
        EXT / "languages/yan.language-configuration.json",
        "languages/yan.language-configuration.json",
    ),
    (
        EXT / "languages/yag.language-configuration.json",
        "languages/yag.language-configuration.json",
    ),
    (EXT / "snippets/yan.code-snippets.json", "snippets/yan.code-snippets.json"),
    (EXT / "snippets/yag.code-snippets.json", "snippets/yag.code-snippets.json"),
]


def stage() -> None:
    shutil.rmtree(BUILD, ignore_errors=True)
    for src, rel in COPIES:
        dst = BUILD / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src, dst)


def package() -> None:
    stage()
    # The client and its library become one file, so the .vsix needs no node_modules.
    subprocess.run(["npm", "run", "--silent", "bundle"], cwd=EXT, check=True)
    vsce = EXT / "node_modules/.bin/vsce"
    subprocess.run([vsce, "package", "--no-dependencies"], cwd=BUILD, check=True)


if __name__ == "__main__":
    cmds = {"stage": stage, "package": package}
    if len(sys.argv) != 2 or sys.argv[1] not in cmds:
        sys.exit(__doc__)
    cmds[sys.argv[1]]()
