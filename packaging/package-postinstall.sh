#!/bin/sh
set -eu

if [ -x /usr/lib/uur/post-install-message ]; then
    /usr/lib/uur/post-install-message
fi
