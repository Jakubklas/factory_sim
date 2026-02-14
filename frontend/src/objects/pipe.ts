import * as THREE from 'three';

export function createPipe(
  start: THREE.Vector3,
  end: THREE.Vector3
): THREE.Group {
  const group = new THREE.Group();

  const direction = new THREE.Vector3().subVectors(end, start);
  const length = direction.length();
  const center = new THREE.Vector3().addVectors(start, end).multiplyScalar(0.5);

  const pipeGeometry = new THREE.CylinderGeometry(0.3, 0.3, length, 8);
  const pipeMaterial = new THREE.MeshStandardMaterial({
    color: 0x1a2a3a,
    roughness: 0.7,
    metalness: 0.8,
  });

  const pipe = new THREE.Mesh(pipeGeometry, pipeMaterial);
  pipe.position.copy(center);

  const axis = new THREE.Vector3(0, 1, 0);
  pipe.quaternion.setFromUnitVectors(axis, direction.clone().normalize());

  pipe.castShadow = true;
  pipe.receiveShadow = true;
  group.add(pipe);

  return group;
}

export function createFlowParticles(
  start: THREE.Vector3,
  end: THREE.Vector3,
  flowRate: number
): THREE.Points {
  const particleCount = Math.floor(flowRate * 2);
  const geometry = new THREE.BufferGeometry();
  const positions = new Float32Array(particleCount * 3);

  for (let i = 0; i < particleCount; i++) {
    const t = i / particleCount;
    positions[i * 3] = start.x + (end.x - start.x) * t;
    positions[i * 3 + 1] = start.y + (end.y - start.y) * t;
    positions[i * 3 + 2] = start.z + (end.z - start.z) * t;
  }

  geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));

  const material = new THREE.PointsMaterial({
    color: 0x00ffff,
    size: 0.2,
    transparent: true,
    opacity: 0.8,
  });

  return new THREE.Points(geometry, material);
}
