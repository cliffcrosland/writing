#!/bin/bash

SUFFIX=$1

docker exec -i writing_postgres psql -U app$SUFFIX -d app$SUFFIX << EOF

CREATE TABLE organizations (
  id uuid NOT NULL DEFAULT uuid_generate_v4() PRIMARY KEY,
  name text,
  photo_url text,
  created_at timestamp NOT NULL,
  updated_at timestamp NOT NULL
);

CREATE TABLE users (
  org_id uuid NOT NULL,
  id uuid NOT NULL DEFAULT uuid_generate_v4(),
  email text NOT NULL,
  hashed_password text,
  name text NOT NULL,
  photo_url text,
  role int NOT NULL DEFAULT 0,
  email_verified bool NOT NULL DEFAULT FALSE,
  created_at timestamp NOT NULL,
  updated_at timestamp NOT NULL,
  PRIMARY KEY (org_id, id)
);

CREATE UNIQUE INDEX users_email ON users (email);

CREATE TABLE email_verifications (
  org_id uuid NOT NULL,
  id uuid NOT NULL DEFAULT uuid_generate_v4(),
  token text NOT NULL,
  email_sent_at timestamp,
  created_at timestamp NOT NULL,
  updated_at timestamp NOT NULL,
  PRIMARY KEY (org_id, id)
);

CREATE TABLE invitations (
  org_id uuid NOT NULL,
  id uuid NOT NULL DEFAULT uuid_generate_v4(),
  source_user_id uuid NOT NULL,
  target_email text NOT NULL,
  email_sent_at timestamp,
  created_at timestamp NOT NULL,
  updated_at timestamp NOT NULL,
  PRIMARY KEY (org_id, id)
);

EOF
