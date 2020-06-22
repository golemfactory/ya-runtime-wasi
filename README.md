<div align="center">
  <h1><code>ya-runtime-wasi</code></h1>

  <p>
    <strong>Yagna-Wasmtime integration</strong>
  </p>

  <p>
    <a href="https://github.com/golemfactory/ya-runtime-wasi/actions"><img src="https://github.com/golemfactory/ya-runtime-wasi/actions?query=workflow%3A%22Continuous+integration%22/badge.svg" /></a>
  </p>
</div>

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
as expected, you can trigger included end-to-end tests like so:

```
cargo test
```

## Running

### As part of Yagna

This step is explained in [Yagna's general tutorial].

[Yagna's general tutorial]: https://github.com/golemfactory/yagna/tree/master/agent/provider

### Standalone

TODO

## License

Licensed under [GPLv3](LICENSE)

