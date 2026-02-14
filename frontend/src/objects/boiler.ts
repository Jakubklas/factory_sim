import * as THREE from 'three';

export function createBoiler(id: string, position: THREE.Vector3): THREE.Group {
  const group = new THREE.Group();
  group.position.copy(position);

  const bodyGeometry = new THREE.CylinderGeometry(2, 2, 8, 16);
  const bodyMaterial = new THREE.MeshStandardMaterial({
    color: 0x2a2a35,
    roughness: 0.6,
    metalness: 0.8,
    emissive: 0x0044ff,
    emissiveIntensity: 0.0,
  });
  const body = new THREE.Mesh(bodyGeometry, bodyMaterial);
  body.castShadow = true;
  body.receiveShadow = true;
  group.add(body);

  const topGeometry = new THREE.SphereGeometry(2, 16, 8, 0, Math.PI * 2, 0, Math.PI / 2);
  const top = new THREE.Mesh(topGeometry, bodyMaterial);
  top.position.y = 4;
  top.castShadow = true;
  group.add(top);

  group.userData = { id, type: 'boiler', material: bodyMaterial };

  return group;
}

export function updateBoilerState(
  boiler: THREE.Group,
  temperature: number,
  targetTemperature: number,
  status: string
) {
  const material = boiler.userData.material as THREE.MeshStandardMaterial;

  const normalizedTemp = Math.min(temperature / 100, 1);
  const coldColor = new THREE.Color(0x0044ff);
  const hotColor = new THREE.Color(0xff4400);

  material.emissive.copy(coldColor.lerp(hotColor, normalizedTemp));
  material.emissiveIntensity = 0.3 + normalizedTemp * 0.5;
}
