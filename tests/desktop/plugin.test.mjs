import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)
const pluginPath = resolve(__dirname, '../../desktop/plugin.js')

test('no JSX syntax in plugin source', async () => {
  const source = await readFile(pluginPath, 'utf8')
  assert.doesNotMatch(source, /<[A-Z][a-zA-Z]*\s/, 'JSX element syntax found')
  assert.doesNotMatch(source, /<\/[A-Z][a-zA-Z]*>/, 'JSX closing tag syntax found')
})

test('only allowed bare import specifiers', async () => {
  const source = await readFile(pluginPath, 'utf8')
  const importRegex = /^import\s+.*\s+from\s+['"]([^'"]+)['"]/gm
  const allowedImports = new Set([
    '@hermes/plugin-sdk',
    'react',
    'react/jsx-runtime'
  ])
  let match
  while ((match = importRegex.exec(source)) !== null) {
    const specifier = match[1]
    assert.ok(allowedImports.has(specifier), `Disallowed import specifier: ${specifier}`)
  }
})

test('route and sidebar use same /gaze-privacy path', async () => {
  const source = await readFile(pluginPath, 'utf8')
  const pathMatches = source.match(/const PATH = ['"]([^'"]+)['"]/g)
  assert.ok(pathMatches && pathMatches.length >= 1, 'PATH constant not found')
  const pathValue = pathMatches[0].match(/['"]([^'"]+)['"]/)[1]
  assert.strictEqual(pathValue, '/gaze-privacy', 'PATH must be /gaze-privacy')
  // Check route area uses PATH
  assert.match(source, /data: \{ path: PATH \}/, 'Route area must use PATH')
  // Check sidebar area uses PATH
  assert.match(source, /data: \{ path: PATH/, 'Sidebar area must use PATH')
})

test('no direct sidecar endpoint or secrets references', async () => {
  const source = await readFile(pluginPath, 'utf8')
  assert.doesNotMatch(source, /65113/, 'Direct port reference found')
  assert.doesNotMatch(source, /Authorization/, 'Authorization header reference found')
  assert.doesNotMatch(source, /api-token/, 'API token file reference found')
  assert.doesNotMatch(source, /master-key|master_key/, 'Master key reference found')
})

test('imports SDK tab primitives Tabs, TabsList, TabsTrigger', async () => {
  const source = await readFile(pluginPath, 'utf8')
  // Check the import statement includes Tabs, TabsList, TabsTrigger
  assert.match(source, /import\s+\{[^}]*Tabs[^}]*TabsList[^}]*TabsTrigger[^}]*\}\s+from\s+['"]@hermes\/plugin-sdk['"]/, 'Must import Tabs, TabsList, TabsTrigger from @hermes/plugin-sdk')
})

test('does not import TabsContent (not exported by SDK)', async () => {
  const source = await readFile(pluginPath, 'utf8')
  assert.doesNotMatch(source, /TabsContent/, 'TabsContent should not be imported (not exported by SDK)')
})

test('no private Radix primitives or app-internal modules', async () => {
  const source = await readFile(pluginPath, 'utf8')
  assert.doesNotMatch(source, /@radix-ui/, 'Private Radix primitives should not be imported')
  assert.doesNotMatch(source, /['"]\.\//, 'Relative imports to app-internal modules not allowed')
})