/* VoiceForge landing — tabs, scroll reveal, dynamic OS detection. */
(function () {
  // --- Download tabs ---
  const tabs = document.querySelectorAll('.tab');
  const panels = {
    win: document.getElementById('tab-win'),
    mac: document.getElementById('tab-mac'),
    linux: document.getElementById('tab-linux'),
  };
  function activate(key) {
    tabs.forEach((t) => t.classList.toggle('active', t.dataset.tab === key));
    Object.entries(panels).forEach(([k, el]) => el && el.classList.toggle('active', k === key));
  }
  tabs.forEach((t) => t.addEventListener('click', () => activate(t.dataset.tab)));

  // Preselect the tab matching the visitor's OS.
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes('mac')) activate('mac');
  else if (ua.includes('linux') && !ua.includes('android')) activate('linux');
  else activate('win');

  // --- Scroll reveal for cards/steps ---
  const io = new IntersectionObserver(
    (entries) => {
      entries.forEach((e) => {
        if (e.isIntersecting) {
          e.target.style.opacity = 1;
          e.target.style.transform = 'none';
          io.unobserve(e.target);
        }
      });
    },
    { threshold: 0.12 }
  );
  document.querySelectorAll('.card, .step, .ak-step, .dl-card').forEach((el, i) => {
    el.style.opacity = 0;
    el.style.transform = 'translateY(18px)';
    el.style.transition = `opacity .5s ease ${(i % 6) * 0.05}s, transform .5s ease ${(i % 6) * 0.05}s`;
    io.observe(el);
  });
})();
