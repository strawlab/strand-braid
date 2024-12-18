#!/bin/bash
set -euxo pipefail

curl -O https://files.pythonhosted.org/packages/8c/3e/bcfa784799bc728d758fb8017ffbf8cba60f598636fd99fb8ef47637a4f6/multicamselfcal-0.3.2.tar.gz
tar xzf multicamselfcal-0.3.2.tar.gz
zip -9 -r --quiet multicamselfcal-0.3.2.zip multicamselfcal-0.3.2
