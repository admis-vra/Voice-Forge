/* VoiceForge — animated 3D background.
 * A circular audio-waveform visualizer: rings of bars pulsing to layered sine waves,
 * like a voice equalizer, plus a breathing wireframe core. Fully deterministic —
 * no randomness, just rhythmic motion driven by time. */
(function () {
  const canvas = document.getElementById('bg-canvas');
  if (!canvas || typeof THREE === 'undefined') return;

  const prefersReduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(60, window.innerWidth / window.innerHeight, 0.1, 100);
  camera.position.set(0, 1.4, 8);
  camera.lookAt(0, 0, 0);

  const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  renderer.setSize(window.innerWidth, window.innerHeight);

  const BRAND = new THREE.Color(0x6d5efc);
  const CYAN = new THREE.Color(0x29d3ff);
  const PINK = new THREE.Color(0xff4d6d);

  const group = new THREE.Group();
  scene.add(group);

  // --- Breathing wireframe core, the "voice" at the center ---
  const coreGeo = new THREE.IcosahedronGeometry(1.3, 4);
  const coreBase = coreGeo.attributes.position.array.slice();
  const coreMat = new THREE.MeshBasicMaterial({ color: BRAND, wireframe: true, transparent: true, opacity: 0.5 });
  const core = new THREE.Mesh(coreGeo, coreMat);
  group.add(core);

  const glowMat = new THREE.MeshBasicMaterial({ color: CYAN, transparent: true, opacity: 0.06 });
  const glow = new THREE.Mesh(new THREE.IcosahedronGeometry(0.9, 2), glowMat);
  group.add(glow);

  // --- Sound-wave rings: concentric circles of bars, height driven by a waveform function ---
  const RINGS = [
    { radius: 2.4, count: 64, baseHeight: 0.18, amp: 0.55, speed: 1.4, freq: 3, color: CYAN, y: 0 },
    { radius: 3.4, count: 80, baseHeight: 0.14, amp: 0.4, speed: 1.1, freq: 5, color: BRAND, y: 0 },
    { radius: 4.4, count: 96, baseHeight: 0.1, amp: 0.3, speed: 0.9, freq: 7, color: PINK, y: 0 },
  ];

  const barMeshes = [];
  RINGS.forEach((ring) => {
    const barGeo = new THREE.BoxGeometry(0.045, 1, 0.045);
    const mat = new THREE.MeshBasicMaterial({ color: ring.color, transparent: true, opacity: 0.55 });
    const mesh = new THREE.InstancedMesh(barGeo, mat, ring.count);
    mesh.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
    group.add(mesh);
    barMeshes.push({ mesh, ring });
  });

  const dummy = new THREE.Object3D();

  function updateBars(t) {
    barMeshes.forEach(({ mesh, ring }) => {
      for (let i = 0; i < ring.count; i++) {
        const angle = (i / ring.count) * Math.PI * 2;
        const wave =
          Math.sin(t * ring.speed + angle * ring.freq) * 0.6 +
          Math.sin(t * ring.speed * 1.7 + angle * ring.freq * 0.5) * 0.4;
        const h = ring.baseHeight + Math.abs(wave) * ring.amp;
        const x = Math.cos(angle) * ring.radius;
        const z = Math.sin(angle) * ring.radius;
        dummy.position.set(x, h / 2 - 0.4, z);
        dummy.scale.set(1, h, 1);
        dummy.rotation.y = -angle;
        dummy.updateMatrix();
        mesh.setMatrixAt(i, dummy.matrix);
      }
      mesh.instanceMatrix.needsUpdate = true;
    });
  }

  // Mouse parallax — smooth, no jitter
  const mouse = { x: 0, y: 0, tx: 0, ty: 0 };
  window.addEventListener('mousemove', (e) => {
    mouse.tx = (e.clientX / window.innerWidth - 0.5) * 2;
    mouse.ty = (e.clientY / window.innerHeight - 0.5) * 2;
  });

  const corePosAttr = coreGeo.attributes.position;
  let t = 0;

  function deformCore() {
    for (let i = 0; i < corePosAttr.count; i++) {
      const ix = i * 3;
      const bx = coreBase[ix], by = coreBase[ix + 1], bz = coreBase[ix + 2];
      const len = Math.sqrt(bx * bx + by * by + bz * bz) || 1;
      const nx = bx / len, ny = by / len, nz = bz / len;
      const wave =
        0.12 * Math.sin(t * 1.8 + bx * 1.5) +
        0.09 * Math.sin(t * 1.3 + by * 1.8) +
        0.07 * Math.cos(t * 2.2 + bz * 1.6);
      const d = len + wave;
      corePosAttr.array[ix] = nx * d;
      corePosAttr.array[ix + 1] = ny * d;
      corePosAttr.array[ix + 2] = nz * d;
    }
    corePosAttr.needsUpdate = true;
  }

  function animate() {
    t += prefersReduced ? 0.004 : 0.014;

    if (!prefersReduced) {
      deformCore();
      updateBars(t);
    }

    core.rotation.y += 0.0022;
    glow.rotation.y -= 0.0016;
    group.rotation.y = Math.sin(t * 0.12) * 0.15;

    mouse.x += (mouse.tx - mouse.x) * 0.04;
    mouse.y += (mouse.ty - mouse.y) * 0.04;
    camera.position.x = mouse.x * 0.7;
    camera.position.y = 1.4 - mouse.y * 0.5;
    camera.lookAt(0, 0, 0);

    const mix = (Math.sin(t * 0.5) + 1) / 2;
    coreMat.color.copy(BRAND).lerp(CYAN, mix * 0.5);

    renderer.render(scene, camera);
    requestAnimationFrame(animate);
  }
  animate();

  window.addEventListener('resize', () => {
    camera.aspect = window.innerWidth / window.innerHeight;
    camera.updateProjectionMatrix();
    renderer.setSize(window.innerWidth, window.innerHeight);
  });
})();
