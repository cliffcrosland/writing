#!/bin/bash

SUFFIX=$1

docker exec -i writing_postgres psql -U app -d app << EOF

ALTER TABLE pages ALTER COLUMN project_owner_user_id SET NOT NULL;

EOF
