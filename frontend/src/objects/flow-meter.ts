import * as THREE from 'three';

export function createFlowMeter(id: string, position: THREE.Vector3): THREE.Group {
  const group = new THREE.Group();
  group.position.copy(position);

  const bodyGeometry = new THREE.BoxGeometry(2, 3, 2);
  const bodyMaterial = new THREE.MeshStandardMaterial({
    color: 0x2a3a45,
    roughness: 0.6,
    metalness: 0.7,
  });
  const body = new THREE.Mesh(bodyGeometry, bodyMaterial);
  body.position.y = 1.5;
  body.castShadow = true;
  body.receiveShadow = true;
  group.add(body);

  const displayGeometry = new THREE.BoxGeometry(1.5, 0.8, 0.1);
  const displayMaterial = new THREE.MeshStandardMaterial({
    color: 0x001122,
    emissive: 0x0066ff,
    emissiveIntensity: 0.4,
  });
  const display = new THREE.Mesh(displayGeometry, displayMaterial);
  display.position.set(0, 2, 1.05);
  group.add(display);

  group.userData = { id, type: 'flow-meter', material: bodyMaterial };

  return group;
}

export function updateFlowMeterState(
  meter: THREE.Group,
  flowRate: number,
  totalVolume: number,
  status: string
) {
  // TODO: Add animated particles showing flow
}
