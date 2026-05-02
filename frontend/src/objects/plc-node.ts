import * as THREE from 'three';

export function createPlcNode(_id: string, position: THREE.Vector3): THREE.Group {
  const group = new THREE.Group();
  group.position.copy(position);

  // Main rack body — flat horizontal box like a 1U rack unit
  const bodyMat = new THREE.MeshStandardMaterial({
    color: 0xa8a8b8,
    roughness: 0.4,
    metalness: 0.8,
  });
  const body = new THREE.Mesh(new THREE.BoxGeometry(3, 0.55, 1.1), bodyMat);
  body.castShadow = true;
  group.add(body);

  // Front face panel — slightly recessed, darker
  const panelMat = new THREE.MeshStandardMaterial({
    color: 0x787888,
    roughness: 0.5,
    metalness: 0.7,
  });
  const panel = new THREE.Mesh(new THREE.BoxGeometry(2.5, 0.42, 0.04), panelMat);
  panel.position.set(0, 0, 0.57);
  group.add(panel);

  // Status LED — green glow
  const ledMat = new THREE.MeshStandardMaterial({
    color: 0x00ff44,
    emissive: 0x00ff44,
    emissiveIntensity: 1.0,
  });
  const led = new THREE.Mesh(new THREE.SphereGeometry(0.07, 8, 8), ledMat);
  led.position.set(1.05, 0.05, 0.6);
  group.add(led);

  group.userData = { id: _id, type: 'plc-node', ledMaterial: ledMat };
  return group;
}
