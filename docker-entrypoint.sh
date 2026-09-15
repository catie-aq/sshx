#!/bin/sh
set -e

# Fix ownership of mounted volumes so the target user can write to them.
# This script runs as root, fixes permissions, then drops to the target user.
LOCAL_UID="${LOCAL_UID:-1000}"
LOCAL_GID="${LOCAL_GID:-1000}"

# chown all paths passed via FIX_DIRS (colon-separated)
if [ -n "$FIX_DIRS" ]; then
  IFS=':'
  for dir in $FIX_DIRS; do
    if [ -d "$dir" ]; then
      chown "$LOCAL_UID:$LOCAL_GID" "$dir"
    fi
  done
  unset IFS
fi

exec gosu "$LOCAL_UID:$LOCAL_GID" "$@"
