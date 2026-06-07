-- Natural key for device_types + descriptive renames.

-- 1. device_types: promote the natural key (name) to PRIMARY KEY and drop the
--    surrogate UUID id. Repoint device_instances from device_type_id (UUID) to
--    device_type_name. The rest of the system already identifies device types by
--    name (io_spec.device_type, the physics registry, plant.json, the simulator),
--    so the UUID was indirection nothing outside this FK used.

-- Backfill the new natural FK column on device_instances from the existing UUID FK.
ALTER TABLE device_instances ADD COLUMN device_type_name VARCHAR(128);
UPDATE device_instances di
   SET device_type_name = dt.name
  FROM device_types dt
 WHERE dt.id = di.device_type_id;
ALTER TABLE device_instances ALTER COLUMN device_type_name SET NOT NULL;

-- Drop the old surrogate FK column (also drops its FK constraint).
ALTER TABLE device_instances DROP COLUMN device_type_id;

-- Promote name to PK on device_types (drop the now-redundant UNIQUE(name), then
-- the UUID id — which drops the old PK with it — and make name the PK).
ALTER TABLE device_types DROP CONSTRAINT device_types_name_key;
ALTER TABLE device_types DROP COLUMN id;
ALTER TABLE device_types ADD PRIMARY KEY (name);

-- Add the natural FK. Renaming a type cascades to its instances; deleting a type
-- is blocked while instances reference it (matches prior behaviour).
ALTER TABLE device_instances
  ADD CONSTRAINT device_instances_device_type_name_fkey
  FOREIGN KEY (device_type_name) REFERENCES device_types(name)
  ON UPDATE CASCADE ON DELETE RESTRICT;

-- 2. Rename model_ref → model_3d_ref. (SQL identifiers can't start with a digit,
--    so model_3d_ref rather than 3d_model_ref.)
ALTER TABLE device_types RENAME COLUMN model_ref TO model_3d_ref;

-- 3. Rename discovered_nodes → discovered_plc_nodes (more descriptive).
ALTER TABLE discovered_nodes RENAME TO discovered_plc_nodes;
