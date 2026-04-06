pub mod factory_handle;

pub use factory_handle::FactoryHandle;

// Planned next:
//   traits.rs  — DeviceFunction trait, PhysicsFunction trait
//   device.rs  — Device struct (live state: fields HashMap, Arc<dyn ...>)
//   plant.rs   — PlantRuntime, simulation loop
