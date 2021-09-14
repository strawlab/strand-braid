#!/bin/bash -x
set -o errexit

rsync -avzP --delete braid-april-cal-webapp/ strawlab-org:strawlab.org/braid-april-cal-webapp/
