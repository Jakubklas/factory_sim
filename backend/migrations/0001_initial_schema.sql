-- Deploy nodes: cluster machines that can host simulated PLCs.
-- Seeded from deploy/inventory.json; updated by the k8s node sync (Phase 5).
CREATE TABLE deploy_nodes (
    name                VARCHAR(64)  PRIMARY KEY,
    display_name        TEXT         NOT NULL,
    k8s_node            TEXT,
    arch                VARCHAR(16)  NOT NULL DEFAULT 'amd64',
    mem_allocatable_mb  INTEGER,
    status              VARCHAR(16)  NOT NULL DEFAULT 'unknown',
    last_seen           TIMESTAMPTZ
);

-- Device types: physics scripts + I/O contract + asset refs.
-- io_spec = { inputs:[{name,type}], outputs:[{name,type,initial}], params:[{name,default}] }
CREATE TABLE device_types (
    id           UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    name         VARCHAR(128) NOT NULL UNIQUE,
    physics_rhai TEXT         NOT NULL DEFAULT '',
    io_spec      JSONB        NOT NULL DEFAULT '{}',
    model_ref    TEXT,
    icon_ref     TEXT,
    updated_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

-- PLCs: simulated (hosted on a deploy_node) or real (accessed via endpoint_uri).
CREATE TYPE plc_kind AS ENUM ('simulated', 'real');

CREATE TABLE plcs (
    id              UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    name            VARCHAR(128) NOT NULL UNIQUE,
    kind            plc_kind     NOT NULL DEFAULT 'simulated',
    deploy_node     VARCHAR(64)  REFERENCES deploy_nodes(name) ON DELETE SET NULL,
    endpoint_uri    TEXT,
    port            INTEGER,
    updated_at      TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    CONSTRAINT plc_kind_check CHECK (
        (kind = 'simulated' AND deploy_node IS NOT NULL) OR
        (kind = 'real'      AND endpoint_uri IS NOT NULL)
    )
);

-- Device instances: a device_type placed on a PLC with concrete params.
CREATE TABLE device_instances (
    id              UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    plc_id          UUID         NOT NULL REFERENCES plcs(id) ON DELETE CASCADE,
    device_type_id  UUID         NOT NULL REFERENCES device_types(id),
    name            VARCHAR(128) NOT NULL,
    param_values    JSONB        NOT NULL DEFAULT '{}',
    floor_pos       JSONB,
    UNIQUE (plc_id, name)
);

-- Wires: connect an upstream OPC-UA node to a downstream device instance input port.
CREATE TABLE wires (
    id               UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    src_plc_id       UUID         NOT NULL REFERENCES plcs(id) ON DELETE CASCADE,
    src_node         TEXT         NOT NULL,
    dst_instance_id  UUID         NOT NULL REFERENCES device_instances(id) ON DELETE CASCADE,
    dst_input_port   TEXT         NOT NULL,
    UNIQUE (dst_instance_id, dst_input_port)
);

-- Discovered OPC-UA nodes: cached from Browse, refreshed on reconnect.
CREATE TABLE discovered_nodes (
    plc_id        UUID         NOT NULL REFERENCES plcs(id) ON DELETE CASCADE,
    node_id       TEXT         NOT NULL,
    browse_path   TEXT,
    browse_name   TEXT         NOT NULL,
    datatype      TEXT,
    access_level  INTEGER      NOT NULL DEFAULT 1,
    last_seen     TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    PRIMARY KEY (plc_id, node_id)
);

-- Audit log: before/after snapshots for every entity mutation.
CREATE TABLE audit_log (
    id         BIGSERIAL    PRIMARY KEY,
    entity     TEXT         NOT NULL,
    entity_id  TEXT         NOT NULL,
    before     JSONB,
    after      JSONB,
    at         TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX audit_log_entity_idx ON audit_log (entity, entity_id, at DESC);
