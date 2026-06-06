-- Runtime integrity fixes

-- 1. The original plc_kind_check required simulated PLCs to always have
--    deploy_node set and real PLCs to always have endpoint_uri set.
--    In practice a PLC may be created before its deploy_node or URI is
--    assigned (e.g. the frontend creates the row first, then the user picks
--    a node). Drop the constraint so the API doesn't reject valid workflows.
ALTER TABLE plcs DROP CONSTRAINT IF EXISTS plc_kind_check;

-- 2. Extend the plant_changed NOTIFY to device_instances and wires so the
--    backend's LISTEN loop refreshes the wire table automatically whenever
--    instances or wires change via the API — not just when plcs change.
CREATE OR REPLACE FUNCTION notify_plant_changed() RETURNS TRIGGER AS $$
BEGIN
  PERFORM pg_notify('plant_changed', TG_OP || ':' || TG_TABLE_NAME);
  RETURN NULL;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS instances_notify ON device_instances;
CREATE TRIGGER instances_notify
  AFTER INSERT OR UPDATE OR DELETE ON device_instances
  FOR EACH STATEMENT EXECUTE FUNCTION notify_plant_changed();

DROP TRIGGER IF EXISTS wires_notify ON wires;
CREATE TRIGGER wires_notify
  AFTER INSERT OR UPDATE OR DELETE ON wires
  FOR EACH STATEMENT EXECUTE FUNCTION notify_plant_changed();

-- 3. wires.src_device_id is a plain TEXT column (no FK) so deleting a source
--    device_instance leaves orphaned wire rows.  An orphaned wire causes
--    ResolvedPlant::build() to fail at startup.  Add a row-level trigger to
--    clean up wires whose source device is removed.
CREATE OR REPLACE FUNCTION cleanup_wires_for_deleted_instance() RETURNS TRIGGER AS $$
BEGIN
  IF OLD.device_id IS NOT NULL THEN
    DELETE FROM wires
    WHERE src_plc_id   = OLD.plc_id
      AND src_device_id = OLD.device_id;
  END IF;
  RETURN OLD;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS device_instances_src_cleanup ON device_instances;
CREATE TRIGGER device_instances_src_cleanup
  AFTER DELETE ON device_instances
  FOR EACH ROW EXECUTE FUNCTION cleanup_wires_for_deleted_instance();
