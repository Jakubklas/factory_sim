import { CSS2DObject } from 'three/examples/jsm/renderers/CSS2DRenderer.js';

export function createLabel(
  id: string,
  content: string,
  status: string = 'Normal'
): CSS2DObject {
  const div = document.createElement('div');
  div.className = 'device-label';
  div.setAttribute('data-status', status.toLowerCase());
  div.innerHTML = content;

  const label = new CSS2DObject(div);
  label.position.set(0, 5, 0);

  return label;
}

export function updateLabel(label: CSS2DObject, content: string, status: string = 'Normal') {
  const div = label.element as HTMLDivElement;
  div.innerHTML = content;
  div.setAttribute('data-status', status.toLowerCase());
}

export function formatBoilerLabel(
  id: string,
  temperature: number,
  targetTemperature: number,
  pressure: number,
  status: string
): string {
  return `
    <div class="label-header">${id.toUpperCase()}</div>
    <div class="label-content">
      <div>${temperature.toFixed(1)}°C → ${targetTemperature.toFixed(1)}°C</div>
      <div>${pressure.toFixed(1)} bar</div>
      <div class="status-indicator">● ${status}</div>
    </div>
  `;
}

export function formatPressureMeterLabel(
  id: string,
  pressure: number,
  status: string
): string {
  return `
    <div class="label-header">${id.toUpperCase()}</div>
    <div class="label-content">
      <div>${pressure.toFixed(2)} bar</div>
      <div class="status-indicator">● ${status}</div>
    </div>
  `;
}

export function formatFlowMeterLabel(
  id: string,
  flowRate: number,
  totalVolume: number,
  status: string
): string {
  return `
    <div class="label-header">${id.toUpperCase()}</div>
    <div class="label-content">
      <div>${flowRate.toFixed(1)} L/min</div>
      <div>${totalVolume.toFixed(0)} L total</div>
      <div class="status-indicator">● ${status}</div>
    </div>
  `;
}

export function formatValveLabel(
  id: string,
  position: number,
  mode: string,
  status: string
): string {
  return `
    <div class="label-header">${id.toUpperCase()}</div>
    <div class="label-content">
      <div>${(position * 100).toFixed(0)}% open</div>
      <div>${mode} mode</div>
      <div class="status-indicator">● ${status}</div>
    </div>
  `;
}
