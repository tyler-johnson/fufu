// Console blocks carry the prompt and the output; the copy button should
// yield only the commands. Material's button copies its target's text via
// clipboard.js, which prefers a data-clipboard-text attribute when one is
// present and reads it at click time — so on each click, in the capture
// phase before clipboard.js runs, compute the command-only text and pin it
// to the button. Blocks without a Pygments prompt token (.gp) are left
// alone and keep the stock whole-block copy.
document.addEventListener(
  "click",
  function (ev) {
    if (!(ev.target instanceof Element)) return;
    var button = ev.target.closest(".md-clipboard");
    if (!button) return;
    var target = button.getAttribute("data-clipboard-target");
    var code = target && document.querySelector(target);
    if (!code || !code.querySelector(".gp")) return;
    var clone = code.cloneNode(true);
    clone.querySelectorAll(".gp, .go").forEach(function (el) {
      el.remove();
    });
    var commands = clone.textContent
      .split("\n")
      .filter(function (line) {
        return line.trim() !== "";
      })
      .join("\n");
    if (commands) button.setAttribute("data-clipboard-text", commands);
  },
  true
);
