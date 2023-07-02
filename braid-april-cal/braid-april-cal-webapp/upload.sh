#!/bin/bash -x
set -o errexit

# Create pkg/.htaccess file
mkdir -p pkg
cat <<EOF > pkg/.htaccess
AddType application/wasm                            wasm
EOF

# Upload pkg directory
rsync -avzP --delete pkg/ strawlab-org:strawlab.org/braid-april-cal-webapp/
