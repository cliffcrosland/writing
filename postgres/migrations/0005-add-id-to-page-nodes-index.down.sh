#!/bin/bash

SUFFIX=$1

docker exec -i writing_postgres psql -U app$SUFFIX -d app$SUFFIX << EOF

DROP INDEX page_nodes_ordered;
CREATE INDEX page_nodes_oid_pid_ord_knd_content ON page_nodes USING btree
  (org_id, page_id, ordering, kind, content);

EOF
