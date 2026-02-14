import * as THREE from 'three';

export function createPressureMeter(id: string, position: THREE.Vector3): THREE.Group {
  const group = new THREE.Group();
  group.position.copy(position);

  const standGeometry = new THREE.BoxGeometry(0.5, 3, 0.5);
  const standMaterial = new THREE.MeshStandardMaterial({
    color: 0x3a3a45,
    roughness: 0.7,
    metalness: 0.6,
  });
  const stand = new THREE.Mesh(standGeometry, standMaterial);
  stand.position.y = 1.5;
  stand.castShadow = true;
  group.add(stand);

  const dialGeometry = new THREE.SphereGeometry(1, 16, 16);
  const dialMaterial = new THREE.MeshStandardMaterial({
    color: 0x2a2a35,
    roughness: 0.5,
    metalness: 0.8,
    emissive: 0x00ff00,
    emissiveIntensity: 0.2,
  });
  const dial = new THREE.Mesh(dialGeometry, dialMaterial);
  dial.position.y = 3.5;
  dial.castShadow = true;
  group.add(dial);

  const ringGeometry = new THREE.TorusGeometry(1.2, 0.1, 8, 32);
  const ringMaterial = new THREE.MeshStandardMaterial({
    color: 0x555566,
    roughness: 0.4,
    metalness: 0.9,
  });
  const ring = new THREE.Mesh(ringGeometry, ringMaterial);
  ring.position.y = 3.5;
  ring.rotation.x = Math.PI / 2;
  group.add(ring);

  group.userData = { id, type: 'pressure-meter', material: dialMaterial };

  return group;
}

export function updatePressureMeterState(
  meter: THREE.Group,
  pressure: number,
  status: string
) {
  const material = meter.userData.material as THREE.MeshStandardMaterial;

  switch (status) {
    case 'Normal':
      material.emissive.setHex(0x00ff00);
      material.emissiveIntensity = 0.3;
      break;
    case 'Warning':
      material.emissive.setHex(0xffff00);
      material.emissiveIntensity = 0.4;
      break;
    case 'Critical':
      material.emissive.setHex(0xff0000);
      material.emissiveIntensity = 0.5;
      break;
  }
}
