#!/bin/bash
set -euo pipefail

mkdir -p scratch
cd scratch
curl -O https://files.pythonhosted.org/packages/fb/be/042a46d0aaa1882e3a387f87be473684978ec416c33a2e52b11fdb7c631e/multicamselfcal-0.3.3.tar.gz
tar xzf multicamselfcal-0.3.3.tar.gz
zip -9 -r --quiet multicamselfcal-0.3.3.zip multicamselfcal-0.3.3
