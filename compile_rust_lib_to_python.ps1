$dir = $args[0]

if ([string]::IsNullOrWhiteSpace($dir)) { 
    $dir = "rust_python_file"
}


create_python_module.ps1 $dir

Copy-Item -Path "$dir/python_module/*.pyi" -Destination "python/rust/." -Force
Copy-Item -Path "$dir/python_module/*.pyd" -Destination "python/rust/." -Force