#!/bin/bash -x
set -o errexit

if [[ -n "$APT_PROXY_CONFIG" ]]; then
    echo "$APT_PROXY_CONFIG" > /etc/apt/apt.conf.d/01proxy
    echo "Using apt proxy: $APT_PROXY_CONFIG"
else
    echo "No apt proxy configured."
fi
