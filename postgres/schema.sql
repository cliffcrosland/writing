CREATE TABLE email_verifications (
    org_id uuid NOT NULL,
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    token text NOT NULL,
    email_sent_at timestamp without time zone,
    created_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);

ALTER TABLE ONLY email_verifications
    ADD CONSTRAINT email_verifications_pkey PRIMARY KEY (org_id, id);


CREATE TABLE invitations (
    org_id uuid NOT NULL,
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    source_user_id uuid NOT NULL,
    target_email text NOT NULL,
    email_sent_at timestamp without time zone,
    created_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);

ALTER TABLE ONLY invitations
    ADD CONSTRAINT invitations_pkey PRIMARY KEY (org_id, id);


CREATE TABLE organizations (
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    name text,
    photo_url text,
    created_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);

ALTER TABLE ONLY organizations
    ADD CONSTRAINT organizations_pkey PRIMARY KEY (id);


CREATE TABLE page_nodes (
    org_id uuid NOT NULL,
    page_id uuid NOT NULL,
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    kind integer NOT NULL,
    content text,
    ordering double precision DEFAULT 0 NOT NULL,
    last_edited_by_user_id uuid NOT NULL,
    created_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);

ALTER TABLE ONLY page_nodes
    ADD CONSTRAINT page_nodes_pkey PRIMARY KEY (org_id, id);

CREATE INDEX page_nodes_oid_pid_ord_knd_content ON page_nodes USING btree (org_id, page_id, ordering, kind, content);


CREATE TABLE page_updates (
    org_id uuid NOT NULL,
    page_id uuid NOT NULL,
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    update_message text NOT NULL,
    occurred_at timestamp without time zone NOT NULL,
    by_user_id uuid NOT NULL
);

ALTER TABLE ONLY page_updates
    ADD CONSTRAINT page_updates_pkey PRIMARY KEY (org_id, id);

CREATE INDEX page_updates_oid_pid_oat_id ON page_updates USING btree (org_id, page_id, occurred_at, id);


CREATE TABLE pages (
    org_id uuid NOT NULL,
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    title text,
    created_by_user_id uuid NOT NULL,
    last_edited_by_user_id uuid NOT NULL,
    project_owner_user_id uuid NOT NULL,
    created_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);

ALTER TABLE ONLY pages
    ADD CONSTRAINT pages_pkey PRIMARY KEY (org_id, id);


CREATE TABLE users (
    org_id uuid NOT NULL,
    id uuid DEFAULT public.uuid_generate_v4() NOT NULL,
    email text NOT NULL,
    hashed_password text,
    name text NOT NULL,
    photo_url text,
    role integer DEFAULT 0 NOT NULL,
    email_verified boolean DEFAULT false NOT NULL,
    created_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);

ALTER TABLE ONLY users
    ADD CONSTRAINT users_pkey PRIMARY KEY (org_id, id);

CREATE UNIQUE INDEX users_email ON users USING btree (email);

