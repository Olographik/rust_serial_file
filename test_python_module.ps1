# Builds the Python module of a Rust crate with create_python_module.ps1,
# then runs the crate's Python tests (python_tests/) against that module.
#
# Usage: test_python_module.ps1 [dir]   (default: rust_python_file)
#
# The tests run with the interpreter in PYO3_PYTHON when it is set, otherwise
# with `python`: the .pyd only loads in the interpreter it was built for.
# Exits with a non-zero code if the build or any test fails.

$dir = $args[0]

if ([string]::IsNullOrWhiteSpace($dir)) {
    $dir = "rust_python_file"
}

$ErrorActionPreference = "Stop"
$back_dir = $PWD
$back_python_path = $env:PYTHONPATH
$exit_code = 0

try {
    $crate = (Resolve-Path $dir).Path

    & "$PSScriptRoot/create_python_module.ps1" $crate
    if ($LASTEXITCODE -ne 0) {
        throw "building the Python module in $crate failed"
    }
    if (-not (Test-Path "$crate/python_module/*.pyd")) {
        throw "no .pyd was produced in $crate/python_module"
    }

    $python = $env:PYO3_PYTHON
    if ([string]::IsNullOrWhiteSpace($python)) {
        $python = "python"
    }

    $env:PYTHONPATH = "$crate/python_module;$back_python_path"
    & $python -m pytest -p no:cacheprovider "$crate/python_tests"
    if ($LASTEXITCODE -ne 0) {
        throw "the Python tests in $crate/python_tests failed"
    }
}
catch {
    Write-Host "test_python_module: $_" -ForegroundColor Red
    $exit_code = 1
}
finally {
    $env:PYTHONPATH = $back_python_path
    cd $back_dir
}

exit $exit_code
