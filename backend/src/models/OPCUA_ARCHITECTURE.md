# OPC UA Address Space Architecture

## Hierarchical Structure

```
Root (ObjectsFolder)
│
├─ PLC-1 (Folder)
│  ├─ Boiler1 (Folder)
│  │  ├─ Temperature (Variable: Double)
│  │  ├─ TargetTemperature (Variable: Double)
│  │  ├─ Pressure (Variable: Double)
│  │  └─ Status (Variable: String)
│  │
│  ├─ PressureMeter1 (Folder)
│  │  ├─ Pressure (Variable: Double)
│  │  └─ Status (Variable: String)
│  │
│  └─ Valve1 (Folder)
│     ├─ Position (Variable: Double)
│     ├─ Mode (Variable: String)
│     └─ Status (Variable: String)
│
└─ PLC-2 (Folder)
   ├─ Boiler2 (Folder)
   │  ├─ Temperature (Variable: Double)
   │  ├─ TargetTemperature (Variable: Double)
   │  ├─ Pressure (Variable: Double)
   │  └─ Status (Variable: String)
   │
   └─ FlowMeter1 (Folder)
      ├─ FlowRate (Variable: Double)
      ├─ TotalVolume (Variable: Double)
      └─ Status (Variable: String)
```

## Naming Conventions

### Layers
1. **Root Layer**: ObjectsFolder (OPC UA standard)
2. **PLC Layer**: `config.name` → Folder (PascalCase with hyphens, e.g., "PLC-1")
3. **Device Layer**: `device_mapping.folder_name` → Folder (PascalCase, e.g., "Boiler1")
4. **Variable Layer**: `metric.node_path` → Variable node (PascalCase, e.g., "Temperature")

### NodeId Format
- **Pattern**: `ns=2;s=PLC-1.Boiler1.Temperature`
- **Namespace**: Always `2` for application-defined nodes
- **Identifier**: String format `{PLC}.{Device}.{Metric}`

### Configuration Mapping
- **`node_path`** (MetricConfig): OPC UA node identifier (e.g., "Boiler1.Temperature")
- **`field_name`** (MetricConfig): Rust struct field name (snake_case, e.g., "temperature")
- **`folder_name`** (DeviceMapping): OPC UA folder name (PascalCase, e.g., "Boiler1")
- **`device_id`** (DeviceMapping): Internal simulator ID (kebab-case, e.g., "boiler-1")

## Address Space Construction

1. **PLC Server** reads `PlcConfig` from JSON
2. Creates folder for `config.name` under ObjectsFolder
3. For each `DeviceMapping`, creates folder `device_mapping.folder_name`
4. For each `MetricConfig`, creates Variable node with NodeId `ns=2;s={folder_name}.{node_path}`
5. Update loop matches `DeviceHandle` to read Rust struct field using `field_name`

## Example NodeId Resolution

```json
{
  "node_path": "Boiler1.Temperature",
  "field_name": "temperature",
  "data_type": "Double"
}
```

- **OPC UA NodeId**: `ns=2;s=PLC-1.Boiler1.Temperature`
- **Rust Access**: `boiler.read().await.temperature`
- **Full Path**: ObjectsFolder → PLC-1 → Boiler1 → Temperature
