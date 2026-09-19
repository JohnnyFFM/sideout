// SIDEOUT theme: dark (default) ↔ light, persisted in localStorage.
// Pre-paint: set data-theme + inline background before the stylesheet loads
// (no flash); keep <meta name="theme-color"> in sync. ?theme= URL override.
(function () {
  var BG = { dark: '#0F131B', light: '#EEF0F4' };
  var ORDER = ['dark', 'light'];
  var NEXT_ICON = { dark: '☀', light: '☾' };

  function apply(next) {
    if (next === 'dark') delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = next;
    document.documentElement.style.background = BG[next] || BG.dark;
    var tc = document.querySelector('meta[name="theme-color"]');
    if (tc) tc.setAttribute('content', BG[next] || BG.dark);
    var btns = document.querySelectorAll('[data-theme-btn]');
    for (var i = 0; i < btns.length; i++) btns[i].textContent = NEXT_ICON[next];
  }

  var url = new URLSearchParams(location.search).get('theme');
  var stored = null;
  try { stored = localStorage.getItem('so_theme'); } catch (e) {}
  var initial = url || stored ||
    (matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark');
  if (ORDER.indexOf(initial) < 0) initial = 'dark';
  apply(initial);

  window.soTheme = {
    get: function () { return document.documentElement.dataset.theme || 'dark'; },
    set: function (next) {
      apply(next);
      try { localStorage.setItem('so_theme', next); } catch (e) {}
    },
    cycle: function () {
      var cur = window.soTheme.get();
      window.soTheme.set(ORDER[(ORDER.indexOf(cur) + 1) % ORDER.length]);
    }
  };

  document.addEventListener('DOMContentLoaded', function () {
    apply(window.soTheme.get());
    var btns = document.querySelectorAll('[data-theme-btn]');
    for (var i = 0; i < btns.length; i++) btns[i].addEventListener('click', window.soTheme.cycle);
  });
  window.addEventListener('storage', function (e) {
    if (e.key === 'so_theme' && e.newValue && e.newValue !== window.soTheme.get()) apply(e.newValue);
  });
})();
