import { defineConfig, Plugin } from 'vite';
import { execSync } from 'child_process';

function portGuard(): Plugin {
  let port = 5173;

  // Mirrors backend port_guard.rs: kill any process on the port, never self.
  const release = (p: number) => {
    try {
      const pids = execSync(`lsof -ti TCP:${p} 2>/dev/null`, { stdio: 'pipe' })
        .toString().trim().split(/\s+/).filter(Boolean)
        .map(Number)
        .filter(pid => pid > 0 && pid !== process.pid);
      if (pids.length > 0) {
        execSync(`kill -9 ${pids.join(' ')}`, { stdio: 'pipe' });
        console.log(`[port-guard] released port ${p} (killed PID${pids.length > 1 ? 's' : ''} ${pids.join(', ')})`);
      }
    } catch { /* port already free */ }
  };

  return {
    name: 'port-guard',
    configResolved(config) {
      port = config.server?.port ?? 5173;
    },
    configureServer(server) {
      // Startup sweep: kill any stale process before Vite binds
      release(port);

      // Shutdown sweep: final cleanup after HTTP server closes
      server.httpServer?.once('close', () => release(port));
    },
  };
}

export default defineConfig({
  plugins: [portGuard()],
  server: { port: 5173 },
});
