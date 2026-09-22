import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)
const pluginPath = resolve(__dirname, '../../desktop/plugin.js')

test('reveal state is ephemeral and auto-cleared', async () => {
  const source = await readFile(pluginPath, 'utf8')
  assert.match(source, /REVEAL_TTL_MS\s*=\s*60_000/, 'REVEAL_TTL_MS constant must be 60_000')
  assert.match(source, /clearTimeout/, 'Must use clearTimeout for auto-clearing')
  assert.match(source, /focusedSessionProfile/, 'Must use focusedSessionProfile for profile-aware clearing')
  assert.doesNotMatch(source, /ctx\.storage\.set\([^)]*reveal/i, 'Reveal state must not be persisted to ctx.storage')
})