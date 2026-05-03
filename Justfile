config_dir := justfile_directory() + "/config"

# Start the simulator process (serves simulated PLCs over OPC-UA)
sim:
    PLANT_CONFIG={{config_dir}} cargo run -p simulator

# Start the backend (API + connectors). Simulator must be running first.
be:
    mkdir -p target/debug/config
    cp {{config_dir}}/* target/debug/config/
    cargo run -p backend
