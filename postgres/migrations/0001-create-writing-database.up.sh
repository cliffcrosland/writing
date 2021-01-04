#!/bin/bash

SUFFIX=$1

docker exec -i writing_postgres psql -U postgres << EOF

-- Create owner of app database
CREATE ROLE app$SUFFIX WITH CREATEDB LOGIN PASSWORD '$WRITING_PG_DEV_PASSWORD';

-- Create app database
CREATE DATABASE app$SUFFIX WITH
  OWNER app$SUFFIX
  ENCODING 'UTF8'
  LOCALE 'en_US.utf8';

EOF
