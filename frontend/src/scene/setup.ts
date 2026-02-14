import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';

export function setupScene(container: HTMLElement) {
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x0a0a0f);
  scene.fog = new THREE.Fog(0x0a0a0f, 50, 200);

  const camera = new THREE.PerspectiveCamera(
    75,
    window.innerWidth / window.innerHeight,
    0.1,
    1000
  );
  camera.position.set(30, 20, 30);
  camera.lookAt(0, 0, 0);

  const renderer = new THREE.WebGLRenderer({ antialias: true });
  renderer.setSize(window.innerWidth, window.innerHeight);
  renderer.setPixelRatio(window.devicePixelRatio);
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  container.appendChild(renderer.domElement);

  const controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = true;
  controls.dampingFactor = 0.05;
  controls.maxPolarAngle = Math.PI / 2 - 0.1;
  controls.minDistance = 10;
  controls.maxDistance = 100;

  const ambientLight = new THREE.AmbientLight(0x404060, 0.3);
  scene.add(ambientLight);

  const mainLight = new THREE.DirectionalLight(0xffffff, 0.5);
  mainLight.position.set(20, 30, 20);
  mainLight.castShadow = true;
  mainLight.shadow.camera.left = -50;
  mainLight.shadow.camera.right = 50;
  mainLight.shadow.camera.top = 50;
  mainLight.shadow.camera.bottom = -50;
  scene.add(mainLight);

  const warmLight = new THREE.PointLight(0xff8844, 0.8, 30);
  warmLight.position.set(-10, 5, 0);
  scene.add(warmLight);

  const coolLight = new THREE.PointLight(0x4488ff, 0.6, 30);
  coolLight.position.set(10, 5, 0);
  scene.add(coolLight);

  return { scene, camera, renderer, controls };
}
