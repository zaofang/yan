from tools.guard import violations


def test_agent_branch_touching_tests_is_rejected() -> None:
    assert violations(["tests/a.jian", "src/x.py"], "claude/x", "User") == ["tests/a.jian"]


def test_bot_author_is_agent() -> None:
    assert violations(["spec/s.md"], "feature", "Bot") == ["spec/s.md"]


def test_human_may_touch_tests() -> None:
    assert violations(["conformance/pass/a.jian"], "main-work", "User") == []


def test_prefix_is_a_directory_not_a_substring() -> None:
    assert violations(["tools/tests_helper.py", "testsuite/a"], "agent/x", "User") == []
