#!/bin/bash -x
set -o errexit

# Create dist/.htaccess file
mkdir -p dist
cat <<EOF > dist/.htaccess
AddType application/wasm                            wasm
EOF

# Upload dist directory
rsync -avzP --delete dist/ strawlab-org:strawlab.org/braid-april-cal-webapp/
