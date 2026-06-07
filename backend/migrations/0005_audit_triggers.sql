-- Populate audit_log automatically, and provide a single-step revert.
--
-- A generic row-level trigger snapshots OLD/NEW into audit_log on every
-- INSERT/UPDATE/DELETE of the user-authored tables. revert_audit(id) undoes one
-- recorded step. (deploy_nodes / discovered_plc_nodes are machine-synced caches,
-- so they're intentionally NOT audited.)

-- ── Audit trigger ────────────────────────────────────────────────────────────
-- TG_ARGV[0] = the row's primary-key column, so entity_id is the real key.
CREATE OR REPLACE FUNCTION audit_row() RETURNS TRIGGER AS $$
DECLARE
  key_col TEXT := TG_ARGV[0];
  b JSONB;
  a JSONB;
BEGIN
  IF    TG_OP = 'DELETE' THEN b := to_jsonb(OLD); a := NULL;
  ELSIF TG_OP = 'INSERT' THEN b := NULL;          a := to_jsonb(NEW);
  ELSE                        b := to_jsonb(OLD); a := to_jsonb(NEW);
  END IF;

  INSERT INTO audit_log (entity, entity_id, before, after)
  VALUES (TG_TABLE_NAME, COALESCE(a ->> key_col, b ->> key_col), b, a);

  IF TG_OP = 'DELETE' THEN RETURN OLD; ELSE RETURN NEW; END IF;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS device_types_audit     ON device_types;
DROP TRIGGER IF EXISTS plcs_audit             ON plcs;
DROP TRIGGER IF EXISTS device_instances_audit ON device_instances;
DROP TRIGGER IF EXISTS wires_audit            ON wires;

CREATE TRIGGER device_types_audit     AFTER INSERT OR UPDATE OR DELETE ON device_types
  FOR EACH ROW EXECUTE FUNCTION audit_row('name');
CREATE TRIGGER plcs_audit             AFTER INSERT OR UPDATE OR DELETE ON plcs
  FOR EACH ROW EXECUTE FUNCTION audit_row('id');
CREATE TRIGGER device_instances_audit AFTER INSERT OR UPDATE OR DELETE ON device_instances
  FOR EACH ROW EXECUTE FUNCTION audit_row('id');
CREATE TRIGGER wires_audit            AFTER INSERT OR UPDATE OR DELETE ON wires
  FOR EACH ROW EXECUTE FUNCTION audit_row('id');

-- ── Single-step revert ───────────────────────────────────────────────────────
-- before IS NULL  → step was an INSERT  → delete the row
-- after  IS NULL  → step was a DELETE   → re-insert `before`
-- both set        → step was an UPDATE  → restore columns to `before` (in place,
--                                          so parent-row reverts don't cascade-delete children)
-- The revert is itself audited (its mutation hits the trigger), so it's undoable too.
CREATE OR REPLACE FUNCTION revert_audit(p_id BIGINT) RETURNS VOID AS $$
DECLARE
  r          audit_log;
  key_col    TEXT;
  set_clause TEXT;
BEGIN
  SELECT * INTO r FROM audit_log WHERE id = p_id;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'audit step % not found', p_id USING ERRCODE = 'no_data_found';
  END IF;

  key_col := CASE r.entity WHEN 'device_types' THEN 'name' ELSE 'id' END;

  IF r.after IS NULL THEN
    -- undo a DELETE → re-insert the old row verbatim
    EXECUTE format('INSERT INTO %I SELECT * FROM jsonb_populate_record(NULL::%I, $1)',
                   r.entity, r.entity)
      USING r.before;
  ELSIF r.before IS NULL THEN
    -- undo an INSERT → delete the row
    EXECUTE format('DELETE FROM %I WHERE %I::text = $1', r.entity, key_col)
      USING r.entity_id;
  ELSE
    -- undo an UPDATE → restore the old column values in place
    SELECT string_agg(format('%I = src.%I', column_name, column_name), ', ')
      INTO set_clause
      FROM information_schema.columns
     WHERE table_schema = 'public' AND table_name = r.entity;
    EXECUTE format(
      'UPDATE %I AS t SET %s FROM jsonb_populate_record(NULL::%I, $1) AS src WHERE t.%I::text = $2',
      r.entity, set_clause, r.entity, key_col)
      USING r.before, r.entity_id;
  END IF;
END;
$$ LANGUAGE plpgsql;
