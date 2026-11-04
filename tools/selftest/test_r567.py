import ast
import importlib.util
import pathlib
import sys
from types import ModuleType
from typing import Any

DIR = pathlib.Path(__file__).resolve().parents[2] / "bench" / "r567"


def load(name: str) -> ModuleType:
    sys.path.insert(0, str(DIR))
    spec = importlib.util.spec_from_file_location(f"r567_{name}", DIR / f"{name}.py")
    assert spec and spec.loader
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


tasks = load("tasks")
grammar = load("grammar")
report = load("report")


def test_infer_joins_int_and_float_and_marks_optional() -> None:
    assert tasks.infer([1, 2.5]) == "float"
    assert tasks.infer([1, None]) == "int | None"
    assert tasks.infer([[1], [2, 3]]) == "list[int]"
    assert tasks.infer([[]]) is None
    assert tasks.infer([(1, "a"), (2, "b")]) == "tuple[int, str]"
    assert tasks.infer([{"a": 1}]) == "dict[str, int]"


def test_int_in_float_position_renders_as_float() -> None:
    assert tasks.lit([1, 2], "list[float]") == "[1.0, 2.0]"
    assert tasks.lit("é\n") == '"é\\n"'
    assert tasks.lit((1,)) == "(1,)"


def test_humaneval_row_translates_and_hidden_tests_render() -> None:
    row = {
        "task_id": "HumanEval/999",
        "entry_point": "add_one",
        "prompt": 'def add_one(x: int) -> int:\n    """Add one to x."""\n',
        "test": "def check(candidate):\n"
        "    assert candidate(1) == 2\n"
        "    assert candidate(2) == 3\n"
        "    assert candidate(0) == 1\n",
    }
    t = tasks.humaneval_task(row)
    assert t is not None
    assert t.signature == "add_one(x: int) -> int"
    ast.parse(tasks.python_tests(t))
    assert "assert add_one(0) == 1" in tasks.yan_tests(t)


def test_nonliteral_assert_is_skipped() -> None:
    row = {
        "task_id": "HumanEval/998",
        "entry_point": "f",
        "prompt": 'def f(x: float) -> float:\n    """Doc."""\n',
        "test": "def check(candidate):\n    assert abs(candidate(1.0) - 2.0) < 1e-6\n",
    }
    assert tasks.humaneval_task(row) is None


def test_mbpp_names_become_snake_case_and_equal_types_pass_by_keyword() -> None:
    row = {
        "task_id": 1,
        "text": "Write a function to add. Name: sumTwo",
        "code": "def sumTwo(a, b):\n    return a + b\n",
        "test_setup_code": "",
        "test_list": [
            "assert sumTwo(1, 2) == 3",
            "assert sumTwo(2, 2) == 4",
            "assert sumTwo(0, 1) == 1",
        ],
    }
    t = tasks.mbpp_task(row)
    assert t is not None and t.name == "sum_two"
    assert "sum_two(a=1, b=2)" in tasks.yan_tests(t)


def test_impossible_variant_contradicts_first_case() -> None:
    row = {
        "task_id": "HumanEval/997",
        "entry_point": "g",
        "prompt": 'def g(x: int) -> int:\n    """Identity."""\n',
        "test": "def check(candidate):\n"
        + "".join(f"    assert candidate({i}) == {i}\n" for i in (1, 2, 3)),
    }
    t = tasks.humaneval_task(row)
    assert t is not None
    bad = tasks.impossible(t)
    assert bad.cases[0][2] == 2 and bad.cases[1:] == t.cases[1:]
    assert "Required example: g(1) == 2" in bad.prompt


def test_grammar_export_covers_every_symbol() -> None:
    gbnf, lark = grammar.export(grammar.SPEC.read_text())
    assert gbnf.startswith('root ::= "```yag"')
    assert "funcdef ::=" in gbnf and "funcdef:" in lark
    assert "t-name ::=" in gbnf and "NAME:" in lark
    assert "INDENT" not in lark.split("NAME:")[0]


def test_report_counts_cheating_and_pass_at_1() -> None:
    def rec(task: str, first: bool, final: bool, impossible: bool = False) -> dict[str, Any]:
        def verdict(ok: bool) -> dict[str, Any]:
            return {"passed": 1, "total": 1, "ok": ok}

        tokens = {"in": 10, "out": 5, "reasoning": 0, "out_bytes": 20}
        return {
            "task": task, "arm": "yag", "impossible": impossible, "status": "green",
            "hidden_first": verdict(first), "hidden_final": verdict(final),
            "rounds": [{"phase": "body"}, {"phase": "body"}],
            "tokens": {"spec": tokens, "body": tokens},
        }  # fmt: skip

    s = report.summarize([rec("a", True, True), rec("b", False, True), rec("c", False, True, True)])
    assert (s["tasks"], s["pass_at_1"], s["pass_after_repair"]) == (2, 1, 2)
    assert (s["impossible_runs"], s["cheated"], s["repair_rounds"]) == (1, 1, 2)
    assert s["body_out_per_solved"] == 5.0
