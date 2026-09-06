// The terminal recordings: an asciinema player on every `.cast` element,
// fed the asciicast its `data-cast` names. The home page's demo carries
// `data-loop` and plays by itself, over and over, with no control bar,
// the way the gif in the README does; a tutorial clip waits on its first
// frame with the control bar showing, so a reader plays it when the prose
// has brought them there. The colors come from the theme in the cast's
// header, which is the theme agg rendered the gif fallback with.
document.querySelectorAll(".cast[data-cast]").forEach(function (el) {
  var opts = {
    preload: true,
    fit: "width",
    theme: "auto/asciinema",
  };
  if (el.hasAttribute("data-loop")) {
    opts.autoPlay = true;
    opts.loop = true;
    opts.controls = false;
  } else {
    opts.autoPlay = false;
    opts.loop = false;
    opts.controls = true;
    // Half a second in: the prompt is on screen and nothing is typed yet.
    opts.poster = "npt:0:0.5";
  }
  AsciinemaPlayer.create(el.getAttribute("data-cast"), el, opts);
});
