#!/bin/bash

SUFFIX=$1

docker exec -i writing_postgres psql -U app$SUFFIX -d app$SUFFIX << EOF

ALTER TABLE pages ALTER COLUMN project_owner_user_id DROP NOT NULL;

EOF
