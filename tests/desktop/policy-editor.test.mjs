import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)
const pluginPath = resolve(__dirname, '../../desktop/plugin.js')

test('policy editor uses edit validate apply flow', async () => {
  const source = await readFile(pluginPath, 'utf8')
  assert.match(source, /\/policies\/validate/, 'Must call /policies/validate endpoint')
  assert.match(source, /\/policies\/edit/, 'Must call /policies/edit endpoint')
  assert.match(source, /\/policies\/apply/, 'Must call /policies/apply endpoint')
  assert.match(source, /Advanced TOML/, 'Must have Advanced TOML mode')
  assert.match(source, /focusedSessionProfile/, 'Must use focusedSessionProfile for profile-aware policy editing')
})