## Running example

Download and build wasmtime example repository:

https://github.com/kubkon/rust-wasi-tutorial


Create input and output directories:
```aidl
mkdir workdir
cd workdir
mkdir input
mkdir output
```

Create input file that will be copied by wasm executable:
```aidl
echo "Important Content" >> workdin/input/input.txt
```

Run example application:
```aidl
cargo run --example run-simple-binary  -- rust-wasi-tutorial/target/wasm32-wasi/debug/main.wasm workdir/input workdir/output
```

Check output file in workdir/output/output.txt