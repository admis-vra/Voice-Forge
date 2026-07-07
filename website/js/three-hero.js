/* VoiceForge — animated 3D background.
 * A pulsing wireframe "voice orb" surrounded by a drifting particle field, with subtle
 * mouse parallax. Designed to be lightweight and to degrade gracefully. */
(function () {
  const canvas = document.getElementById('bg-canvas');
  if (!canvas || typeof THREE === 'undefined') return;

  const prefersReduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(60, window.innerWidth / window.innerHeight, 0.1, 100);
  camera.position.z = 6;

  const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  renderer.setSize(window.innerWidth, window.innerHeight);

  const BRAND = new THREE.Color(0x6d5efc);
  const CYAN = new THREE.Color(0x29d3ff);

  // --- Voice orb: an icosahedron whose vertices breathe like a waveform ---
  const geo = new THREE.IcosahedronGeometry(2, 5);
  const basePositions = geo.attributes.position.array.slice();
  const orbMat = new THREE.MeshBasicMaterial({
    color: BRAND,
    wireframe: true,
    transparent: true,
    opacity: 0.55,
  });
  const orb = new THREE.Mesh(geo, orbMat);
  scene.add(orb);

  // Inner glow core
  const coreMat = new THREE.MeshBasicMaterial({ color: CYAN, transparent: true, opacity: 0.08 });
  const core = new THREE.Mesh(new THREE.IcosahedronGeometry(1.4, 2), coreMat);
  scene.add(core);

  // --- Particle field ---
  const COUNT = 900;
  const pGeo = new THREE.BufferGeometry();
  const pPos = new Float32Array(COUNT * 3);
  for (let i = 0; i < COUNT; i++) {
    const r = 6 + Math.random() * 14;
    const theta = Math.random() * Math.PI * 2;
    const phi = Math.acos(2 * Math.random() - 1);
    pPos[i * 3] = r * Math.sin(phi) * Math.cos(theta);
    pPos[i * 3 + 1] = r * Math.sin(phi) * Math.sin(theta);
    pPos[i * 3 + 2] = r * Math.cos(phi);
  }
  pGeo.setAttribute('position', new THREE.BufferAttribute(pPos, 3));
  const pMat = new THREE.PointsMaterial({ color: CYAN, size: 0.035, transparent: true, opacity: 0.7 });
  const particles = new THREE.Points(pGeo, pMat);
  scene.add(particles);

  // Mouse parallax
  const mouse = { x: 0, y: 0, tx: 0, ty: 0 };
  window.addEventListener('mousemove', (e) => {
    mouse.tx = (e.clientX / window.innerWidth - 0.5) * 2;
    mouse.ty = (e.clientY / window.innerHeight - 0.5) * 2;
  });

  const posAttr = geo.attributes.position;
  let t = 0;

  function deform() {
    // Displace each vertex along its normal by layered sine waves → "speaking" pulse.
    for (let i = 0; i < posAttr.count; i++) {
      const ix = i * 3;
      const bx = basePositions[ix], by = basePositions[ix + 1], bz = basePositions[ix + 2];
      const len = Math.sqrt(bx * bx + by * by + bz * bz) || 1;
      const nx = bx / len, ny = by / len, nz = bz / len;
      const wave =
        0.16 * Math.sin(t * 1.6 + bx * 1.5) +
        0.13 * Math.sin(t * 1.2 + by * 1.8) +
        0.10 * Math.cos(t * 2.0 + bz * 1.6);
      const d = len + wave;
      posAttr.array[ix] = nx * d;
      posAttr.array[ix + 1] = ny * d;
      posAttr.array[ix + 2] = nz * d;
    }
    posAttr.needsUpdate = true;
  }

  function animate() {
    t += prefersReduced ? 0.004 : 0.012;

    if (!prefersReduced) deform();

    orb.rotation.y += 0.0024;
    orb.rotation.x += 0.0011;
    core.rotation.y -= 0.0018;
    particles.rotation.y += 0.0006;

    // Ease mouse parallax
    mouse.x += (mouse.tx - mouse.x) * 0.04;
    mouse.y += (mouse.ty - mouse.y) * 0.04;
    camera.position.x = mouse.x * 0.6;
    camera.position.y = -mouse.y * 0.6;
    camera.lookAt(0, 0, 0);

    // Hue shimmer between brand and cyan
    const mix = (Math.sin(t * 0.5) + 1) / 2;
    orbMat.color.copy(BRAND).lerp(CYAN, mix * 0.5);

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
