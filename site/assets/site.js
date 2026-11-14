// Nilavar website: decorative rain, screenshot tour, OS tabs, OS detection.
// No network requests, no cookies, no storage.
(function () {
  "use strict";
  var reduce = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  // ---- Digital rain (decorative glyphs only) ----
  var canvas = document.getElementById("rain");
  var ctx = canvas && canvas.getContext("2d");
  var glyphs = "λΣΩπΔ≡±жяψφ0123456789<>{}[]/\\*+=:;¦§µßΞΓΘЛФЦ".split("");
  var drops = [], size = 15, w = 0, h = 0, last = 0;
  function reset() {
    var dpr = Math.min(window.devicePixelRatio || 1, 2);
    w = window.innerWidth; h = window.innerHeight;
    canvas.width = w * dpr; canvas.height = h * dpr;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    drops = [];
    var cols = Math.floor(w / (size * 1.15));
    for (var c = 0; c < cols; c++) {
      if (Math.random() < 0.6) continue;
      var len = 6 + Math.floor(Math.random() * 18), g = [];
      for (var k = 0; k < len; k++) g.push(glyphs[(Math.random() * glyphs.length) | 0]);
      drops.push({ x: c * size * 1.15 + size / 2, y: Math.random() * h * 1.3, v: 35 + Math.random() * 100, g: g });
    }
  }
  function frame(t) {
    var dt = last ? Math.min((t - last) / 1000, 0.1) : 0; last = t;
    ctx.clearRect(0, 0, w, h);
    ctx.font = size + "px 'JetBrains Mono', monospace";
    ctx.textAlign = "center";
    for (var i = 0; i < drops.length; i++) {
      var d = drops[i];
      if (!reduce) {
        d.y += d.v * dt;
        if (Math.random() < 0.04) d.g[(Math.random() * d.g.length) | 0] = glyphs[(Math.random() * glyphs.length) | 0];
        if (d.y - d.g.length * size > h) { d.y = -Math.random() * h * 0.3; d.v = 35 + Math.random() * 100; }
      }
      for (var k = 0; k < d.g.length; k++) {
        var y = d.y - k * size;
        if (y < -size || y > h + size) continue;
        var fade = 1 - k / d.g.length;
        ctx.fillStyle = k === 0 ? "rgba(184,255,214,.45)" : "rgba(57,255,136," + (0.16 * fade) + ")";
        ctx.fillText(d.g[k], d.x, y);
      }
    }
    if (!reduce && !document.hidden) setTimeout(function () { requestAnimationFrame(frame); }, 50);
  }
  if (ctx) {
    reset(); requestAnimationFrame(frame);
    window.addEventListener("resize", function () { reset(); if (reduce) requestAnimationFrame(frame); });
    document.addEventListener("visibilitychange", function () { if (!document.hidden && !reduce) { last = 0; requestAnimationFrame(frame); } });
  }

  // ---- Screenshot tour ----
  var shot = document.getElementById("shot"), cap = document.getElementById("cap");
  var tabs = document.querySelectorAll(".tab");
  tabs.forEach(function (b) {
    var pre = new Image(); pre.src = "assets/img/" + b.dataset.shot + ".jpg";
    b.addEventListener("click", function () {
      tabs.forEach(function (x) { x.classList.remove("active"); x.setAttribute("aria-selected", "false"); });
      b.classList.add("active"); b.setAttribute("aria-selected", "true");
      shot.style.opacity = 0;
      setTimeout(function () {
        shot.src = "assets/img/" + b.dataset.shot + ".jpg";
        shot.alt = "Nilavar: " + b.textContent + " (example data)";
        cap.textContent = b.dataset.cap;
        shot.style.opacity = 1;
      }, reduce ? 0 : 150);
    });
  });

  // ---- Install tabs ----
  var ostabs = document.querySelectorAll(".ostab"), panels = document.querySelectorAll(".ospanel");
  function showOs(os) {
    ostabs.forEach(function (x) { x.classList.toggle("active", x.dataset.os === os); });
    panels.forEach(function (p) { p.hidden = p.dataset.os !== os; });
  }
  ostabs.forEach(function (b) { b.addEventListener("click", function () { showOs(b.dataset.os); }); });

  // ---- Highlight the visitor's platform (local check, nothing sent anywhere) ----
  var ua = navigator.userAgent || "", os = null;
  if (/Mac/i.test(ua) && !/iPhone|iPad/i.test(ua)) os = "mac";
  else if (/Windows/i.test(ua)) os = "win";
  else if (/Arch/i.test(ua)) os = "arch";
  else if (/Linux/i.test(ua) && !/Android/i.test(ua)) os = "deb";
  if (os) {
    var btn = document.querySelector('.dl .btn[data-os="' + os + '"]');
    if (btn) btn.classList.add("recommended");
    showOs(os);
  }
})();
