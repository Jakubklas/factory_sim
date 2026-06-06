-- Add plc_id (human slug like "plc-001"), protocol, and endpoint path to plcs.
-- These fields are needed to round-trip PlcConfig from the DB.
ALTER TABLE plcs
  ADD COLUMN IF NOT EXISTS plc_id   VARCHAR(128),
  ADD COLUMN IF NOT EXISTS protocol VARCHAR(16) NOT NULL DEFAULT 'opcua',
  ADD COLUMN IF NOT EXISTS endpoint VARCHAR(64) NOT NULL DEFAULT '/';

CREATE UNIQUE INDEX IF NOT EXISTS plcs_plc_id_key ON plcs(plc_id) WHERE plc_id IS NOT NULL;

-- Add device_id (slug like "boiler_001") to device_instances.
-- Needed to reconstruct DeviceConfig from DB and to compute OPC-UA node paths.
ALTER TABLE device_instances
  ADD COLUMN IF NOT EXISTS device_id VARCHAR(128);

-- Replace src_node (opaque OPC-UA path string) with typed source fields on wires.
-- src_device_id: the originating device's device_id slug (e.g. "boiler_001")
-- src_field:     the metric/output name    (e.g. "pressure")
-- Together they let orchestration look up the value from IngestedState and compute
-- the destination OPC-UA node path without hardcoding path formats in SQL.
ALTER TABLE wires
  ADD COLUMN IF NOT EXISTS src_device_id VARCHAR(128),
  ADD COLUMN IF NOT EXISTS src_field     VARCHAR(128);

ALTER TABLE wires DROP COLUMN IF EXISTS src_node;

-- Postgres NOTIFY: fires on any PLC insert/update/delete so the backend's
-- LISTEN loop can start new connectors within one reconcile interval
-- without requiring a restart.
CREATE OR REPLACE FUNCTION notify_plant_changed() RETURNS TRIGGER AS $$
BEGIN
  PERFORM pg_notify('plant_changed', TG_OP || ':plcs');
  RETURN NULL;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS plcs_notify ON plcs;
CREATE TRIGGER plcs_notify
  AFTER INSERT OR UPDATE OR DELETE ON plcs
  FOR EACH STATEMENT EXECUTE FUNCTION notify_plant_changed();
