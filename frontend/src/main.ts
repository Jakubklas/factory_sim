import * as THREE from 'three';
import { CSS2DRenderer, CSS2DObject } from 'three/examples/jsm/renderers/CSS2DRenderer.js';
import { setupScene } from './scene/setup';
import { createFactoryFloor } from './scene/factory-floor';
import { subscribe, connectToBackend } from './data/state';
import { fetchPlantConfig } from './data/plant-client';
import type { DeviceConfig, FieldValues } from './data/types';
import { createBoiler, updateBoilerState } from './objects/boiler';
import { createFlowMeter, updateFlowMeterState } from './objects/flow-meter';
import { createValve, updateValveState } from './objects/valve';
import { createPipe } from './objects/pipe';
import { createPlcNode } from './objects/plc-node';
import {
  createLabel, updateLabel, createPlcLabel,
  formatBoilerLabel, formatFlowMeterLabel, formatValveLabel, formatPlcLabel,
} from './overlays/label';
import './overlays/styles.css';

const PIPE_Y  = 1.5;
const SPACING = 10;
const PLC_Y   = 14;  // height PLCs hover above the floor

// How far each device type's geometry extends from its group centre on the X axis.
// Used to connect pipe endpoints flush with device edges.
const HALF_WIDTHS: Record<string, number> = {
  Boiler:    2.0,
  Valve:     1.75,
  FlowMeter: 1.0,
};

interface DeviceEntry {
  deviceId:   string;
  deviceType: string;
  position:   THREE.Vector3;
  group:      THREE.Group;
  label:      CSS2DObject;
  update:     (fields: FieldValues) => void;
}

function buildEntry(device: DeviceConfig, position: THREE.Vector3): DeviceEntry | null {
  switch (device.device_type) {
    case 'Boiler': {
      const group = createBoiler(device.device_id, position);
      const label = createLabel(device.device_id, `${device.name}\nInitializing...`);
      group.add(label);
      return {
        deviceId: device.device_id, deviceType: 'Boiler', position, group, label,
        update: (s) => {
          console.log(`[render] boiler ${device.device_id}: temp=${s.temperature} status=${s.status}`);
          updateBoilerState(group, s.temperature as number, s.target_temperature as number, s.status as string);
          updateLabel(label, formatBoilerLabel(
            device.name, s.temperature as number, s.target_temperature as number,
            s.pressure as number, s.status as string,
          ), s.status as string);
        },
      };
    }
    case 'Valve': {
      const group = createValve(device.device_id, position);
      const label = createLabel(device.device_id, `${device.name}\nInitializing...`);
      group.add(label);
      return {
        deviceId: device.device_id, deviceType: 'Valve', position, group, label,
        update: (s) => {
          console.log(`[render] valve ${device.device_id}: pos=${s.position} status=${s.status}`);
          updateValveState(group, s.position as number, s.status as string, s.status as string);
          updateLabel(label, formatValveLabel(
            device.name, s.position as number, s.status as string, s.status as string,
          ), s.status as string);
        },
      };
    }
    case 'FlowMeter': {
      const group = createFlowMeter(device.device_id, position);
      const label = createLabel(device.device_id, `${device.name}\nInitializing...`);
      group.add(label);
      return {
        deviceId: device.device_id, deviceType: 'FlowMeter', position, group, label,
        update: (s) => {
          console.log(`[render] flowmeter ${device.device_id}: flow=${s.flow_rate} status=${s.status}`);
          updateFlowMeterState(group, s.flow_rate as number, s.total_volume as number, s.status as string);
          updateLabel(label, formatFlowMeterLabel(
            device.name, s.flow_rate as number, s.total_volume as number, s.status as string,
          ), s.status as string);
        },
      };
    }
    default:
      console.warn(`[scene] unknown device_type "${device.device_type}" — skipping ${device.device_id}`);
      return null;
  }
}

async function init() {
  const container = document.getElementById('app');
  if (!container) throw new Error('App container not found');

  // Plant config drives the entire scene — no hardcoded device IDs or names.
  const plant   = await fetchPlantConfig();
  const devices = plant.plcs.flatMap(plc => plc.devices);

  const { scene, camera, renderer, controls } = setupScene(container);

  const labelRenderer = new CSS2DRenderer();
  labelRenderer.setSize(window.innerWidth, window.innerHeight);
  labelRenderer.domElement.style.position = 'absolute';
  labelRenderer.domElement.style.top = '0px';
  labelRenderer.domElement.style.pointerEvents = 'none';
  container.appendChild(labelRenderer.domElement);

  createFactoryFloor(scene);

  // Spread devices evenly along X, centred on origin.
  const totalWidth = (devices.length - 1) * SPACING;
  const entries: DeviceEntry[] = [];

  for (let i = 0; i < devices.length; i++) {
    const position = new THREE.Vector3(-totalWidth / 2 + i * SPACING, 0, 0);
    const entry    = buildEntry(devices[i], position);
    if (!entry) continue;
    scene.add(entry.group);
    entries.push(entry);
  }

  // Connect consecutive devices with pipes, flush to each device's edge.
  for (let i = 0; i < entries.length - 1; i++) {
    const a     = entries[i];
    const b     = entries[i + 1];
    const halfA = HALF_WIDTHS[a.deviceType] ?? 1;
    const halfB = HALF_WIDTHS[b.deviceType] ?? 1;
    scene.add(createPipe(
      new THREE.Vector3(a.position.x + halfA, PIPE_Y, 0),
      new THREE.Vector3(b.position.x - halfB, PIPE_Y, 0),
    ));
  }

  // PLC nodes — one per PLC, hovering above its connected devices.
  for (const plc of plant.plcs) {
    const connected = entries.filter(e => plc.devices.some(d => d.device_id === e.deviceId));
    if (connected.length === 0) continue;

    // Centre horizontally between the most physically distant connected devices.
    const xs   = connected.map(e => e.position.x);
    const plcX = (Math.min(...xs) + Math.max(...xs)) / 2;
    const plcPos = new THREE.Vector3(plcX, PLC_Y, 0);

    const plcGroup = createPlcNode(plc.plc_id, plcPos);
    plcGroup.add(createPlcLabel(formatPlcLabel(plc.name, plc.protocol, plc.simulated)));
    scene.add(plcGroup);

    // Dashed line from each connected device up to the PLC.
    const lineMat = new THREE.LineDashedMaterial({
      color: 0x9090b0,
      transparent: true,
      opacity: 0.4,
      dashSize: 0.35,
      gapSize: 0.2,
    });
    // Route: up 40% → horizontal to PLC x → up remaining 60% to PLC bottom.
    const startY = 3.5;
    const connY  = PLC_Y - 0.28;
    const bendY  = startY + 0.4 * (connY - startY);
    for (const entry of connected) {
      const geo = new THREE.BufferGeometry().setFromPoints([
        new THREE.Vector3(entry.position.x, startY, 0),
        new THREE.Vector3(entry.position.x, bendY,  0),
        new THREE.Vector3(plcX,             bendY,  0),
        new THREE.Vector3(plcX,             connY,  0),
      ]);
      const line = new THREE.Line(geo, lineMat);
      line.computeLineDistances();
      scene.add(line);
    }
  }

  // Subscribe to live WS state — device IDs come from the config, not hardcoded.
  for (const entry of entries) {
    subscribe(entry.deviceId, entry.update);
  }

  connectToBackend();

  function animate() {
    requestAnimationFrame(animate);
    controls.update();
    renderer.render(scene, camera);
    labelRenderer.render(scene, camera);
  }
  animate();

  window.addEventListener('resize', () => {
    camera.aspect = window.innerWidth / window.innerHeight;
    camera.updateProjectionMatrix();
    renderer.setSize(window.innerWidth, window.innerHeight);
    labelRenderer.setSize(window.innerWidth, window.innerHeight);
  });
}

init();
