"""Tests of the rust_python_file binding, run against the compiled module.

Build the module and run these tests with ``test_python_module.ps1`` from the
workspace root.
"""

import pytest

from rust_python_file import ExamplStruct, PyExampleFile, PyPosition


def file_with(path, values):
    file = PyExampleFile(str(path), False)
    for value in values:
        file.write(ExamplStruct(value))
    return file


def records(file):
    return [file.read_at(index).index for index in range(file.item_count())]


# T-008
def test_notebook_scenario(tmp_path):
    file = PyExampleFile(str(tmp_path / "test.file"), False)
    file.write(ExamplStruct(120))

    first = file.read_at(0)
    assert first is not None
    assert first.index == 120
    assert file.item_count() == 1

    file.write_at(PyPosition(1), ExamplStruct(12))
    assert file.item_count() == 2
    assert file.read_at(0).index == 120
    assert file.read_at(1).index == 12


# T-009
def test_open_creates_or_rejects_files(tmp_path):
    path = tmp_path / "new.bin"
    assert PyExampleFile(str(path), False).item_count() == 0
    assert path.exists()
    assert PyExampleFile(str(path), True).item_count() == 0

    missing = tmp_path / "missing.bin"
    with pytest.raises(TypeError, match=r"^Could not open the file"):
        PyExampleFile(str(missing), True)
    assert not missing.exists()


# T-010
def test_read_at_and_struct_property(tmp_path):
    file = file_with(tmp_path / "values.bin", [3, 1, 2])
    assert records(file) == [3, 1, 2]
    assert file.read_at(3) is None

    value = ExamplStruct(5)
    assert value.index == 5
    value.index = 6
    assert value.index == 6


# T-011
def test_find_by_index_returns_nearest_position(tmp_path):
    file = file_with(tmp_path / "sorted.bin", [10, 20, 30])
    assert file.find_by_index(20).index() == 1
    assert file.find_by_index(25).index() == 1
    assert file.find_by_index(5).index() == 0
    assert file.find_by_index(40).index() == 2

    empty = file_with(tmp_path / "empty.bin", [])
    assert empty.find_by_index(10).index() == 0


# T-012
def test_write_at_past_the_end_raises(tmp_path):
    file = file_with(tmp_path / "values.bin", [1, 2])
    with pytest.raises(TypeError, match=r"^Could not write in the file"):
        file.write_at(PyPosition(5), ExamplStruct(0))
    assert file.item_count() == 2


# T-013
def test_read_only_file_rejects_writes(tmp_path):
    path = tmp_path / "values.bin"
    file_with(path, [4])  # closed at once: nothing keeps a reference

    file = PyExampleFile(str(path), True)
    with pytest.raises(TypeError, match="read-only"):
        file.write(ExamplStruct(5))
    with pytest.raises(TypeError, match="read-only"):
        file.write_at(PyPosition(0), ExamplStruct(5))
    del file

    assert records(PyExampleFile(str(path), False)) == [4]
