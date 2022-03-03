# redirection.io fastly worker

This project contains the worker code that can be deployed to fastly@edge.

You can read more [documentation on redirection.io website](https://redirection.io/documentation/developer-documentation/fastly-compute-edge-integration)

## Requirements

To build this project, you'll need
[Rust](https://www.rust-lang.org/tools/install) and [cargo](https://crates.io/).
You will also need [fastly toolchain](https://github.com/fastly/cli).

## Usage

Copy `fastly.dist.toml` to `fastly.toml` and adapt it according to your need.

Note: You only need to adapt the URL to make it works locally.
If you only publish the worker, you can keep the the file as it.

### Use a local fastly server

1. Copy `redirectionio.dist.json` to `redirectionio.json` and adapt it according to your need.
1. Start the web server :
    ```
    fastly compute serve
    ```

### Deploy it to fastly

**Warning**: you must configure the fastly worker with all required parameters
before deploying the worker.

```
fastly compute publish --service-id=XXXXX
```

You can check the logs:

```
fastly log-tail --service-id=XXXXX
```
