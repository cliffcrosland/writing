# Notes to self

### Installing wasm-pack
I installed wasm-pack "globally" as follows:
```
cargo install wasm-pack
```

### Using wasm-pack to build the project
To build the project and save the wasm artifact in `./static`:

```
make
```

Or, equivalently:

```
wasm-pack build --target web --out-name wasm --out-dir ./static
```

### minserve: Serving up static files via http
To serve up the static files, can use the `miniserve` crate.

To install:
```
cargo install miniserve
```

To run:
```
./dev_server.sh
```

Or, equivalently:

```
miniserve ./static --index index.html
```
