# backend

The orchestrator: browses and polls every PLC over OPC-UA, routes wires, writes setpoints,
serves the API/WebSocket, persists to Postgres, and provisions simulated PLC pods.

See **[../docs/backend.md](../docs/backend.md)** for internals, and
**[../README.md](../README.md)** for the project overview.
