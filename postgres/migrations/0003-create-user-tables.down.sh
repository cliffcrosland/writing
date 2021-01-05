#!/bin/bash

SUFFIX=$1

docker exec -i writing_postgres psql -U app -d app << EOF

DROP INDEX email_verifications_tkn;
DROP TABLE email_verifications;

DROP INDEX invitations_tkn;
DROP TABLE invitations;

DROP INDEX users_email;
DROP TABLE users;

DROP TABLE organizations;

EOF
