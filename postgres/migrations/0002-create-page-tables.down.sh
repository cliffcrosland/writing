#!/bin/bash

SUFFIX=$1

docker exec -i writing_postgres psql -U app -d app << EOF

DROP INDEX page_updates_oid_pid_oat_id;
DROP TABLE page_updates;
DROP INDEX page_nodes_oid_pid_ord_knd_content;
DROP TABLE page_nodes;
DROP TABLE pages;

DROP EXTENSION IF EXISTS "uuid-ossp";

EOF
