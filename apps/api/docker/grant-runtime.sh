#!/bin/sh
set -eu
psql --set=ON_ERROR_STOP=1 --file=/docker/grant-runtime.sql
