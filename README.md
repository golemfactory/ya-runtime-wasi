# Yagna-Wasmtime integration (`ya-runtime-wasi`)
![Continuous integration](https://github.com/golemfactory/ya-runtime-wasi/workflows/Continuous%20integration/badge.svg)

`ya-runtime-wasi` is a [`Yagna`] plugin that allows the provider to execute WASI modules
in a safe, sandboxed way. Typically, you will use this crate as part of your Yagna provider
installation. However, it is also possible to use the integration standalone to execute
zipped WASI modules according to some included manifest file.

[`Yagna`]: https://github.com/golemfactory/yagna

## Building

Building the project is very straightforward:

```
cargo build
```

If you decide to make some tweaks and would like to test if everything still behaves
as expected, you can trigger included end-to-end integration tests like so:

```
cargo test --features integration-tests
```

Note that running the end-to-end tests requires you to have `wasm32-wasi` target installed.

## Running

### As part of Yagna

This step is explained in [Yagna's general tutorial].

[Yagna's general tutorial]: https://github.com/golemfactory/yagna/tree/master/agent/provider

### Standalone

TODO

## License

Licensed under [GPLv3](LICENSE)

