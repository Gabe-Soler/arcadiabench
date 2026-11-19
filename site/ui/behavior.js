/* ==========================================================================
   The ArcadiaBench site's behaviour (no dependencies, works on file://)
   Load with <script defer src="behavior.js"></script>.

   Data attributes
     .g-header                      gets .is-scrolled after 8px of scroll
     data-g-drawer-toggle="#id"     button that opens / closes a .g-drawer
     data-g-rail                    wrapper around a .g-rail and a .g-stepper
       data-g-rail-prev / -next     the two stepper buttons
     data-g-vtabs                   wrapper around .g-vtabs__tab[aria-controls] + .g-vtabs__panel
     data-g-tabs                    same contract for .g-segment__item / .g-tabs__tab
     data-g-spy                     nav whose a[href="#id"] items track the section in view
     data-g-theme-toggle            button that flips <html data-g-theme> light <-> dark

   In a SPA call window.SiteUI.init(rootEl) after each mount. It is
   idempotent: elements already wired are skipped.
   ========================================================================== */
(function () {
  'use strict';
  var WIRED = 'data-g-wired';
  var reduce = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  function all(root, sel) { return Array.prototype.slice.call((root || document).querySelectorAll(sel)); }
  function fresh(el) { if (el.hasAttribute(WIRED)) return false; el.setAttribute(WIRED, ''); return true; }

  /* ---- header: transparent at the top, veiled once the page moves ------ */
  var headerBound = false;
  function header() {
    if (headerBound) return; headerBound = true;
    var ticking = false;
    function update() {
      ticking = false;
      var on = window.scrollY > 8;
      all(document, '.g-header').forEach(function (h) { h.classList.toggle('is-scrolled', on); });
    }
    window.addEventListener('scroll', function () { if (!ticking) { ticking = true; requestAnimationFrame(update); } }, { passive: true });
    update();
  }

  /* ---- drawer ---------------------------------------------------------- */
  function drawers(root) {
    all(root, '[data-g-drawer-toggle]').filter(fresh).forEach(function (btn) {
      var drawer = document.querySelector(btn.getAttribute('data-g-drawer-toggle'));
      if (!drawer) return;
      function set(open) {
        drawer.classList.toggle('is-open', open);
        btn.setAttribute('aria-expanded', String(open));
        document.documentElement.classList.toggle('g-no-scroll', open);
        var icon = btn.querySelector('.g-icon'); if (icon && btn.hasAttribute('data-g-icon-swap')) icon.textContent = open ? 'close' : 'menu';
      }
      btn.setAttribute('aria-expanded', 'false');
      btn.addEventListener('click', function () { set(!drawer.classList.contains('is-open')); });
      drawer.addEventListener('click', function (e) { if (e.target.closest('a')) set(false); });
      document.addEventListener('keydown', function (e) { if (e.key === 'Escape' && drawer.classList.contains('is-open')) { set(false); btn.focus(); } });
    });
  }

  /* ---- rail: prev / next step by one card ------------------------------ */
  function rails(root) {
    all(root, '[data-g-rail]').filter(fresh).forEach(function (wrap) {
      var rail = wrap.querySelector('.g-rail');
      var prev = wrap.querySelector('[data-g-rail-prev]');
      var next = wrap.querySelector('[data-g-rail-next]');
      if (!rail) return;
      function step() {
        var first = rail.firstElementChild; if (!first) return rail.clientWidth;
        var gap = parseFloat(getComputedStyle(rail).columnGap) || 0;
        return first.getBoundingClientRect().width + gap;
      }
      function sync() {
        var max = rail.scrollWidth - rail.clientWidth - 2;
        if (prev) prev.disabled = rail.scrollLeft <= 2;
        if (next) next.disabled = rail.scrollLeft >= max;
      }
      function go(dir) { rail.scrollBy({ left: dir * step(), behavior: reduce ? 'auto' : 'smooth' }); }
      if (prev) prev.addEventListener('click', function () { go(-1); });
      if (next) next.addEventListener('click', function () { go(1); });
      rail.addEventListener('scroll', sync, { passive: true });
      window.addEventListener('resize', sync);
      sync();
    });
  }

  /* ---- tabs (vertical list, segment or underline) ----------------------- */
  function tabs(root) {
    all(root, '[data-g-vtabs], [data-g-tabs]').filter(fresh).forEach(function (wrap) {
      var items = all(wrap, '.g-vtabs__tab, .g-segment__item, .g-tabs__tab').filter(function (t) { return t.hasAttribute('aria-controls'); });
      if (!items.length) return;
      function select(tab, focus) {
        items.forEach(function (t) {
          var on = t === tab;
          t.setAttribute('aria-selected', String(on));
          t.tabIndex = on ? 0 : -1;
          var panel = document.getElementById(t.getAttribute('aria-controls'));
          if (panel) panel.hidden = !on;
        });
        if (focus) tab.focus();
      }
      items.forEach(function (tab, i) {
        tab.setAttribute('role', 'tab');
        tab.addEventListener('click', function () { select(tab); });
        tab.addEventListener('keydown', function (e) {
          var d = (e.key === 'ArrowDown' || e.key === 'ArrowRight') ? 1 : (e.key === 'ArrowUp' || e.key === 'ArrowLeft') ? -1 : 0;
          if (!d) return;
          e.preventDefault();
          select(items[(i + d + items.length) % items.length], true);
        });
      });
      select(items.filter(function (t) { return t.getAttribute('aria-selected') === 'true'; })[0] || items[0]);
    });
  }

  /* ---- scroll spy for jump links ---------------------------------------- */
  function spies(root) {
    if (!('IntersectionObserver' in window)) return;
    all(root, '[data-g-spy]').filter(fresh).forEach(function (nav) {
      var links = all(nav, 'a[href^="#"]');
      var map = {};
      links.forEach(function (a) { var el = document.getElementById(a.getAttribute('href').slice(1)); if (el) map[el.id] = a; });
      var io = new IntersectionObserver(function (entries) {
        entries.forEach(function (en) {
          if (!en.isIntersecting) return;
          links.forEach(function (a) { a.removeAttribute('aria-current'); });
          map[en.target.id].setAttribute('aria-current', 'true');
        });
      }, { rootMargin: '-30% 0px -60% 0px' });
      Object.keys(map).forEach(function (id) { io.observe(document.getElementById(id)); });
    });
  }

  /* ---- theme toggle ----------------------------------------------------- */
  function themes(root) {
    var html = document.documentElement;
    all(root, '[data-g-theme-toggle]').filter(fresh).forEach(function (btn) {
      function current() {
        var t = html.getAttribute('data-g-theme');
        if (t === 'dark' || t === 'light') return t;
        return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
      }
      function paint() {
        var dark = current() === 'dark';
        btn.setAttribute('aria-pressed', String(dark));
        var icon = btn.querySelector('.g-icon'); if (icon) icon.textContent = dark ? 'light_mode' : 'dark_mode';
      }
      btn.addEventListener('click', function () {
        var next = current() === 'dark' ? 'light' : 'dark';
        html.setAttribute('data-g-theme', next);
        try { localStorage.setItem('g-theme', next); } catch (e) { /* storage unavailable */ }
        all(document, '[data-g-theme-toggle]').forEach(function (b) { b.dispatchEvent(new CustomEvent('g-theme-change')); });
      });
      btn.addEventListener('g-theme-change', paint);
      paint();
    });
  }
  function restoreTheme() {
    try {
      var saved = localStorage.getItem('g-theme');
      if ((saved === 'dark' || saved === 'light') && document.querySelector('[data-g-theme-toggle]')) document.documentElement.setAttribute('data-g-theme', saved);
    } catch (e) { /* storage unavailable */ }
  }

  function init(root) {
    header(); drawers(root); rails(root); tabs(root); spies(root); themes(root);
  }
  window.SiteUI = { init: init };
  function boot() { restoreTheme(); init(document); }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', boot); else boot();
})();
