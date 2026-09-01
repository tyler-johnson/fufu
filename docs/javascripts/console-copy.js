// Console blocks carry the prompt and the output; the copy button should
// yield only the commands. Material's copy handler resolves its text
// through the block's `data-copy` attribute when one is present — the
// value is copied verbatim in place of the rendered text — so at load
// time, compute the command-only text for every console block and pin it
// to the code element. No click interception: Material's own button,
// tooltip, and announcement all run unchanged. Blocks without a Pygments
// prompt token (.gp) get no attribute and keep the stock whole-block copy.
document.querySelectorAll("pre > code").forEach(function (code) {
  if (!code.querySelector(".gp")) return;
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
  if (commands) code.setAttribute("data-copy", commands);
});
