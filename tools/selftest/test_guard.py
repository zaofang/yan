from tools.guard import violations


def test_agent_branch_touching_tests_is_rejected() -> None:
    assert violations(["tests/a.jian", "src/x.py"], "claude/x", "User") == ["tests/a.jian"]


def test_bot_author_is_agent() -> None:
    assert violations(["tests/s.jian"], "feature", "Bot") == ["tests/s.jian"]


def test_agent_may_propose_spec_and_rfc_changes() -> None:
    assert violations(["spec/03-yong.md", "rfcs/0001-x.md"], "claude/x", "User") == []


def test_human_may_touch_tests() -> None:
    assert violations(["conformance/pass/a.jian"], "main-work", "User") == []


def test_prefix_is_a_directory_not_a_substring() -> None:
    assert violations(["tools/tests_helper.py", "testsuite/a"], "agent/x", "User") == []
