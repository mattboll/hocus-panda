#!/bin/sh
# A package manager busy elsewhere.
echo "Reading package lists..."
echo "E: Could not get lock /var/lib/dpkg/lock-frontend" >&2
exit 100
