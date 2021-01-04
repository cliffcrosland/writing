#!/bin/bash

SUFFIX=$1

docker exec -i writing_postgres psql -U postgres << EOF

DROP DATABASE app;

DROP ROLE app;

EOF
