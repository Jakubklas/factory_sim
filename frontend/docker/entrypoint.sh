#!/bin/sh
set -e
envsubst '$BE_URL' < /env.template > /usr/share/nginx/html/env.js
exec nginx -g 'daemon off;'
