// Entrada de Kütral: la fogata se enciende con su sonido. Corre antes que la
// app (desde app.html), así que lee el ajuste directo de localStorage.
// La animación arranca cuando el audio empieza a sonar, para que la llamarada
// y el "fwoom" coincidan; si el audio no puede (apagado, sin salida, el
// webview lo bloquea), arranca igual sin él.
// +layout.svelte lee data-hasta para no cerrar el splash a mitad.
(function () {
  var splash = document.getElementById("kutral-splash");
  if (!splash) return;
  // Lo que dura la animación desde que arranca hasta quedar quieta.
  var ANIMACION_MS = 2300;
  var empezo = false;
  function empezar() {
    if (empezo) return;
    empezo = true;
    splash.dataset.hasta = String(Date.now() + ANIMACION_MS);
    splash.classList.add("go");
  }
  var sonar = true;
  try {
    sonar = localStorage.getItem("sonido_inicio") !== "0";
  } catch (e) {
    /* sin storage: con sonido, como viene de fábrica */
  }
  if (sonar) {
    try {
      var audio = new Audio(splash.dataset.sonido);
      audio.volume = 0.7;
      audio.addEventListener("playing", empezar);
      var p = audio.play();
      if (p && p.catch) p.catch(empezar);
    } catch (e) {
      empezar();
    }
    // El audio tarda en abrir: no se espera más que esto.
    setTimeout(empezar, 600);
  } else {
    empezar();
  }
})();
