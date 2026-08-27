import './style.css';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';

const app = document.querySelector('#app');
app.innerHTML = `<header><div><h1>BIN CHD <b>Converter</b></h1><p>Windows · macOS · Linux</p></div><span id="dep">Checking CHDMAN…</span></header>
<main><section class="panel"><nav><button class="tab active" data-mode="convert">Convert</button><button class="tab" data-mode="verify">Verify</button><button class="tab" data-mode="extract">Extract</button></nav>
<label>Source folder</label><div class="field"><input id="source" readonly><button id="pick-source">Browse</button></div>
<div id="output-row"><label>Output folder</label><div class="field"><input id="output" readonly><button id="pick-output">Browse</button></div></div>
<div class="options"><label><input type="checkbox" id="recursive" checked> Include subfolders</label><label><input type="checkbox" id="delete"> Delete source after success</label></div>
<div class="options" id="format-row" hidden><label for="format">Extract format</label><select id="format"><option value="auto">Auto-detect</option><option value="cue">BIN + CUE (CD / GD-ROM)</option><option value="iso">ISO (DVD)</option><option value="img">IMG (HDD)</option></select></div>
<div class="summary"><strong id="count">0</strong><span>compatible files</span></div>
<div class="progress-card" id="progress-card" hidden><div class="progress-head"><strong id="progress-percent">0%</strong><span id="progress-file">Waiting…</span></div><div class="progress-track"><i id="progress-bar"></i></div><div class="metrics"><span>File <b id="metric-file">0/0</b></span><span>Elapsed <b id="metric-elapsed">00:00</b></span><span>Remaining <b id="metric-eta">--:--</b></span></div></div>
<div class="action-row"><button class="primary" id="start">Start conversion</button><button class="danger" id="cancel" hidden>Cancel progress</button></div></section>
<section class="console"><div class="console-title">Detailed activity log <button id="clear">Clear</button></div><pre id="log">Ready.</pre></section></main>`;

let mode = 'convert';
let lastLoggedProgress = -1;
let running = false;
const $ = selector => document.querySelector(selector);
const clock = () => new Date().toLocaleTimeString([], { hour12: false });
const log = message => { $('#log').textContent += `\n[${clock()}] ${message}`; $('#log').scrollTop = $('#log').scrollHeight; };
const duration = seconds => {
  if (seconds == null) return '--:--';
  const value = Math.max(0, Math.round(seconds));
  const hours = Math.floor(value / 3600), minutes = Math.floor(value % 3600 / 60), secs = value % 60;
  return hours ? `${String(hours).padStart(2, '0')}:${String(minutes).padStart(2, '0')}:${String(secs).padStart(2, '0')}` : `${String(minutes).padStart(2, '0')}:${String(secs).padStart(2, '0')}`;
};

async function pick(id) {
  const path = await open({ directory: true, multiple: false });
  if (path) { $(id).value = path; if (id === '#source' && !$('#output').value) $('#output').value = path; await scan(); }
}

async function scan() {
  if (!$('#source').value) return;
  try {
    const files = await invoke('scan_files', { folder: $('#source').value, mode, recursive: $('#recursive').checked });
    $('#count').textContent = files.length;
    log(`Scan complete: ${files.length} compatible file(s).`);
  } catch (error) { log(`ERROR: ${error}`); }
}

await listen('batch-progress', ({ payload }) => {
  $('#progress-card').hidden = false;
  $('#progress-percent').textContent = `${payload.overallPercent.toFixed(1)}%`;
  $('#progress-bar').style.width = `${payload.overallPercent}%`;
  $('#progress-file').textContent = payload.file ? payload.file.split(/[\\/]/).pop() : 'Complete';
  $('#metric-file').textContent = `${payload.fileIndex}/${payload.totalFiles}`;
  $('#metric-elapsed').textContent = duration(payload.elapsedSeconds);
  $('#metric-eta').textContent = duration(payload.remainingSeconds);

  if (payload.kind === 'progress') {
    const rounded = Math.floor(payload.filePercent);
    if (rounded >= lastLoggedProgress + 5 || rounded === 100) {
      lastLoggedProgress = rounded;
      log(`PROGRESS ${payload.fileIndex}/${payload.totalFiles}: ${payload.filePercent.toFixed(1)}% file · ${payload.overallPercent.toFixed(1)}% total · ETA ${duration(payload.remainingSeconds)} · ${payload.message}`);
    }
  } else {
    if (payload.kind === 'start') lastLoggedProgress = -1;
    log(`${payload.kind.toUpperCase()}: ${payload.message}`);
  }
});

$('#pick-source').onclick = () => pick('#source');
$('#pick-output').onclick = () => pick('#output');
$('#recursive').onchange = scan;
$('#clear').onclick = () => $('#log').textContent = '';
$('#cancel').onclick = async () => {
  const wasRunning = await invoke('cancel_batch');
  log(wasRunning ? 'CANCEL REQUESTED: stopping current file…' : 'No running process to cancel.');
};
document.querySelectorAll('.tab').forEach(button => button.onclick = () => {
  document.querySelectorAll('.tab').forEach(item => item.classList.remove('active'));
  button.classList.add('active'); mode = button.dataset.mode;
$('#output-row').hidden = mode === 'verify';
$('#format-row').hidden = mode !== 'extract';
$('#start').textContent = mode === 'convert' ? 'Start conversion' : mode === 'extract' ? 'Start extraction' : 'Start verification';
  scan();
});

$('#start').onclick = async () => {
  if (running) return; // disable re-entry while a batch is in flight
  const source = $('#source').value, output = $('#output').value || source;
  if (!source) return log('Select a source folder.');
  running = true;
  $('#start').disabled = true; $('#cancel').hidden = false;
  $('#progress-card').hidden = false; $('#progress-bar').style.width = '0%'; lastLoggedProgress = -1;
  log(`BATCH START: mode=${mode}, source="${source}", output="${output}", recursive=${$('#recursive').checked}, deleteSource=${$('#delete').checked}`);
  try {
    const lines = await invoke('process_batch', { source, output, mode, recursive: $('#recursive').checked, deleteSource: $('#delete').checked, extractFormat: $('#format').value });
    const canceled = lines.some(line => line.startsWith('CANCELED:'));
    log(`BATCH RESULT: ${lines.filter(line => line.startsWith('OK:')).length} success, ${lines.filter(line => line.startsWith('FAILED:')).length} failed${canceled ? ', canceled' : ''}.`);
    if (canceled) $('#progress-file').textContent = 'Canceled';
    else await scan();
  } catch (error) { log(`ERROR: ${error}`); }
  finally {
    running = false;
    $('#start').disabled = false; $('#cancel').hidden = true;
  }
};

invoke('dependency_status').then(status => {
  $('#dep').textContent = `CHDMAN ${status.chdman ? '✓' : '✕'} · 7-Zip ${status.seven_zip ? '✓' : '–'} · MaxCSO ${status.maxcso ? '✓' : '–'}`;
  $('#dep').className = status.chdman ? 'ok' : 'bad';
});
