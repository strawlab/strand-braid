# freemovr-calibration-webapp

The web worker aspects here are based on the `multi_thread` example from `yew`.

## Development

Compile:

    ./build.sh

Run locally:

    trunk serve

## Install to production

    rsync -avzP dist/ strawlab-org:strawlab.org/vr-cal/
