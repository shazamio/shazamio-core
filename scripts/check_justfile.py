#!/usr/bin/env python3
"""Check every recipe forwards a variadic parameter as `"$@"`, never as `{{ name }}`."""

import json
import subprocess
import sys
from typing import Final, TypeAlias

# The `type` statement needs Python 3.12; `tests/scripts` imports this module on the
#  3.11 floor, where it failed with `SyntaxError: invalid syntax`.
#  https://docs.python.org/3/reference/simple_stmts.html#the-type-statement
ExpressionType: TypeAlias = "str | list[ExpressionType]"

# `{{ args }}` joins the arguments with spaces into the shell line, so one holding a
#  space reaches the tool as several; the gates never pass any, so nothing else fails.
#  https://just.systems/man/en/positional-arguments.html
_POSITIONAL: Final[str] = "positional-arguments"


def _names(expression: ExpressionType) -> set[str]:
    """Every variable an interpolated expression of the JSON dump reads."""
    if isinstance(expression, str):
        return set()

    if len(expression) == 2 and expression[0] == "variable" and isinstance(expression[1], str):
        return {expression[1]}

    return {name for part in expression for name in _names(part)}


def main() -> int:
    dumped = subprocess.run(
        ["just", "--dump", "--dump-format", "json"],
        capture_output=True,
        check=True,
        encoding="utf-8",
    )
    problems: list[str] = []

    dump = json.loads(dumped.stdout)
    positional = dump["settings"]["positional_arguments"]

    for name, recipe in dump["recipes"].items():
        variadic = {
            parameter["name"]
            for parameter in recipe["parameters"]
            if parameter["kind"] != "singular"
        }

        if not variadic:
            continue

        listed = ", ".join(sorted(variadic))

        if not positional and _POSITIONAL not in recipe["attributes"]:
            problems.append(f"`{name}` takes `{listed}` but has no `[{_POSITIONAL}]`")

        if variadic & _names(recipe["body"]):
            problems.append(f'`{name}` interpolates `{listed}`; forward it as `"$@"`')

    for problem in problems:
        print(problem, file=sys.stderr)

    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
