#!/bin/sh
# A package manager that has three updates, until they are installed.
[ -e "$1/installed" ] && exit 0
echo "firefox 132.0 -> 132.0.1"
echo "mesa 24.2.4 -> 24.2.5"
echo "linux 6.11.3 -> 6.11.4"
