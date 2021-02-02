#!/bin/bash

SUFFIX=$1

docker exec -i writing_postgres psql -U app$SUFFIX -d app$SUFFIX << EOF

CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

CREATE TABLE pages (
  org_id uuid NOT NULL,
  id uuid NOT NULL DEFAULT uuid_generate_v4(),
  title text,
  created_by_user_id uuid NOT NULL,
  last_edited_by_user_id uuid NOT NULL,
  project_owner_user_id uuid NOT NULL,
  created_at timestamp NOT NULL,
  updated_at timestamp NOT NULL,
  PRIMARY KEY (org_id, id)
);

CREATE TABLE page_nodes (
  org_id uuid NOT NULL,
  page_id uuid NOT NULL,
  id uuid NOT NULL DEFAULT uuid_generate_v4(),
  kind int NOT NULL,
  content text,
  ordering double precision NOT NULL DEFAULT 0,
  last_edited_by_user_id uuid NOT NULL,
  created_at timestamp NOT NULL,
  updated_at timestamp NOT NULL,
  PRIMARY KEY (org_id, id)
);

-- This index allows us to render nodes in the UI quickly. Nodes that are near
-- each other on the page (via ordering) will be near each other on disk in the
-- index B-tree leaf nodes. We can fetch full node content without making too
-- many disk reads.
CREATE INDEX page_nodes_oid_pid_ord_knd_content
  ON page_nodes
  (org_id, page_id, ordering, kind, content);

CREATE TABLE page_updates (
  org_id uuid NOT NULL,
  page_id uuid NOT NULL,
  id uuid NOT NULL DEFAULT uuid_generate_v4(),
  update_message text NOT NULL,
  occurred_at timestamp NOT NULL,
  by_user_id uuid NOT NULL,
  PRIMARY KEY (org_id, id)
);

CREATE INDEX page_updates_oid_pid_oat_id
  ON page_updates
  (org_id, page_id, occurred_at, id)

EOF
