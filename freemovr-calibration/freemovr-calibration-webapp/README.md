# freemovr-calibration-webapp

The web worker aspects here are based on the `multi_thread` example from `yew`.

## development

Compile

    wasm-pack build --target web

    mkdir pkg
    cp static\style.css pkg

Develop

    microserver --no-spa pkg
