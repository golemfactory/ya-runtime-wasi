## Running wasm-exeunit runtime without supervisor

### Preparing environment

* Copy content of workdir directory to location, where you want to run runtime.
* Change wasm package location in tasks/agreement.json file.

### Running commands

Deploy:
```
RUST_LOG=info cargo run --bin wasmtime-exeunit -- --cachedir workdir/cache --workdir workdir/tasks --agreement agreement.json deploy
```

Start
```
RUST_LOG=info cargo run --bin wasmtime-exeunit -- --cachedir workdir/cache --workdir workdir/tasks --agreement agreement.json start
```

Run:
```
RUST_LOG=info cargo run --bin wasmtime-exeunit -- --cachedir workdir/cache --workdir workdir/tasks --agreement agreement.json run --entrypoint rust-wasi-tutorial input/input.txt output/output.txt
RUST_LOG=info cargo run --bin wasmtime-exeunit -- --cachedir workdir/cache --workdir workdir/tasks --agreement agreement.json run --entrypoint hello-world
```
