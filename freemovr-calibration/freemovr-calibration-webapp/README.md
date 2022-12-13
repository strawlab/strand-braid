# freemovr-calibration-webapp

The web worker aspects here are based on the `multi_thread` example from `yew`.

## Development

Compile:

    ./build.sh

Run locally:

    # install microserver with: 'cargo install microserver'
    microserver --port 8000 --no-spa pkg

## Install to production

    rsync -avzP pkg/ strawlab-org:strawlab.org/vr-cal/
