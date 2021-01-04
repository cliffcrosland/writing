CREATE TABLE page_nodes (
    org_id bigint NOT NULL,
    page_id bigint NOT NULL,
    id bigint NOT NULL,
    kind integer NOT NULL,
    content text,
    ordering double precision DEFAULT 0 NOT NULL,
    last_edited_by_user_id bigint NOT NULL,
    created_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);
CREATE INDEX page_nodes_oid_pid_ord_knd_content ON page_nodes USING btree (org_id, page_id, ordering, kind, content);


CREATE TABLE page_updates (
    org_id bigint NOT NULL,
    page_id bigint NOT NULL,
    id bigint NOT NULL,
    update_message text NOT NULL,
    occurred_at timestamp without time zone NOT NULL,
    by_user_id bigint NOT NULL
);
CREATE INDEX page_updates_oid_pid_oat_id ON page_updates USING btree (org_id, page_id, occurred_at, id);


CREATE TABLE pages (
    org_id bigint NOT NULL,
    id bigint NOT NULL,
    title text,
    created_by_user_id bigint NOT NULL,
    last_edited_by_user_id bigint NOT NULL,
    project_owner_user_id bigint NOT NULL,
    created_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);
