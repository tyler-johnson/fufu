// Exercise the installed client's local discovery without its authenticated CLI entry point. The pinned build exposes its loader through webpack; a changed bundle or export fails this probe and requires reviewing the client update.
const fs = require('node:fs');
const path = require('node:path');
const Module = require('node:module');

const [file, home, workspace] = process.argv.slice(2);
const original = fs.readFileSync(file, 'utf8');
const entry = 'var __webpack_exports__=__webpack_require__("./src/main.tsx")';
if (!original.endsWith(`${entry}})();`)) {
    throw new Error('Cursor bundle entry changed; review the installed client');
}
const bundle = new Module(file);
bundle.filename = file;
bundle.paths = Module._nodeModulePaths(path.dirname(file));
bundle._compile(original.replace(entry, 'module.exports=__webpack_require__'), file);

async function main() {
    const plugins = bundle.exports('../cursor-plugins/dist/index.js');
    const result = await plugins.wpN([workspace], {
        userHomeDir: home,
        loadClaude: false,
        loadUserLocal: true,
        loadCursorFirstParty: false,
    });
    process.stdout.write(JSON.stringify(result));
}
main().catch(error => {
    console.error(error);
    process.exitCode = 1;
});
