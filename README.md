# httpserve

`httpserve` is a simple http file server built on top of [hyper](https://hyper.rs). It loads all files in the target directory into its cache, and serves from cache only.

## Usage

```
httpserve --address 0.0.0.0 --port 3000 /path/to/files
```

```
Usage: httpserve [OPTIONS] <DIR>

Arguments:
  <DIR>  The directory to serve

Options:
  -a, --address <ADDRESS>  The address to listen on [default: 127.0.0.1]
  -p, --port <PORT>        The port to listen on [default: 3000]
  -r, --redirect-http      Whether to redirect http to https
  -h, --help               Print help
  -V, --version            Print version
```

