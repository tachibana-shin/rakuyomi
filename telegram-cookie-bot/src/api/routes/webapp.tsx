import { Hono } from "hono"
import type { FC } from "hono/jsx"

const app = new Hono()

const Page: FC = () => (
  <html lang="en">
    <head>
      <meta charset="UTF-8" />
      <meta name="viewport" content="width=device-width, initial-scale=1.0" />
      <script src="https://telegram.org/js/telegram-web-app.js" />
      <script src="https://unpkg.com/vue@3/dist/vue.global.prod.js" />
      <title>Rakuyomi Cookies</title>
      <style
        dangerouslySetInnerHTML={{
          __html: `
        * { margin: 0; padding: 0; box-sizing: border-box; }
        :root { color-scheme: dark; }
        body {
          font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
          background: var(--tg-theme-bg-color, #1a1a2e);
          color: var(--tg-theme-text-color, #e0e0e0);
          -webkit-font-smoothing: antialiased;
        }
        .container { max-width: 640px; margin: 0 auto; padding: 20px 16px; }
        .header { margin-bottom: 20px; }
        .header h1 { font-size: 16px; font-weight: 700; letter-spacing: -0.3px; }
        .n-select {
          width: 100%; padding: 10px 12px; border-radius: 10px; border: 1px solid var(--tg-theme-hint-color, #444);
          background: var(--tg-theme-secondary-bg-color, #16162a);
          color: var(--tg-theme-text-color, #e0e0e0); font-size: 14px; outline: none;
          transition: border-color .2s; appearance: none;
          cursor: pointer; margin-bottom: 12px;
        }
        .n-select:focus { border-color: var(--tg-theme-button-color, #2ea6ff); }
        .n-actions { display: flex; gap: 8px; margin-bottom: 12px; flex-wrap: wrap; }
        .n-btn {
          display: inline-flex; align-items: center; gap: 6px;
          padding: 7px 16px; border-radius: 8px; border: none; cursor: pointer;
          font-size: 12px; font-weight: 600;
          transition: opacity .2s;
        }
        .n-btn:disabled { opacity: .5; cursor: not-allowed; }
        .n-btn:hover:not(:disabled) { opacity: .8; }
        .n-btn-primary {
          background: var(--tg-theme-button-color, #2ea6ff);
          color: var(--tg-theme-button-text-color, #fff);
        }
        .n-btn-danger {
          background: rgba(231,76,60,.15);
          color: var(--tg-theme-destructive-text-color, #e74c3c);
        }
        .n-btn-ghost {
          background: transparent;
          color: var(--tg-theme-hint-color, #888);
          padding: 4px 8px;
        }
        .n-btn-ghost:hover { color: var(--tg-theme-destructive-text-color, #e74c3c); }
        .n-card {
          background: var(--tg-theme-secondary-bg-color, #16162a);
          border-radius: 12px; padding: 14px 16px; margin-bottom: 10px;
          border: 1px solid transparent;
        }
        .n-card-hdr {
          display: flex; justify-content: space-between; align-items: flex-start; gap: 8px;
        }
        .n-card-title {
          font-size: 14px; font-weight: 600; word-break: break-all;
          color: var(--tg-theme-accent-text-color, #70b9ff);
          min-width: 0;
        }
        .n-tag {
          display: inline-block; font-size: 11px; padding: 2px 8px; border-radius: 6px;
          background: rgba(46,166,255,.15); color: var(--tg-theme-button-color, #2ea6ff);
          margin-bottom: 6px;
        }
        .n-cookie {
          font-size: 12px; padding: 3px 0; word-break: break-all;
          color: var(--tg-theme-hint-color, #888);
          display: flex; gap: 6px;
        }
        .n-cookie strong { color: var(--tg-theme-text-color, #e0e0e0); white-space: nowrap; }
        .n-empty {
          text-align: center; padding: 48px 0;
          color: var(--tg-theme-hint-color, #888);
        }
        .n-empty svg { width: 48px; height: 48px; margin-bottom: 12px; opacity: .3; }
        .n-empty p { font-size: 14px; }
        .n-section { margin-top: 20px; border-top: 1px solid rgba(255,255,255,.08); padding-top: 16px; }
        .n-toggle { font-size: 13px; color: var(--tg-theme-button-color, #2ea6ff); cursor: pointer; background: none; border: none; padding: 4px 0; display: block; }
        .n-toggle:hover { text-decoration: underline; }
        .n-textarea {
          width: 100%; padding: 10px 12px; border-radius: 10px; border: 1px solid var(--tg-theme-hint-color, #444);
          background: var(--tg-theme-secondary-bg-color, #16162a);
          color: var(--tg-theme-text-color, #e0e0e0); font-size: 13px; outline: none; resize: vertical;
          min-height: 100px; font-family: 'Menlo', 'Consolas', monospace; margin-top: 8px;
          transition: border-color .2s;
        }
        .n-textarea:focus { border-color: var(--tg-theme-button-color, #2ea6ff); }
        .n-input {
          width: 100%; padding: 10px 12px; border-radius: 10px; border: 1px solid var(--tg-theme-hint-color, #444);
          background: var(--tg-theme-secondary-bg-color, #16162a);
          color: var(--tg-theme-text-color, #e0e0e0); font-size: 13px; outline: none; margin-top: 8px;
          transition: border-color .2s;
        }
        .n-input:focus { border-color: var(--tg-theme-button-color, #2ea6ff); }
        .n-msg { font-size: 13px; margin-top: 8px; }
        .n-msg-ok { color: #4caf50; }
        .n-msg-err { color: var(--tg-theme-destructive-text-color, #e74c3c); }
        .n-loading { text-align: center; padding: 48px 0; }
        .n-spinner {
          width: 24px; height: 24px; border: 3px solid var(--tg-theme-hint-color, #444);
          border-top-color: var(--tg-theme-button-color, #2ea6ff);
          border-radius: 50%; animation: spin .6s linear infinite;
          margin: 0 auto 12px;
        }
        @keyframes spin { to { transform: rotate(360deg); } }
        .n-error { text-align: center; padding: 48px 0; }
        .n-error p { color: var(--tg-theme-destructive-text-color, #e74c3c); font-size: 14px; }
      `,
        }}
      />
    </head>
    <body>
      <div id="app"></div>

      <script
        dangerouslySetInnerHTML={{
          __html: `
const { createApp, ref, computed, onMounted } = Vue

const API = window.location.origin
const params = new URLSearchParams(window.location.search)
const initialDevice = params.get('device') || ''
const initData = window.Telegram?.WebApp?.initData

createApp({
  template: \`
    <div class="container">
      <div class="header">
        <h1>Stored Cookies</h1>
      </div>

      <div v-if="loading" class="n-loading">
        <div class="n-spinner"></div>
        <div>Loading...</div>
      </div>

      <div v-else-if="error" class="n-error">
        <p>{{ error }}</p>
      </div>

      <template v-else>
        <template v-if="devices.length">
          <select class="n-select" v-model="activeDevice" v-on:change="onDeviceChange">
            <option value="/all">Global (all devices)</option>
            <option v-for="d in devices" :key="d" :value="d">{{ d }}</option>
          </select>

          <div class="n-actions" v-if="activeDevice">
            <button class="n-btn n-btn-danger" v-on:click="clearDevice" :disabled="clearing">
              Clear {{ activeDevice === '/all' ? 'Global' : activeDevice }}
            </button>
            <button class="n-btn n-btn-danger" v-on:click="unlinkDevice" :disabled="unlinking">
              Unlink {{ activeDevice === '/all' ? 'Global' : activeDevice }}
            </button>
          </div>

          <div v-if="domains.length === 0" class="n-empty">
            <p>No cookies for this device.</p>
          </div>

          <div v-for="(entry, domain) in payload" :key="domain" class="n-card">
            <div class="n-card-hdr">
              <div class="n-card-title">{{ domain }}</div>
              <button class="n-btn n-btn-ghost" v-on:click="deleteDomain(domain)" title="Delete domain">&times;</button>
            </div>
            <span v-if="entry.user_agent" class="n-tag">UA: {{ entry.user_agent }}</span>
            <div v-for="c in entry.cookies" :key="c.name" class="n-cookie">
              <strong>{{ c.name }}:</strong>
              <span>{{ truncate(c.value, 80) }}</span>
            </div>
          </div>
        </template>

        <template v-else>
          <div class="n-empty">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
              <path d="M12 2a10 10 0 1 0 10 10"/>
              <path d="M12 6v6l4 2"/>
            </svg>
            <p>No cookies stored.</p>
          </div>
        </template>

        <div v-if="actionMsg" :class="actionOk ? 'n-msg n-msg-ok' : 'n-msg n-msg-err'">{{ actionMsg }}</div>

        <div class="n-section">
          <button class="n-toggle" v-on:click="showAdd = !showAdd">
            {{ showAdd ? '\\u2212 Cancel' : '+ Add Cookies' }}
          </button>
          <template v-if="showAdd">
            <select class="n-select" v-model="newDevice">
              <option value="">Global (all devices)</option>
              <option v-for="d in devices" :key="d" :value="d">{{ d }}</option>
            </select>
            <input class="n-input" v-model="newUserAgent" placeholder="User agent (optional)" />
            <textarea class="n-textarea" v-model="newCookiesJson" placeholder='Paste cookie JSON array here\\n[{\\"name\\":\\"session\\",\\"value\\":\\"abc\\",\\"domain\\":\\"example.com\\"}]'></textarea>
            <button class="n-btn n-btn-primary" v-on:click="submitCookies" :disabled="saving">
              {{ saving ? 'Saving...' : 'Save Cookies' }}
            </button>
            <div v-if="saveMsg" :class="saveOk ? 'n-msg n-msg-ok' : 'n-msg n-msg-err'">{{ saveMsg }}</div>
          </template>
        </div>
      </template>
    </div>
  \`,
  setup() {
    const loading = ref(true)
    const error = ref('')
    const devices = ref([])
    const payload = ref({})
    const activeDevice = ref(initialDevice)
    const showAdd = ref(false)
    const newDevice = ref('')
    const newUserAgent = ref('')
    const newCookiesJson = ref('')
    const saving = ref(false)
    const saveMsg = ref('')
    const saveOk = ref(false)
    const clearing = ref(false)
    const unlinking = ref(false)
    const actionMsg = ref('')
    const actionOk = ref(false)

    const domains = computed(() => Object.keys(payload.value))

    function truncate(val, max) {
      return val && val.length > max ? val.substring(0, max) + String.fromCharCode(8230) : val
    }

    async function fetchData(device) {
      loading.value = true
      error.value = ''
      try {
        let url = API + '/api/webapp/data?initData=' + encodeURIComponent(initData)
        if (device) url += '&device=' + encodeURIComponent(device)
        const res = await fetch(url)
        const data = await res.json()
        if (!res.ok) {
          error.value = data.error || 'Request failed'
          return
        }
        devices.value = data.devices || []
        payload.value = data.payload || {}
        if (!activeDevice.value || !devices.value.includes(activeDevice.value)) {
          activeDevice.value = devices.value[0] || ''
        }
      } catch (err) {
        error.value = err.message
      } finally {
        loading.value = false
      }
    }

    async function apiPost(path, body) {
      const res = await fetch(API + path, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      })
      const data = await res.json()
      if (!res.ok) throw new Error(data.error || 'Request failed')
      return data
    }

    async function deleteDomain(domain) {
      if (!confirm('Delete cookies for ' + domain + '?')) return
      try {
        await apiPost('/api/webapp/clear', { initData, device: activeDevice.value, domain })
        actionMsg.value = 'Deleted ' + domain
        actionOk.value = true
        fetchData(activeDevice.value)
      } catch (err) {
        actionMsg.value = err.message
        actionOk.value = false
      }
    }

    async function clearDevice() {
      const label = activeDevice.value === '/all' ? 'Global' : activeDevice.value
      if (!confirm('Clear all cookies for ' + label + '?')) return
      clearing.value = true
      try {
        await apiPost('/api/webapp/clear', { initData, device: activeDevice.value })
        actionMsg.value = 'Cleared ' + label
        actionOk.value = true
        fetchData(activeDevice.value)
      } catch (err) {
        actionMsg.value = err.message
        actionOk.value = false
      } finally {
        clearing.value = false
      }
    }

    async function unlinkDevice() {
      const label = activeDevice.value === '/all' ? 'Global' : activeDevice.value
      if (!confirm('Unlink ' + label + '? This will remove all cookies and pairing info.')) return
      unlinking.value = true
      try {
        await apiPost('/api/webapp/unlink', { initData, device: activeDevice.value })
        actionMsg.value = 'Unlinked ' + label
        actionOk.value = true
        fetchData(activeDevice.value)
      } catch (err) {
        actionMsg.value = err.message
        actionOk.value = false
      } finally {
        unlinking.value = false
      }
    }

    async function submitCookies() {
      if (!newCookiesJson.value) {
        saveMsg.value = 'Cookie JSON is required'
        saveOk.value = false
        return
      }
      saving.value = true
      saveMsg.value = ''
      try {
        const data = await apiPost('/api/webapp/cookies', {
          initData,
          device: newDevice.value || '/all',
          cookies: newCookiesJson.value,
          user_agent: newUserAgent.value || undefined,
        })
        const label = newDevice.value || 'Global'
        saveMsg.value = 'Saved ' + data.domains.length + ' domain(s) for "' + label + '"'
        saveOk.value = true
        newCookiesJson.value = ''
        newDevice.value = ''
        newUserAgent.value = ''
        showAdd.value = false
        fetchData(data.device)
      } catch (err) {
        saveMsg.value = err.message
        saveOk.value = false
      } finally {
        saving.value = false
      }
    }

    function onDeviceChange() {
      const newUrl = window.location.pathname + '?device=' + encodeURIComponent(activeDevice.value)
      window.history.replaceState({}, '', newUrl)
      actionMsg.value = ''
      fetchData(activeDevice.value)
    }

    onMounted(() => {
      if (!initData) {
        error.value = 'Not running inside Telegram. Open this from the bot.'
        loading.value = false
        return
      }
      fetchData(activeDevice.value)
    })

    return {
      loading, error, devices, payload, activeDevice, domains, truncate, onDeviceChange,
      showAdd, newDevice, newUserAgent, newCookiesJson, saving, saveMsg, saveOk,
      clearing, unlinking, actionMsg, actionOk,
      deleteDomain, clearDevice, unlinkDevice, submitCookies,
    }
  }
}).mount('#app')
      `,
        }}
      />
    </body>
  </html>
)

app.get("/webapp/cookies", (c) => {
  return c.html(<Page />)
})

export default app
