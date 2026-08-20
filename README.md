# Yagna WASI runtime
![Continuous integration](https://github.com/golemfactory/ya-runtime-wasi/workflows/Continuous%20integration/badge.svg)

`ya-runtime-wasi` is a [`Yagna`] plugin that allows the provider to execute WASI Preview 1
modules and WASI Preview 2 components
in a safe, sandboxed way. Typically, you will use this crate as part of your Yagna provider
installation. However, it is also possible to use the integration standalone to execute
zipped Wasm programs according to an included manifest file. Wasmtime is the only execution
engine; legacy `aswasm` packages are rejected.

[`Yagna`]: https://github.com/golemfactory/yagna

## Building

The project uses Rust 1.97 and edition 2024. Install the pinned Rust toolchain, WASI target,
`protoc`, and workflow validator through [`mise`]:

```sh
mise install rust protoc actionlint shellcheck
mise run ci:build
```

[`mise`]: https://mise.jdx.dev/

If you decide to make some tweaks to the [API] and would like to test if everything still behaves
as expected, you can trigger included end-to-end integration tests. Make sure you have
the tools from `mise.toml` installed and then run:

[API]: crates/api

```
mise run ci:test
```

The runtime detects the binary format automatically: core modules use WASI Preview 1 and
components exporting `wasi:cli/run@0.2.0` use WASI Preview 2. No manifest switch is needed.

Execution is unlimited by default, which allows long-running time-and-material workloads.
`YA_RUNTIME_WASI_MAX_FUEL` can optionally set a deterministic Wasmtime fuel budget for each
invocation; `0` or an unset variable disables it. Fuel limits guest computation, not wall-clock
time, so waiting on host I/O does not consume it. Each guest linear memory defaults to a
non-moving 1 GiB virtual reservation and a matching hard limit;
`YA_RUNTIME_WASI_INIT_MEM` can override both with values such as `256m` or `2g`. `start`
instantiates every entry point, so incompatible imports and impossible initial memory requests
fail before `run`. As with Wasmtime's former static-memory setting, the reservation is virtual
address space and does not eagerly commit the same amount of physical RAM.

## Running

### As part of Yagna

This step is explained in [Yagna's general tutorial].

[Yagna's general tutorial]: https://github.com/golemfactory/yagna/tree/master/agent/provider

### Standalone

Running standalone is pretty simple. For this, you'll want to use a Wasm module which performs
some input from the host, does some computations, and outputs the results to a file on the host.
To keep everything simple, we'll assume you use [`rust-wasi-tutorial`]. Clone the repo, and
build the project:

```
git clone https://github.com/kubkon/rust-wasi-tutorial.git
cd rust-wasi-tutorial
cargo build --release --target wasm32-wasip1
```

[`rust-wasi-tutorial`]: https://github.com/kubkon/rust-wasi-tutorial

This will automatically cross-compile your project to the WASI Preview 1 target.

Next, we'll need to create a Yagna package. Go ahead and create new dir called `package`
somewhere in your home directory, and copy `rust-wasi-tutorial.wasm` module into it:

```
mkdir package
cp rust-wasi-tutorial/target/wasm32-wasip1/release/main.wasm package/rust-wasi-tutorial.wasm
```

Next, we'll need to create a manifest for the package called `manifest.json`:

```json
{
    "id": "rust-wasi-tutorial",
    "name": "rust-wasi-tutorial",
    "runtime": "wasi",
    "entry-points": [
        {
            "id": "rust-wasi-tutorial",
            "wasm-path": "rust-wasi-tutorial.wasm"
        }
    ],
    "mount-points": [
        { "ro": "input" },
        { "rw": "output" }
    ]
}
```

Here, of interest are `entry-points` and `mount-points` entries. The former tell the runtime
what modules to load up when we specify some entrypoint (e.g., `rust-wasi-tutorial` will load
up the `rust-wasi-tutorial.wasm` module), whereas the latter instruct the runtime which directories
to preopen and map into our container so that we can make use of it. In this case, we'll map a
relative dir `input` as read-only `/input` inside the container and `output` as read/write
`/output`. Mounts can use `ro`, `rw`, or `wo`; these permissions are enforced by Wasmtime.

OK, now we can create the package by zipping the `package` folder:

```
(cd package && zip -r ../rust-wasi-tutorial.zip .)
```

Finally, we'll create a `workspace` dir where we'll mount our package using the runtime:

```
mkdir workspace
```

We're now ready to deploy the package:

```
./target/debug/ya-runtime-wasi --task-package rust-wasi-tutorial.zip --workdir workspace deploy
```

Deployment created the mount points on the host for us inside `workspace`. Namely, you should find
there `workspace/input` and `workspace/output` among other things. Next, go ahead and create some
dummy text file `in` with `Hello WASI!` and put it in `workspace/input/in`. It will then automatically
get mapped into `/input/in` for use by our Wasm module.

Next, we need to start the module:

```
./target/debug/ya-runtime-wasi --task-package rust-wasi-tutorial.zip --workdir workspace start
```

Finally, we can run it:

```
./target/debug/ya-runtime-wasi --task-package rust-wasi-tutorial.zip --workdir workspace run --entrypoint rust-wasi-tutorial /input/in /output/out
```

If everything went according to plan, you should now find `out` text file with `Hello WASI!` text in it
inside `workspace/output/out`.


## License

Licensed under [GPLv3](LICENSE)
