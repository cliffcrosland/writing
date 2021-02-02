#!/bin/bash

SUFFIX=$1

docker exec -i writing_postgres psql -U app$SUFFIX -d app$SUFFIX << EOF

DROP TABLE organization_users;

ALTER TABLE users ADD COLUMN role int NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN org_id uuid NOT NULL;
ALTER TABLE users DROP CONSTRAINT users_pkey;
ALTER TABLE users ADD PRIMARY KEY (org_id, id);

EOF
