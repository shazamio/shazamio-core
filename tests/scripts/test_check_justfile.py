from pathlib import Path

import pytest

from check_justfile import main


@pytest.mark.parametrize(
    ("justfile", "expected"),
    [
        pytest.param('[positional-arguments]\nt *args:\n    echo "$@"\n', "", id="attribute-and-forwarded"),
        pytest.param('set positional-arguments\n\nt *args:\n    echo "$@"\n', "", id="global-setting-and-forwarded"),
        pytest.param("t name:\n    echo {{ name }}\n", "", id="no-variadic-parameter"),
        pytest.param(
            't *args:\n    echo "$@"\n',
            "`t` takes `args` but has no `[positional-arguments]`\n",
            id="no-attribute",
        ),
        pytest.param(
            "[positional-arguments]\nt +args:\n    echo {{ args }}\n",
            '`t` interpolates `args`; forward it as `"$@"`\n',
            id="interpolated",
        ),
        pytest.param(
            "set positional-arguments\n\nt *args:\n    variable{{ args }}\n",
            '`t` interpolates `args`; forward it as `"$@"`\n',
            id="text-fragment-named-variable",
        ),
    ],
)
def test_a_variadic_recipe_must_forward_its_arguments_positionally(
    *,
    justfile: str,
    expected: str,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    (tmp_path / "justfile").write_text(justfile, encoding="utf-8")
    monkeypatch.chdir(tmp_path)

    assert main() == (1 if expected else 0)
    assert capsys.readouterr().err == expected
