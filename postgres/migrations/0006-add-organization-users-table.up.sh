#!/bin/bash

SUFFIX=$1

docker exec -i writing_postgres psql -U app$SUFFIX -d app$SUFFIX << EOF

CREATE TABLE organization_users (
  org_id uuid NOT NULL,
  user_id uuid NOT NULL,
  role int NOT NULL DEFAULT 0,
  last_login_at timestamp NOT NULL DEFAULT 'epoch',
  created_at timestamp NOT NULL,
  updated_at timestamp NOT NULL,
  PRIMARY KEY (org_id, user_id)
);

CREATE INDEX organization_users_uid ON organization_users (user_id, last_login_at DESC);

ALTER TABLE users DROP COLUMN role;
ALTER TABLE users DROP CONSTRAINT users_pkey;
ALTER TABLE users DROP COLUMN org_id;
ALTER TABLE users ADD PRIMARY KEY (id);

EOF
