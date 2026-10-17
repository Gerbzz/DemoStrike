const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const typescript = require(process.env.CANDIDATE_TYPESCRIPT_MODULE || 'typescript');
const moduleRoot = process.env.CANDIDATE_MODULE_ROOT || path.join(__dirname, '../node_modules');
const jsxRuntime = require(path.join(moduleRoot, 'react/jsx-runtime'));
const { renderToStaticMarkup } = require(path.join(moduleRoot, 'react-dom/server'));

const source = fs.readFileSync(path.join(__dirname, '../src/upstream/get-team-score-class-name.ts'), 'utf8');
const compiled = typescript.transpileModule(source, {
  compilerOptions: { module: typescript.ModuleKind.CommonJS },
  reportDiagnostics: true,
});
assert.equal(compiled.diagnostics.length, 0);
const moduleExports = {};
new Function('exports', compiled.outputText)(moduleExports);
const { getTeamScoreClassName } = moduleExports;

assert.equal(getTeamScoreClassName(0, 0), 'text-blue-400');
assert.equal(getTeamScoreClassName(10, 5), 'text-green-400');
assert.equal(getTeamScoreClassName(5, 10), 'text-red-400');

const componentSource = fs.readFileSync(path.join(__dirname, '../src/upstream/TeamScore.tsx'), 'utf8');
const componentCompiled = typescript.transpileModule(componentSource, {
  fileName: 'TeamScore.tsx',
  compilerOptions: { module: typescript.ModuleKind.CommonJS, jsx: typescript.JsxEmit.ReactJSX },
  reportDiagnostics: true,
});
assert.equal(componentCompiled.diagnostics.length, 0);
const componentExports = {};
const localRequire = (specifier) => {
  if (specifier === './get-team-score-class-name') return moduleExports;
  if (specifier === 'react/jsx-runtime') return jsxRuntime;
  throw new Error(`Unexpected dependency: ${specifier}`);
};
new Function('require', 'exports', componentCompiled.outputText)(localRequire, componentExports);
const { TeamScore } = componentExports;
const html = renderToStaticMarkup(jsxRuntime.jsx(TeamScore, {
  teamName: 'CT', teamScore: 10, scoreOppositeTeam: 5, relation: 'leading',
}));
assert.match(html, /class="team-score-number text-green-400"/);
assert.match(html, /aria-label="CT: 10, leading"/);
assert.match(html, />10<\/p>/);
assert.match(html, />CT<\/p>/);
console.log('PASS: pinned upstream score helper and adapted TeamScore render contract');
