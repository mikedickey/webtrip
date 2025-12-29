# wasm audio worklet

This project builds Rust code into a Web Assembly module that runs inside of a browser's realtime audio thread.

You can build it by running:
```
npm run build
```

Start a local web server by running:
```
npm run serve
```

Then just point your browser at http://localhost:8080/

Special thanks to Lukas Lihotzki for the [WASM Audio Worklet example](https://wasm-bindgen.github.io/wasm-bindgen/examples/wasm-audio-worklet.html).
