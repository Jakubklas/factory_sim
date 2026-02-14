import * as THREE from 'three';
import { CSS2DRenderer } from 'three/examples/jsm/renderers/CSS2DRenderer.js';
import { setupScene } from './scene/setup';
import { createFactoryFloor } from './scene/factory-floor';
import { connectWebSocket } from './data/websocket';
import { subscribe } from './data/state';
import { createBoiler, updateBoilerState } from './objects/boiler';
import { createPressureMeter, updatePressureMeterState } from './objects/pressure-meter';
import { createFlowMeter, updateFlowMeterState } from './objects/flow-meter';
import { createValve, updateValveState } from './objects/valve';
import { createPipe } from './objects/pipe';
import {
  createLabel,
  updateLabel,
  formatBoilerLabel,
  formatPressureMeterLabel,
  formatFlowMeterLabel,
  formatValveLabel
} from './overlays/label';
import './overlays/styles.css';

async function init() {
  const container = document.getElementById('app');
  if (!container) {
    throw new Error('App container not found');
  }

  const { scene, camera, renderer, controls } = setupScene(container);

  // Create CSS2DRenderer for labels
  const labelRenderer = new CSS2DRenderer();
  labelRenderer.setSize(window.innerWidth, window.innerHeight);
  labelRenderer.domElement.style.position = 'absolute';
  labelRenderer.domElement.style.top = '0px';
  labelRenderer.domElement.style.pointerEvents = 'none';
  container.appendChild(labelRenderer.domElement);

  createFactoryFloor(scene);

  // Create 3D devices with proper positioning
  // Layout: Boiler1 → PressureMeter → Valve → FlowMeter → Boiler2

  const boiler1 = createBoiler('boiler-1', new THREE.Vector3(-15, 0, 0));
  scene.add(boiler1);
  const boiler1Label = createLabel('boiler-1', 'BOILER-1\nInitializing...');
  boiler1.add(boiler1Label);

  const pressureMeter = createPressureMeter('pressure-meter-1', new THREE.Vector3(-7, 0, 0));
  scene.add(pressureMeter);
  const pressureMeterLabel = createLabel('pressure-meter-1', 'PRESSURE METER 1\nInitializing...');
  pressureMeter.add(pressureMeterLabel);

  const valve = createValve('valve-1', new THREE.Vector3(0, 0, 0));
  scene.add(valve);
  const valveLabel = createLabel('valve-1', 'VALVE 1\nInitializing...');
  valve.add(valveLabel);

  const flowMeter = createFlowMeter('flow-meter-1', new THREE.Vector3(7, 0, 0));
  scene.add(flowMeter);
  const flowMeterLabel = createLabel('flow-meter-1', 'FLOW METER 1\nInitializing...');
  flowMeter.add(flowMeterLabel);

  const boiler2 = createBoiler('boiler-2', new THREE.Vector3(15, 0, 0));
  scene.add(boiler2);
  const boiler2Label = createLabel('boiler-2', 'BOILER-2\nInitializing...');
  boiler2.add(boiler2Label);

  // Create pipes connecting devices
  const pipe1 = createPipe(new THREE.Vector3(-13, 1.5, 0), new THREE.Vector3(-7, 1.5, 0));
  scene.add(pipe1);

  const pipe2 = createPipe(new THREE.Vector3(-7, 1.5, 0), new THREE.Vector3(-2, 1.5, 0));
  scene.add(pipe2);

  const pipe3 = createPipe(new THREE.Vector3(2, 1.5, 0), new THREE.Vector3(7, 1.5, 0));
  scene.add(pipe3);

  const pipe4 = createPipe(new THREE.Vector3(7, 1.5, 0), new THREE.Vector3(13, 1.5, 0));
  scene.add(pipe4);

  // Subscribe to device state updates
  subscribe('boiler-1', (state: any) => {
    updateBoilerState(boiler1, state.temperature, state.target_temperature, state.status);
    updateLabel(boiler1Label, formatBoilerLabel(
      state.id,
      state.temperature,
      state.target_temperature,
      state.pressure,
      state.status
    ), state.status);
  });

  subscribe('boiler-2', (state: any) => {
    updateBoilerState(boiler2, state.temperature, state.target_temperature, state.status);
    updateLabel(boiler2Label, formatBoilerLabel(
      state.id,
      state.temperature,
      state.target_temperature,
      state.pressure,
      state.status
    ), state.status);
  });

  subscribe('pressure-meter-1', (state: any) => {
    updatePressureMeterState(pressureMeter, state.pressure, state.status);
    updateLabel(pressureMeterLabel, formatPressureMeterLabel(
      state.id,
      state.pressure,
      state.status
    ), state.status);
  });

  subscribe('valve-1', (state: any) => {
    updateValveState(valve, state.position, state.mode, state.status);
    updateLabel(valveLabel, formatValveLabel(
      state.id,
      state.position,
      state.mode,
      state.status
    ), state.status);
  });

  subscribe('flow-meter-1', (state: any) => {
    updateFlowMeterState(flowMeter, state.flow_rate, state.total_volume, state.status);
    updateLabel(flowMeterLabel, formatFlowMeterLabel(
      state.id,
      state.flow_rate,
      state.total_volume,
      state.status
    ), state.status);
  });

  // Connect WebSocket to start receiving data
  connectWebSocket();

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
