# httpserve

`httpserve` is a simple http file server built on top of [hyper](https://hyper.rs). It loads all files in the target directory into its cache, and serves from cache only.

## Usage

```
httpserve --address 0.0.0.0 --port 3000 /path/to/files
```

```
httpserve 0.2.0
James Guthrie
Serve files from a directory

USAGE:
    httpserve [FLAGS] [OPTIONS] <DIR>

FLAGS:
    -h, --help             Prints help information
    -r, --redirect-http    Whether to redirect http to https
    -V, --version          Prints version information

OPTIONS:
    -a, --address <ADDRESS>    Sets the address to bind to
    -p, --port <PORT>          Set the port to listen on

ARGS:
    <DIR>    Set the directory to serve
```
