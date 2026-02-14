import * as THREE from 'three';

export function createFactoryFloor(scene: THREE.Scene) {
  const groundGeometry = new THREE.PlaneGeometry(100, 100);
  const groundMaterial = new THREE.MeshStandardMaterial({
    color: 0x1a1a25,
    roughness: 0.8,
    metalness: 0.2,
  });
  const ground = new THREE.Mesh(groundGeometry, groundMaterial);
  ground.rotation.x = -Math.PI / 2;
  ground.receiveShadow = true;
  scene.add(ground);

  const gridHelper = new THREE.GridHelper(100, 50, 0x444466, 0x222233);
  gridHelper.position.y = 0.01;
  scene.add(gridHelper);

  const wallHeight = 15;
  const wallThickness = 0.5;
  const wallMaterial = new THREE.MeshStandardMaterial({
    color: 0x15151f,
    roughness: 0.9,
    metalness: 0.1,
  });

  const backWall = new THREE.Mesh(
    new THREE.BoxGeometry(100, wallHeight, wallThickness),
    wallMaterial
  );
  backWall.position.set(0, wallHeight / 2, -50);
  backWall.receiveShadow = true;
  backWall.castShadow = true;
  scene.add(backWall);

  const leftWall = new THREE.Mesh(
    new THREE.BoxGeometry(wallThickness, wallHeight, 100),
    wallMaterial
  );
  leftWall.position.set(-50, wallHeight / 2, 0);
  leftWall.receiveShadow = true;
  leftWall.castShadow = true;
  scene.add(leftWall);
}
