# Water Cleaning Plant — Digital Twin

A real-time digital twin of a water cleaning plant with a Rust backend simulating OPC-UA devices and a TypeScript/Three.js frontend rendering a live 3D visualization.

## Prerequisites

Before running this project, ensure you have the following installed:

### Backend Requirements
- **Rust** (1.75 or later)
  - Install via [rustup](https://rustup.rs/): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Cargo** (comes with Rust)

### Frontend Requirements
- **Node.js** (18.x or later)
  - Download from [nodejs.org](https://nodejs.org/)
- **npm** (comes with Node.js)

## Project Structure

```
water-plant-twin/
├── backend/              # Rust simulation and servers
│   ├── Cargo.toml        # Rust dependencies
│   └── src/
│       ├── main.rs       # Entry point
│       ├── simulator/    # Device simulation and physics
│       ├── opcua_server/ # OPC-UA server (future)
│       └── ws_bridge/    # WebSocket bridge to frontend
│
├── frontend/             # TypeScript 3D visualization
│   ├── package.json      # Node dependencies
│   ├── tsconfig.json     # TypeScript config
│   ├── vite.config.ts    # Vite build config
│   ├── index.html        # HTML entry point
│   └── src/
│       ├── main.ts       # Frontend entry point
│       ├── scene/        # Three.js scene setup
│       ├── objects/      # 3D device geometries
│       ├── overlays/     # CSS2D labels and styling
│       └── data/         # WebSocket client and state management
│
├── architecture.md       # Detailed architecture documentation
└── README.md            # This file
```

## Installation

### 1. Install Backend Dependencies

```bash
cd backend
cargo build
```

This will download and compile all Rust dependencies listed in `Cargo.toml`.

### 2. Install Frontend Dependencies

```bash
cd ../frontend
npm install
```

This will download all Node.js packages listed in `package.json`.

## Running the Project

You'll need to run both the backend and frontend in separate terminal windows.

### Terminal 1: Start the Backend

```bash
cd backend
cargo run
```

This starts:
- **WebSocket server** on `ws://localhost:3000/ws`
- **OPC-UA server** on `opc.tcp://localhost:4840` (when implemented)

The backend will begin simulating the plant devices and broadcasting state updates over WebSocket.

### Terminal 2: Start the Frontend

```bash
cd frontend
npm run dev
```

This starts the Vite development server on `http://localhost:5173`.

Open your browser and navigate to **http://localhost:5173** to view the 3D digital twin.

## Development

### Backend Development

The backend is structured around three main modules:

1. **simulator/** — Device structs, plant topology, and physics simulation
2. **opcua_server/** — OPC-UA server setup (placeholder for Phase 2+)
3. **ws_bridge/** — Axum WebSocket server that broadcasts plant state to browsers

To modify simulation behavior, edit files in `backend/src/simulator/`.

### Frontend Development

The frontend uses Vite for hot module reloading. Changes to TypeScript files will automatically refresh the browser.

Key directories:
- **scene/** — Three.js setup, camera, lighting, environment
- **objects/** — 3D geometries for boilers, meters, valves, pipes
- **overlays/** — CSS2D labels with live device data
- **data/** — WebSocket client and reactive state store

### Build for Production

#### Backend
```bash
cd backend
cargo build --release
```
The optimized binary will be in `backend/target/release/water-plant-twin`.

#### Frontend
```bash
cd frontend
npm run build
```
Static files will be generated in `frontend/dist/`. Serve with any static file server.

## Current Implementation Status

This is a **Phase 1 skeleton** as outlined in [architecture.md](architecture.md):

- ✅ Project structure and build configuration
- ✅ Basic device structs with placeholder state
- ✅ Axum WebSocket server skeleton
- ✅ Three.js scene with ground plane, lighting, and camera
- ✅ 3D geometries for all device types (boiler, pressure meter, flow meter, valve, pipes)
- ✅ CSS2D label system with dark industrial styling
- ✅ WebSocket client with auto-reconnect
- ✅ State store with subscription system
- ⏳ **TODO:** Implement simulation physics (temperature ramping, pressure calculation, flow rates)
- ⏳ **TODO:** Connect devices to labels and update visuals on state changes
- ⏳ **TODO:** Implement OPC-UA server
- ⏳ **TODO:** Add pipe particle animations
- ⏳ **TODO:** Add post-processing effects (bloom)

See [architecture.md](architecture.md) for the full phase plan.

## Troubleshooting

### Backend fails to compile
- Ensure Rust is up to date: `rustup update`
- Check that all dependencies in `Cargo.toml` are accessible

### Frontend fails to start
- Ensure Node.js version is 18+: `node --version`
- Delete `node_modules` and `package-lock.json`, then run `npm install` again

### WebSocket connection fails
- Ensure the backend is running on port 3000
- Check browser console for connection errors
- Verify firewall settings allow localhost connections

### 3D scene doesn't render
- Check browser console for Three.js errors
- Ensure WebGL is supported in your browser
- Try a different browser (Chrome, Firefox, Edge recommended)

## License

This project is a demonstration and learning resource. Use freely.

## References

- [Rust OPC-UA Crate](https://github.com/locka99/opcua)
- [Axum Web Framework](https://github.com/tokio-rs/axum)
- [Three.js Documentation](https://threejs.org/docs/)
- [Vite Documentation](https://vite.dev/)
# factory_sim
