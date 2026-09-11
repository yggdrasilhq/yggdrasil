'use strict';

// Everything rendered here comes from the API, which comes from the working
// tree. There is no seeded example state and no placeholder row: when a fact is
// unavailable the UI says it is unavailable. A build tool that shows a
// confident wrong answer is worse than one that shows none.

const $ = (sel) => document.querySelector(sel);

/** The one place the current selection lives. */
const state = {
  repo: null,
  config: '',
  profile: '',
  runPoll: null,
};

const esc = (value) =>
  String(value ?? '').replace(/[&<>"']/g, (ch) => (
    { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[ch]
  ));

async function api(path, options) {
  const response = await fetch(path, options);
  const body = await response.json().catch(() => ({ error: `HTTP ${response.status}` }));
  if (!response.ok) throw new Error(body.error || `HTTP ${response.status}`);
  return body;
}

/* ---- view switching ---------------------------------------------------- */

function showView(name) {
  document.querySelectorAll('.segmented button').forEach((btn) => {
    const on = btn.dataset.view === name;
    btn.classList.toggle('active', on);
    btn.setAttribute('aria-selected', String(on));
  });
  document.querySelectorAll('.view').forEach((view) => {
    view.classList.toggle('active', view.id === `view-${name}`);
  });
  if (name === 'plan') loadPlan();
  // The run log only polls while its tab is up: a surface that keeps a timer
  // running behind a hidden view is how a viewport starts costing CPU for
  // nothing.
  if (name === 'run') startRunPoll(); else stopRunPoll();
}

document.querySelectorAll('.segmented button').forEach((btn) => {
  btn.addEventListener('click', () => showView(btn.dataset.view));
});

/* ---- home -------------------------------------------------------------- */

const bytes = (n) => (n >= 1 << 30 ? `${(n / (1 << 30)).toFixed(2)} GB` : `${(n / (1 << 20)).toFixed(0)} MB`);

async function loadHome() {
  let repo;
  try {
    repo = await api('/api/repo');
  } catch (err) {
    $('#home-body').innerHTML = `<div class="card"><div class="err-box">Could not read the repository: ${esc(err.message)}</div></div>`;
    return;
  }
  state.repo = repo;
  $('#repo-root').textContent = repo.root;

  const lb = repo.live_build_present
    ? '<span class="pill ok">live-build present</span>'
    : '<span class="pill warn">live-build missing</span>';

  const profiles = repo.profiles_error
    ? `<div class="err-box">The profile list could not be read: ${esc(repo.profiles_error)}</div>`
    : repo.profiles.map((p) => `
        <div class="row">
          <div class="name"><code>--profile ${esc(p.name)}</code></div>
          <div class="meta">${p.composite ? 'Composite — expands into every single-image profile.' : 'Builds one image.'}</div>
        </div>`).join('');

  const configs = repo.configs.length ? repo.configs.map((c) => `
      <div class="row">
        <div class="name">
          <code>${esc(c.name)}</code>
          ${c.is_default ? '<span class="pill ok">default</span>' : ''}
          ${c.is_example ? '<span class="pill muted">example</span>' : ''}
        </div>
        <div class="meta">${esc(c.summary) || '<em>no header comment</em>'} — ${c.knob_count} knobs</div>
      </div>`).join('')
    : `<div class="warn-box">No <code>ygg*.toml</code> in the checkout. Copy <code>ygg.example.toml</code> to <code>ygg.local.toml</code> to build.</div>`;

  const artifacts = repo.artifacts.length
    ? repo.artifacts.map((a) => `<div class="row"><div class="name"><code>${esc(a.name)}</code></div><div class="meta">${bytes(a.bytes)}</div></div>`).join('')
    : '<p class="sub" style="margin:0">Nothing built yet — <code>./artifacts</code> holds no ISO.</p>';

  $('#home-body').innerHTML = `
    <div class="card">
      <h2>Checkout</h2>
      <div class="facts">
        <div class="fact"><b>${repo.hooks_count}</b><span>chroot hooks</span></div>
        <div class="fact"><b>${repo.package_lists.length}</b><span>package lists</span></div>
        <div class="fact"><b>${repo.configs.length}</b><span>configs</span></div>
        <div class="fact"><b>${repo.artifacts.length}</b><span>built ISOs</span></div>
      </div>
      <p class="sub" style="margin-top:12px">${lb} ${repo.live_build_present ? '' : 'The image stages cannot run on this host; the config stages still can.'}</p>
    </div>

    <div class="card">
      <h2>Profiles</h2>
      <p class="sub">Read from <code>mkconfig.sh</code>'s own usage block, so this list cannot drift from the entry point.</p>
      ${profiles}
    </div>

    <div class="card">
      <h2>Configs</h2>
      <p class="sub">Every <code>ygg*.toml</code> in the checkout root. <code>${esc(repo.configs.find((c) => c.is_default)?.name || 'ygg.local.toml')}</code> is what a bare <code>./mkconfig.sh</code> reaches for.</p>
      ${configs}
    </div>

    <div class="card">
      <h2>Package lists</h2>
      ${repo.package_lists.map((n) => `<div class="row"><div class="name"><code>${esc(n)}</code></div></div>`).join('') || '<p class="sub" style="margin:0">none</p>'}
    </div>

    <div class="card">
      <h2>Artifacts</h2>
      ${artifacts}
    </div>`;

  fillSelectors();
}

/* ---- selectors --------------------------------------------------------- */

function fillSelectors() {
  const repo = state.repo;
  if (!repo) return;

  const configSel = $('#plan-config');
  configSel.innerHTML = repo.configs
    .map((c) => `<option value="${esc(c.name)}">${esc(c.name)}${c.is_example ? ' (example)' : ''}</option>`)
    .join('');
  // Prefer a real buildable config over an example, which is the choice a user
  // almost always wants and the one the old GUI got wrong.
  const preferred = repo.configs.find((c) => c.is_default) || repo.configs.find((c) => !c.is_example) || repo.configs[0];
  if (preferred) {
    configSel.value = preferred.name;
    state.config = preferred.name;
  }

  const profileSel = $('#plan-profile');
  profileSel.innerHTML = ['<option value="">(from the config)</option>']
    .concat(repo.profiles.map((p) => `<option value="${esc(p.name)}">${esc(p.name)}</option>`))
    .join('');
  profileSel.value = state.profile;

  configSel.onchange = () => { state.config = configSel.value; loadPlan(); };
  profileSel.onchange = () => { state.profile = profileSel.value; loadPlan(); };
  $('#plan-skip-smoke').onchange = loadPlan;
}

/* ---- plan -------------------------------------------------------------- */

async function loadPlan() {
  if (!state.config) return;
  const skip = $('#plan-skip-smoke').checked ? '1' : '0';
  const query = `config=${encodeURIComponent(state.config)}&profile=${encodeURIComponent(state.profile)}&skip_smoke=${skip}`;

  let plan;
  try {
    plan = await api(`/api/plan?${query}`);
  } catch (err) {
    $('#plan-body').innerHTML = `<div class="card"><div class="err-box">${esc(err.message)}</div></div>`;
    return;
  }

  const warnings = plan.warnings.map((w) => `<div class="warn-box">${esc(w)}</div>`).join('');

  const resolution = plan.profile_from_config
    ? `<p class="sub">No profile was chosen, so <code>mkconfig.sh</code> takes it from the config's <code>build_profile</code>: <b>${esc(plan.profile_from_config)}</b>.</p>`
    : '';

  const steps = plan.steps.map((s) => `
    <div class="step">
      <div class="step-head">
        <span class="n">${s.index}</span>
        <span class="t">${esc(s.title)}</span>
        <span class="pill ${s.cost === 'cheap' ? 'ok' : s.cost === 'long' ? 'bad' : 'warn'}">${
          s.cost === 'cheap' ? 'seconds, no root' : s.cost === 'long' ? 'root, tens of minutes' : 'needs root'
        }</span>
      </div>
      ${s.command.length ? `<pre>${esc(s.command.join(' '))}</pre>` : ''}
      <p class="note">${esc(s.note)}</p>
    </div>`).join('');

  const deltas = plan.delta.length ? plan.delta.map((d) => {
    if (d.kind === 'changed') return `<div class="delta"><span class="k">${esc(d.key)}</span> <span class="from">${esc(d.baseline)}</span> → <span class="to">${esc(d.value)}</span></div>`;
    if (d.kind === 'added') return `<div class="delta"><span class="k">${esc(d.key)}</span> <span class="pill ok">added</span> <span class="to">${esc(d.value)}</span></div>`;
    return `<div class="delta"><span class="k">${esc(d.key)}</span> <span class="pill warn">absent</span> <span class="from">${esc(d.baseline)}</span> — falls back to build-profile.sh's own default</div>`;
  }).join('') : '<p class="sub" style="margin:0">Identical to the shipped example.</p>';

  $('#plan-body').innerHTML = `
    ${warnings}
    <div class="card">
      <h2>What will run</h2>
      <p class="sub">Profile <b>${esc(plan.requested_profile)}</b>${
        plan.effective_profiles.length > 1 ? ` → builds ${plan.effective_profiles.map(esc).join(' and ')}` : ''
      }. Nothing below has been executed.</p>
      ${resolution}
      ${steps}
    </div>

    <div class="card">
      <h2>Config delta vs ygg.example.toml</h2>
      <p class="sub">What <code>${esc(plan.config)}</code> changes from the shipped defaults.</p>
      ${deltas}
    </div>

    <div class="card env">
      <h2>Resolved environment</h2>
      <p class="sub">Produced by really running <code>scripts/toml-to-env.sh</code> — this is the environment the build sources, not a model of it. ${plan.env.length} variables.</p>
      <pre>${esc(plan.env.map(([k, v]) => `${k}="${v}"`).join('\n'))}</pre>
    </div>`;
}

/* ---- run --------------------------------------------------------------- */

$('#run-start').addEventListener('click', async () => {
  if (!state.config) return;
  const query = `config=${encodeURIComponent(state.config)}&profile=${encodeURIComponent(state.profile)}`;
  $('#run-start').disabled = true;
  try {
    renderRun(await api(`/api/run?${query}`, { method: 'POST' }));
  } catch (err) {
    $('#run-log').innerHTML = `<span class="stderr">${esc(err.message)}</span>`;
    $('#run-start').disabled = false;
  }
  startRunPoll();
});

function startRunPoll() {
  stopRunPoll();
  const tick = async () => {
    try { renderRun(await api('/api/run')); } catch { /* transient; the next tick retries */ }
  };
  tick();
  state.runPoll = setInterval(tick, 500);
}

function stopRunPoll() {
  if (state.runPoll) clearInterval(state.runPoll);
  state.runPoll = null;
}

function renderRun(status) {
  const badge = $('#run-state');
  badge.className = `state ${status.state}`;
  badge.textContent = status.state;
  $('#run-start').disabled = status.state === 'running' || !state.config;

  const log = $('#run-log');
  const head = status.command.length
    ? `<span class="exit">$ ${esc(status.command.join(' '))}</span>\n`
    : '';
  const body = status.lines
    .map((l) => (l.stream === 'stderr' ? `<span class="stderr">${esc(l.text)}</span>` : esc(l.text)))
    .join('\n');
  let tail = '';
  if (status.error) tail = `\n<span class="stderr">${esc(status.error)}</span>`;
  else if (status.state === 'done') tail = `\n<span class="exit">— finished, exit ${status.exit_code}</span>`;
  else if (status.state === 'failed') tail = `\n<span class="stderr">— failed, exit ${status.exit_code}</span>`;

  const atBottom = log.scrollHeight - log.scrollTop - log.clientHeight < 40;
  log.innerHTML = head + body + tail || '<span class="exit">Nothing has run yet.</span>';
  // Follow the tail only when the user is already at it; yanking the viewport
  // away from something they scrolled up to read is its own bug.
  if (atBottom) log.scrollTop = log.scrollHeight;

  if (status.state !== 'running') stopRunPoll();
}

loadHome();
