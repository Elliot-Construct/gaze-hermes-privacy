import {
  ROUTES_AREA,
  SIDEBAR_NAV_AREA,
  Button,
  Codicon,
  StatusDot,
  Tabs,
  TabsList,
  TabsTrigger,
  useQuery,
  useQueryClient,
  useValue,
  host
} from '@hermes/plugin-sdk'
import { useEffect, useState } from 'react'
import { jsx, jsxs } from 'react/jsx-runtime'

const ID = 'gaze-hermes-privacy'
const PATH = '/gaze-privacy'

function usePrivacyStatus(ctx) {
  return useQuery({
    queryKey: [ID, 'status'],
    queryFn: () => ctx.rest('/status'),
    refetchInterval: 3000
  })
}

function usePrivacyEvents(ctx) {
  const queryClient = useQueryClient()
  const query = useQuery({
    queryKey: [ID, 'events'],
    queryFn: () => ctx.rest('/events?limit=200'),
    refetchInterval: 5000
  })

  useEffect(() => {
    return ctx.socket('/events', () => {
      void queryClient.invalidateQueries({ queryKey: [ID, 'events'] })
    })
  }, [ctx, queryClient])

  return query
}

function Overview({ ctx }) {
  const query = usePrivacyStatus(ctx)
  if (query.isPending) return jsx('div', { children: 'Loading privacy status' })
  if (query.isError) return jsx('div', { children: 'Privacy status unavailable' })
  const s = query.data
  return jsxs('div', {
    children: [
      jsx('h2', { children: s.protection_state }),
      jsx('div', { children: 'Sidecar: ' + s.sidecar.mode + ' ' + s.sidecar.version }),
      jsx('div', { children: 'Policy: ' + s.policy_hash }),
      jsx('div', { children: 'Protected ' + s.counters.protected + ' · Bypassed ' + s.counters.bypassed + ' · Blocked ' + s.counters.blocked }),
      jsx('div', { children: 'Fail-closed ' + (s.capabilities.fail_closed ? 'yes' : 'no') + ' · Stream transform ' + (s.capabilities.stream_text ? 'yes' : 'no') })
    ]
  })
}

function LiveDebug({ ctx }) {
  const query = usePrivacyEvents(ctx)
  if (query.isPending) return jsx('div', { children: 'Loading events...' })
  if (query.isError) return jsx('div', { children: 'Events unavailable' })
  const events = query.data?.events || []
  return jsxs('div', {
    children: [
      jsx('h3', { children: 'Live Events' }),
      jsx('div', {
        style: { maxHeight: '400px', overflow: 'auto' },
        children: events.map(e => jsxs('div', {
          style: { borderBottom: '1px solid var(--vscode-panel-border)', padding: '8px' },
          children: [
            jsx('div', { children: e.request_id + ' · ' + e.provider + ' · ' + e.api_mode }),
            jsx('div', { children: 'Classes: ' + (e.detections?.map(d => d.class + '(' + d.count + ')').join(', ') || 'none') }),
            jsx('div', { children: 'Decision: ' + e.state + (e.latency_ms ? ' · ' + e.latency_ms + 'ms' : '') }),
            e.error_code && jsx('div', { style: { color: 'var(--vscode-errorForeground)' }, children: 'Error: ' + e.error_code })
          ]
        }, e.id))
      })
    ]
  })
}

function Rules({ ctx }) {
  const [mode, setMode] = useState('Visual')
  const [globalPolicy, setGlobalPolicy] = useState('')
  const [profilePolicy, setProfilePolicy] = useState('')
  const [activeProfile, setActiveProfile] = useState('default')
  const [diff, setDiff] = useState('')

  const profile = host.state?.focusedSessionProfile || 'default'

  async function loadPolicy() {
    const global = await ctx.rest('/policies/global')
    const prof = await ctx.rest('/policies/profiles/' + profile)
    setGlobalPolicy(global.toml)
    setProfilePolicy(prof.toml)
  }

  useEffect(() => { loadPolicy() }, [profile])

  async function applyVisualEdit(edit) {
    const edited = await ctx.rest('/policies/edit', { method: 'POST', body: { scope: 'profile:' + profile, edit } })
    const validated = await ctx.rest('/policies/validate', { method: 'POST', body: { scope: 'profile:' + profile, toml: edited.toml } })
    if (!validated.valid) throw new Error('Policy validation failed')
    return ctx.rest('/policies/apply', { method: 'POST', body: { scope: 'profile:' + profile, toml: edited.toml, expected_hash: edited.base_hash } })
  }

  async function runTest(toml, sample) {
    return ctx.rest('/policies/test', { method: 'POST', body: { scope: 'profile:' + profile, toml, sample } })
  }

  return jsxs('div', {
    children: [
      jsx('div', { style: { display: 'flex', gap: '8px', marginBottom: '16px' }, children: [
        jsx(Button, { variant: mode === 'Visual' ? 'primary' : 'secondary', onClick: () => setMode('Visual'), children: 'Visual' }),
        jsx(Button, { variant: mode === 'Advanced TOML' ? 'primary' : 'secondary', onClick: () => setMode('Advanced TOML'), children: 'Advanced TOML' })
      ]}),
      mode === 'Visual' ? jsxs('div', { children: [
        jsx('h4', { children: 'Recognizers' }),
        jsx('div', { children: 'Visual recognizer editor (name, kind, pattern, class, dictionary, caseSensitive, tokenFamily)' }),
        jsx('h4', { children: 'Rules' }),
        jsx('div', { children: 'Visual rule editor (identity, action, locales, NER settings)' }),
        jsx('div', { style: { marginTop: '16px' }, children: [
          jsx(Button, { variant: 'primary', onClick: async () => { await applyVisualEdit({}); await loadPolicy() }, children: 'Apply' }),
          jsx(Button, { variant: 'secondary', onClick: loadPolicy, children: 'Revert' })
        ]})
      ]}) : jsxs('div', { children: [
        jsx('label', { children: 'Advanced TOML (byte-preserved)' }),
        jsx('textarea', { value: profilePolicy, onChange: e => setProfilePolicy(e.target.value), style: { width: '100%', height: '300px', fontFamily: 'monospace' } }),
        jsx('div', { style: { marginTop: '16px' }, children: [
          jsx(Button, { variant: 'primary', onClick: async () => {
            const validated = await ctx.rest('/policies/validate', { method: 'POST', body: { scope: 'profile:' + profile, toml: profilePolicy } })
            if (!validated.valid) throw new Error('Policy validation failed')
            await ctx.rest('/policies/apply', { method: 'POST', body: { scope: 'profile:' + profile, toml: profilePolicy, expected_hash: 'hash' } })
            await loadPolicy()
          }, children: 'Apply' }),
          jsx(Button, { variant: 'secondary', onClick: loadPolicy, children: 'Revert' })
        ]})
      ]})
    ]
  })
}

function Providers({ ctx }) {
  const [providers, setProviders] = useState([])

  async function loadProviders() {
    const res = await ctx.rest('/providers')
    setProviders(res.providers || [])
  }

  async function toggleTrust(providerId, nextTrusted) {
    await ctx.rest('/providers/' + encodeURIComponent(providerId) + '/trust', { method: 'PUT', body: { trusted_local: nextTrusted } })
    await loadProviders()
  }

  return jsxs('div', {
    children: [
      jsx('h3', { children: 'Provider Trust' }),
      jsx('div', { children: providers.map(p => jsxs('div', {
        style: { display: 'flex', alignItems: 'center', gap: '8px', padding: '8px', borderBottom: '1px solid var(--vscode-panel-border)' },
        children: [
          jsx('span', { style: { width: '200px' }, children: p.id }),
          jsx('span', { style: { width: '150px' }, children: p.trusted_local ? 'TRUSTED LOCAL' : (p.supported ? 'PROTECTED' : 'BLOCKED / UNSUPPORTED') }),
          jsx(Button, { size: 'sm', onClick: () => toggleTrust(p.id, !p.trusted_local), children: p.trusted_local ? 'Revoke Trust' : 'Trust' })
        ]
      }, p.id))})
    ]
  })
}

function Sessions({ ctx }) {
  const [sessions, setSessions] = useState([])
  const profile = host.state?.focusedSessionProfile || 'default'

  async function loadSessions() {
    const res = await ctx.rest('/sessions')
    setSessions(res.sessions || [])
  }

  async function recoverSession(profileId, sessionId) {
    await ctx.rest('/sessions/' + encodeURIComponent(profileId) + '/' + encodeURIComponent(sessionId) + '/recover', { method: 'POST' })
    await loadSessions()
  }

  async function deleteSession(profileId, sessionId) {
    if (!confirm('Delete session ' + sessionId + '? Previous reversible mappings will be abandoned.')) return
    await ctx.rest('/sessions/' + encodeURIComponent(profileId) + '/' + encodeURIComponent(sessionId), { method: 'DELETE' })
    await loadSessions()
  }

  return jsxs('div', {
    children: [
      jsx('h3', { children: 'Sessions' }),
      jsx('div', { children: sessions.map(s => jsxs('div', {
        style: { display: 'flex', alignItems: 'center', gap: '8px', padding: '8px', borderBottom: '1px solid var(--vscode-panel-border)' },
        children: [
          jsx('span', { style: { width: '150px' }, children: s.profile_id }),
          jsx('span', { style: { width: '200px' }, children: s.session_id }),
          jsx('span', { style: { width: '150px' }, children: s.snapshot_state }),
          jsx('span', { style: { width: '100px' }, children: s.mapping_count + ' mappings' }),
          jsx(Button, { size: 'sm', variant: 'secondary', onClick: () => recoverSession(s.profile_id, s.session_id), children: 'Recover' }),
          jsx(Button, { size: 'sm', variant: 'secondary', onClick: () => deleteSession(s.profile_id, s.session_id), children: 'Delete' })
        ]
      }, s.session_id))})
    ]
  })
}

const REVEAL_TTL_MS = 60_000

function useSensitiveReveal(profileId) {
  const [revealed, setRevealed] = useState(null)

  useEffect(() => {
    setRevealed(null)
  }, [profileId])

  useEffect(() => {
    if (!revealed) return undefined
    const timer = setTimeout(() => setRevealed(null), REVEAL_TTL_MS)
    return () => clearTimeout(timer)
  }, [revealed])

  return { revealed, setRevealed }
}

function EventsWithReveal({ ctx }) {
  const query = usePrivacyEvents(ctx)
  const profile = host.state?.focusedSessionProfile || 'default'
  const { revealed, setRevealed } = useSensitiveReveal(profile)

  if (query.isPending) return jsx('div', { children: 'Loading events...' })
  if (query.isError) return jsx('div', { children: 'Events unavailable' })
  const events = query.data?.events || []

  async function handleReveal(eventId) {
    const res = await ctx.rest('/events/' + eventId + '/reveal', { method: 'POST', body: { ttl_seconds: 60 } })
    setRevealed({ token: res.token, eventId })
  }

  return jsxs('div', {
    children: [
      jsx('h3', { children: 'Events' }),
      jsx('div', {
        style: { maxHeight: '400px', overflow: 'auto' },
        children: events.map(e => jsxs('div', {
          style: { borderBottom: '1px solid var(--vscode-panel-border)', padding: '8px' },
          children: [
            jsx('div', { style: { display: 'flex', justifyContent: 'space-between' }, children: [
              jsx('span', { children: e.request_id + ' · ' + e.provider }),
              e.detections?.some(d => d.class) && jsx(Button, { size: 'sm', onClick: () => handleReveal(e.id), children: revealed?.eventId === e.id ? 'Hide' : 'Reveal' })
            ]}),
            jsx('div', { children: 'Classes: ' + (e.detections?.map(d => d.class + '(' + d.count + ')').join(', ') || 'none') }),
            revealed?.eventId === e.id && jsxs('div', { style: { marginTop: '8px', padding: '8px', background: 'var(--vscode-textBlockQuote-background)', borderRadius: '4px' }, children: [
              jsx('strong', { children: 'Revealed:' }),
              jsx('pre', { children: JSON.stringify(revealed, null, 2) })
            ]})
          ]
        }, e.id))
      })
    ]
  })
}

function PrivacyWorkspace({ ctx }) {
  const [activeTab, setActiveTab] = useState('Overview')

  return jsxs('div', {
    style: { padding: '16px' },
    children: [
      jsxs(Tabs, {
        children: [
          jsxs(TabsList, { children: [
            jsx(TabsTrigger, { onClick: () => setActiveTab('Overview'), children: 'Overview' }),
            jsx(TabsTrigger, { onClick: () => setActiveTab('Live Debug'), children: 'Live Debug' }),
            jsx(TabsTrigger, { onClick: () => setActiveTab('Rules'), children: 'Rules' }),
            jsx(TabsTrigger, { onClick: () => setActiveTab('Test Lab'), children: 'Test Lab' }),
            jsx(TabsTrigger, { onClick: () => setActiveTab('Providers'), children: 'Providers' }),
            jsx(TabsTrigger, { onClick: () => setActiveTab('Sessions'), children: 'Sessions' }),
            jsx(TabsTrigger, { onClick: () => setActiveTab('Events'), children: 'Events' })
          ]}),
          activeTab === 'Overview' && jsx(Overview, { ctx }),
          activeTab === 'Live Debug' && jsx(LiveDebug, { ctx }),
          activeTab === 'Rules' && jsx(Rules, { ctx }),
          activeTab === 'Test Lab' && jsx('div', { children: 'Test Lab - integrate with Rules tab' }),
          activeTab === 'Providers' && jsx(Providers, { ctx }),
          activeTab === 'Sessions' && jsx(Sessions, { ctx }),
          activeTab === 'Events' && jsx(EventsWithReveal, { ctx })
        ]
      })
    ]
  })
}

function PrivacyStatus({ ctx }) {
  const query = usePrivacyStatus(ctx)
  const label = query.isError
    ? 'Error'
    : query.isPending
      ? 'Checking privacy'
      : query.data.protection_state
  return jsx(Button, {
    variant: 'ghost',
    onClick: () => host.navigate(PATH),
    children: label
  })
}

export default {
  id: 'gaze-hermes-privacy',
  name: 'Gaze Privacy',
  register(ctx) {
    ctx.registerMany([
      {
        id: 'workspace',
        area: ROUTES_AREA,
        data: { path: PATH },
        render: () => jsx(PrivacyWorkspace, { ctx })
      },
      {
        id: 'nav',
        area: SIDEBAR_NAV_AREA,
        data: { path: PATH, label: 'Gaze Privacy', codicon: 'shield' }
      },
      {
        id: 'status',
        area: 'statusBar.right',
        render: () => jsx(PrivacyStatus, { ctx })
      }
    ])
  }
}