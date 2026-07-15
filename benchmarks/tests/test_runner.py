from pathlib import Path
import tempfile
import unittest

from zg_bench.runner import (
    SuiteConfigError,
    available_suites,
    build_harbor_command,
    load_suite,
)


def option_value(command: list[str], option: str) -> str:
    return command[command.index(option) + 1]


class SuiteConfigTests(unittest.TestCase):
    def test_both_smoke_suites_are_available(self) -> None:
        self.assertEqual(
            available_suites(), ["swebench-verified", "terminal-bench-2.1"]
        )

    def test_suite_definitions_are_pinned_and_have_one_smoke_task(self) -> None:
        expected = {
            "swebench-verified": (
                "swe-bench/swe-bench-verified@2",
                "swe-bench/sympy__sympy-15976",
            ),
            "terminal-bench-2.1": (
                "terminal-bench/terminal-bench-2-1@6",
                "terminal-bench/regex-log",
            ),
        }
        for name, (dataset, task) in expected.items():
            with self.subTest(name=name):
                suite = load_suite(name)
                self.assertEqual(suite.dataset, dataset)
                self.assertEqual(suite.task, task)

    def test_smoke_tier_rejects_multiple_tasks(self) -> None:
        contents = """\
name: invalid
dataset: example/dataset@1
tiers:
  smoke:
    tasks: [one, two]
"""
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "invalid.yaml"
            path.write_text(contents, encoding="utf-8")
            with self.assertRaisesRegex(SuiteConfigError, "exactly one task"):
                load_suite(path)


class HarborCommandTests(unittest.TestCase):
    def test_command_selects_only_the_configured_task(self) -> None:
        suite = load_suite("swebench-verified")
        command = build_harbor_command(
            suite,
            agent="example-agent",
            model="example/model",
            jobs_dir=Path("results"),
            job_name="test-job",
        )

        self.assertEqual(command[:2], ["harbor", "run"])
        self.assertEqual(option_value(command, "--dataset"), suite.dataset)
        self.assertEqual(option_value(command, "--include-task-name"), suite.task)
        self.assertEqual(option_value(command, "--agent"), "example-agent")
        self.assertEqual(option_value(command, "--model"), "example/model")
        self.assertEqual(option_value(command, "--n-attempts"), "1")
        self.assertEqual(option_value(command, "--n-concurrent"), "1")
        self.assertEqual(option_value(command, "--job-name"), "test-job")
        self.assertNotIn("--skill", command)


if __name__ == "__main__":
    unittest.main()

