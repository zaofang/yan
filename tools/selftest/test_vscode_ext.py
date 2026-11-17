import json

from tools.vscode_ext import BUILD, stage


def test_every_path_in_the_manifest_is_staged() -> None:
    stage()
    manifest = json.loads((BUILD / "package.json").read_text(encoding="utf-8"))
    c = manifest["contributes"]
    paths = [g["path"] for g in c["grammars"] + c["snippets"] + c["themes"]]
    paths += [lang["configuration"] for lang in c["languages"]]
    paths += [v for lang in c["languages"] for v in lang["icon"].values()]
    paths.append(manifest["icon"])
    assert [p for p in paths if not (BUILD / p).is_file()] == []


def test_each_language_has_one_grammar() -> None:
    manifest = json.loads((BUILD.parent / "package.json").read_text(encoding="utf-8"))
    c = manifest["contributes"]
    assert sorted(lang["id"] for lang in c["languages"]) == sorted(
        g["language"] for g in c["grammars"]
    )
