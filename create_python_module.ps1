$dir = $args[0]

$back_dir = $PWD

if ([string]::IsNullOrWhiteSpace($dir)) { 
    $dir = "."    
}

cd $dir


if (-not (Test-Path 'python_module')) {
    New-Item -ItemType Directory -Path 'python_module' | Out-Null
}

cargo build --release

Get-ChildItem -Filter target/release/*.dll | ForEach-Object { 
    $in = "$_"
    $out=$in.replace(".dll", ".pyd")
    Copy-Item target/release/$in python_module/$out
}

cargo run --release --bin stub_gen

Move-Item -Path "*.pyi" -Destination "python_module\." -Force

cd $back_dir