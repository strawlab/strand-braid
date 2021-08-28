#!/bin/bash -x
set -o errexit

rsync -avzP --delete pkg/ strawlab-org:strawlab.org/braid-april-cal-webapp/
