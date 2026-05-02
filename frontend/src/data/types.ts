// Field values from the WS stream — DataType serialises as raw JSON primitives.
export type FieldValue   = number | boolean | string;
export type FieldValues  = Record<string, FieldValue>;
export type PlantSnapshot = Record<string, FieldValues>; // device_id → field_name → value

// Mirrors backend PlantConfig / PlcConfig / DeviceConfig from factory.json.
export interface PlantConfig {
  plant_id:    string;
  name:        string;
  description: string;
  plcs:        PlcConfig[];
}

export interface PlcConfig {
  plc_id:    string;
  name:      string;
  simulated: boolean;
  protocol:  string;
  uri:       string;
  port:      number;
  endpoint:  string;
  devices:   DeviceConfig[];
}

export interface DeviceConfig {
  device_id:       string;
  name:            string;
  device_type:     string;
  input_variables: InputVariable[];
  tick_ms:         number | null;
  params:          Record<string, number>;
}

export interface InputVariable {
  name:             string;
  source_device_id: string;
  source_field:     string;
}
