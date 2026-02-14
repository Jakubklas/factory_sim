import * as THREE from 'three';

export function createValve(id: string, position: THREE.Vector3): THREE.Group {
  const group = new THREE.Group();
  group.position.copy(position);

  const flangeGeometry = new THREE.CylinderGeometry(1.5, 1.5, 0.5, 16);
  const flangeMaterial = new THREE.MeshStandardMaterial({
    color: 0x3a3a45,
    roughness: 0.6,
    metalness: 0.8,
  });

  const flange1 = new THREE.Mesh(flangeGeometry, flangeMaterial);
  flange1.position.set(-1.5, 1.5, 0);
  flange1.rotation.z = Math.PI / 2;
  flange1.castShadow = true;
  group.add(flange1);

  const flange2 = new THREE.Mesh(flangeGeometry, flangeMaterial);
  flange2.position.set(1.5, 1.5, 0);
  flange2.rotation.z = Math.PI / 2;
  flange2.castShadow = true;
  group.add(flange2);

  const handleGeometry = new THREE.CylinderGeometry(0.3, 0.3, 2, 8);
  const handleMaterial = new THREE.MeshStandardMaterial({
    color: 0xff4400,
    roughness: 0.5,
    metalness: 0.7,
    emissive: 0xff2200,
    emissiveIntensity: 0.2,
  });
  const handle = new THREE.Mesh(handleGeometry, handleMaterial);
  handle.position.set(0, 3, 0);
  handle.castShadow = true;
  group.add(handle);

  group.userData = { id, type: 'valve', handle };

  return group;
}

export function updateValveState(
  valve: THREE.Group,
  position: number,
  mode: string,
  status: string
) {
  const handle = valve.userData.handle as THREE.Mesh;

  const rotation = position * Math.PI / 2;
  handle.rotation.z = rotation;
}
