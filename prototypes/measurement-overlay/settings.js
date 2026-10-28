'use strict';
// PROTOTYPE — the Settings window (decided in WPC-278, footer from WPC-277), drawn in the
// toolbar's style. Throwaway: changes live in memory and vanish on reload.

const setEl = $('#settings');
const SX = { open: false, rec: null, recMsg: null, startNote: false, upd: null, updN: 0, pos: null, drag: null };
const ST = {
  pt: { title: 'Pixmeter — Configurações', general: 'Geral', measure: 'Medidas', shortcut: 'Atalho global', reset: 'Restaurar', press: 'Aperte as teclas…',
    used: 'Já usada por outro programa', busy: k => `${k} já é usado por outro programa. Escolha outro.`,
    ctrlAlt: 'Ctrl + Alt sem o Win não vale: no teclado brasileiro é o AltGr e roubaria caracteres como / e ?.',
    needKey: 'Use Win, Ctrl ou Alt com uma letra, um número ou uma tecla de F1 a F12.',
    startWin: 'Iniciar com o Windows', startBlocked: 'Desligado nas configurações do Windows. Ligue em Configurações > Aplicativos > Inicialização.',
    language: 'Idioma', auto: 'Automático', theme: 'Tema', light: 'Claro', dark: 'Escuro',
    tolDef: 'Tolerância padrão', tolNote: 'A roda do mouse ajusta durante o uso.',
    logical: 'Mostrar também o pixel de escala', logNote: 'Quando a escala do monitor não é 100%.',
    colorFmt: 'Formato da cor', both: 'Os dois', dest: 'Destino', clip: 'Área de transferência', file: 'Arquivo',
    saveFolder: 'Salvar na pasta padrão', change: 'Alterar…', noFolder: 'Nenhuma pasta escolhida', folderNote: 'Só vale quando o destino inclui arquivo.',
    version: 'Versão 1.0.0', oss: 'Código aberto (MIT)', page: 'Página do projeto', check: 'Verificar atualizações',
    checking: 'Verificando…', latest: 'Você está na versão mais recente.', avail: 'Versão 1.1.0 disponível.', download: 'Baixar',
    fail: 'Não foi possível verificar. Confira a conexão e tente de novo.' },
  en: { title: 'Pixmeter Settings', general: 'General', measure: 'Measurements', shortcut: 'Global shortcut', reset: 'Reset', press: 'Press the keys…',
    used: 'Already used by another program', busy: k => `${k} is already used by another program. Choose another.`,
    ctrlAlt: 'Ctrl + Alt without Win isn’t allowed: it works as AltGr and would steal characters like / and ?.',
    needKey: 'Use Win, Ctrl or Alt with a letter, a number or a key from F1 to F12.',
    startWin: 'Start with Windows', startBlocked: 'Turned off in Windows settings. Turn it on in Settings > Apps > Startup.',
    language: 'Language', auto: 'Automatic', theme: 'Theme', light: 'Light', dark: 'Dark',
    tolDef: 'Default tolerance', tolNote: 'The mouse wheel adjusts it while measuring.',
    logical: 'Also show logical pixels', logNote: 'When the display scale isn’t 100%.',
    colorFmt: 'Color format', both: 'Both', dest: 'Destination', clip: 'Clipboard', file: 'File',
    saveFolder: 'Save to default folder', change: 'Change…', noFolder: 'No folder chosen', folderNote: 'Only applies when the destination includes a file.',
    version: 'Version 1.0.0', oss: 'Open source (MIT)', page: 'Project page', check: 'Check for updates',
    checking: 'Checking…', latest: 'You’re on the latest version.', avail: 'Version 1.1.0 is available.', download: 'Download',
    fail: 'Couldn’t check. Check your connection and try again.' },
};
const st = k => ST[lang()][k];
const DEFAULT_SHORTCUT = ['Win', 'Ctrl', 'M'];
const MOD_ORDER = ['Win', 'Ctrl', 'Alt', 'Shift'];

function openSettings() {
  const wasOpen = SX.open;
  SX.open = true;
  renderSettings();
  if (!wasOpen || !SX.pos) {
    // Centered on the monitor under the cursor, at its real size even if it doesn't fit.
    const mon = S.cur.mon || MONS[0], w = setEl.offsetWidth, h = setEl.offsetHeight;
    SX.pos = { x: mon.x / dpr + (mon.w / dpr - w) / 2, y: Math.max(mon.y / dpr, mon.y / dpr + (mon.h / dpr - h) / 2) };
  }
  placeSettings();
  render();
}
function placeSettings() { setEl.style.left = SX.pos.x + 'px'; setEl.style.top = SX.pos.y + 'px'; }

function keycaps(keys) { return keys.map(k => `<span class="kc">${k}</span>`).join('<span class="plus">+</span>'); }
function segc(key, opts) {
  return `<span class="segc">${opts.map(([v, l]) => `<button data-seg="${key}" data-val="${v}" class="${S.settings[key] === v ? 'on' : ''}">${l}</button>`).join('')}</span>`;
}
// Long paths are cut in the middle, as decided in WPC-278.
function midEllipsis(p, max = 46) { return p.length <= max ? p : p.slice(0, 16) + '…' + p.slice(p.length - (max - 17)); }

function renderSettings() {
  setEl.hidden = !SX.open;
  if (!SX.open) return;
  const s = S.settings;
  let recField;
  if (SX.rec) recField = SX.rec.length ? `${keycaps(SX.rec)}<span class="plus">+</span><span class="wait">…</span>` : `<span class="wait">${st('press')}</span>`;
  else recField = keycaps(s.shortcut);
  const busy = S.scen.shortcutBusy && !SX.rec;
  const recMsg = busy ? `<span class="msg">${st('busy')(s.shortcut.join(' + '))}</span>` : SX.recMsg ? `<span class="msg">${SX.recMsg}</span>` : '';
  const startOn = s.startWin && !S.scen.startupBlocked;
  const folder = s.folder ? `<span class="fpath" title="${s.folder}">${midEllipsis(s.folder)}</span>` : `<span class="fpath empty">${st('noFolder')}</span>`;
  let upd = '';
  if (SX.upd === 'checking') upd = `<div class="ustat">${st('checking')}</div>`;
  else if (SX.upd === 'latest') upd = `<div class="ustat"><span class="ok">${IC.ok}</span>${st('latest')}</div>`;
  else if (SX.upd === 'avail') upd = `<div class="ustat">${st('avail')} <button class="lnk" data-a="download">${st('download')}</button></div>`;
  else if (SX.upd === 'fail') upd = `<div class="ustat"><span class="bad">${IC.bad}</span>${st('fail')}</div>`;
  setEl.innerHTML = `
  <div class="tbar"><span class="appic"></span><span class="ttl">${st('title')}</span><button class="capb" data-a="min" title="Minimizar">—</button><button class="capb x" data-a="close" title="Fechar">✕</button></div>
  <div class="body">
    <section class="sec"><h5>${st('general')}</h5><div class="scard">
      <div class="srow ${busy ? 'warn' : ''}"><span class="lab">${st('shortcut')}${recMsg}</span><span class="rec ${SX.rec ? 'recording' : ''}" data-a="rec">${recField}</span><button class="pbtn" data-a="reset">${st('reset')}</button></div>
      <div class="srow"><span class="lab">${st('startWin')}${SX.startNote && S.scen.startupBlocked ? `<span class="msg">${st('startBlocked')}</span>` : ''}</span><button class="tgl ${startOn ? 'on' : ''}" data-a="start"></button></div>
      <div class="srow"><span class="lab">${st('language')}</span>${segc('lang', [['auto', st('auto')], ['en', 'English'], ['pt', 'Português']])}</div>
      <div class="srow"><span class="lab">${st('theme')}</span>${segc('theme', [['auto', st('auto')], ['light', st('light')], ['dark', st('dark')]])}</div>
    </div></section>
    <section class="sec"><h5>${st('measure')}</h5><div class="scard">
      <div class="srow"><span class="lab">${st('tolDef')}<span class="note">${st('tolNote')}</span></span><span class="step"><button class="sbtn" data-a="tol-">−</button><span class="num">${s.tolDefault}</span><button class="sbtn" data-a="tol+">+</button></span></div>
      <div class="srow"><span class="lab">${st('logical')}<span class="note">${st('logNote')}</span></span><button class="tgl ${s.showLogical ? 'on' : ''}" data-a="logical"></button></div>
    </div></section>
    <section class="sec"><h5>Inspect</h5><div class="scard">
      <div class="srow"><span class="lab">${st('colorFmt')}</span>${segc('colorFmt', [['hex', 'HEX'], ['rgb', 'RGB'], ['both', st('both')]])}</div>
    </div></section>
    <section class="sec"><h5>Snip</h5><div class="scard">
      <div class="srow"><span class="lab">${st('dest')}</span>${segc('snipDest', [['clip', st('clip')], ['file', st('file')], ['both', st('both')]])}</div>
      <div><div class="srow" style="border-top:1px solid var(--card-edge)"><span class="lab">${st('saveFolder')}</span><button class="tgl ${s.folderOn ? 'on' : ''}" data-a="folder"></button></div>
        <div class="fline">${folder}<button class="pbtn" data-a="change">${st('change')}</button></div>
        ${s.snipDest === 'clip' ? `<div class="fnote">${st('folderNote')}</div>` : ''}</div>
    </div></section>
  </div>
  <div class="sfoot"><span>${st('version')}</span><span>·</span><span>${st('oss')}</span><span class="sp"></span><button class="lnk" data-a="page">${st('page')}</button><span>·</span><button class="lnk" data-a="check">${st('check')}</button></div>
  ${upd}`;
}

setEl.addEventListener('click', e => {
  const b = e.target.closest('[data-a], [data-seg]'); if (!b) return;
  const s = S.settings, a = b.dataset.a;
  if (b.dataset.seg) s[b.dataset.seg] = b.dataset.val;
  else if (a === 'close' || a === 'min') { SX.open = false; SX.rec = null; S.msg = a === 'min' ? 'Configurações minimizada na barra de tarefas. O botão "Configurações" traz a mesma janela de volta.' : ''; }
  else if (a === 'rec') { SX.rec = SX.rec ? SX.rec : []; SX.recMsg = null; }
  else if (a === 'reset') { s.shortcut = DEFAULT_SHORTCUT.slice(); SX.rec = null; SX.recMsg = null; }
  else if (a === 'start') { if (S.scen.startupBlocked) SX.startNote = true; else s.startWin = !s.startWin; }
  else if (a === 'tol-' || a === 'tol+') s.tolDefault = clamp(s.tolDefault + (a === 'tol+' ? 1 : -1), 0, TOL_MAX);
  else if (a === 'logical') s.showLogical = !s.showLogical;
  else if (a === 'folder') {
    // Turning it on needs a folder; choosing one is part of turning on, and cancelling leaves it off.
    if (s.folderOn) s.folderOn = false;
    else if (s.folder) s.folderOn = true;
    else pickFolder(path => { s.folder = path; s.folderOn = true; });
  }
  else if (a === 'change') pickFolder(path => { s.folder = path; });
  else if (a === 'page') S.msg = 'O link abriria a página do projeto no GitHub, no navegador.';
  else if (a === 'download') S.msg = 'O link abriria a página da versão nova no GitHub, no navegador.';
  else if (a === 'check') {
    if (S.scen.copy === 'store') { SX.upd = null; S.msg = 'Na cópia da Store, o clique abriria a página do Pixmeter na Microsoft Store.'; }
    else {
      SX.upd = 'checking';
      const result = ['latest', 'avail', 'fail'][SX.updN++ % 3];
      setTimeout(() => { SX.upd = result; renderSettings(); }, 900);
    }
  }
  renderSettings(); renderToolbar(); render();
});

// Click outside the recorder cancels recording and keeps the previous shortcut.
document.addEventListener('pointerdown', e => {
  if (SX.rec && !e.target.closest('.rec')) { SX.rec = null; renderSettings(); }
}, true);

// Drag the window by its title bar.
setEl.addEventListener('pointerdown', e => {
  if (!e.target.closest('.tbar') || e.target.closest('button')) return;
  SX.drag = { dx: e.clientX - SX.pos.x, dy: e.clientY - SX.pos.y };
  setEl.setPointerCapture(e.pointerId);
});
setEl.addEventListener('pointermove', e => { if (SX.drag) { SX.pos = { x: e.clientX - SX.drag.dx, y: e.clientY - SX.drag.dy }; placeSettings(); } });
setEl.addEventListener('pointerup', () => { SX.drag = null; });

// The shortcut recorder: returns true when it consumed the key.
function settingsKey(e) {
  if (!SX.open || !SX.rec) return false;
  e.preventDefault();
  if (e.key === 'Escape') { SX.rec = null; SX.recMsg = null; renderSettings(); return true; }
  const mods = [];
  if (e.metaKey || e.key === 'Meta') mods.push('Win');
  if (e.ctrlKey) mods.push('Ctrl');
  if (e.altKey) mods.push('Alt');
  if (e.shiftKey) mods.push('Shift');
  if (['Meta', 'OS', 'Control', 'Alt', 'Shift', 'AltGraph'].includes(e.key)) { SX.rec = mods; renderSettings(); return true; }
  let main = null;
  if (/^Key[A-Z]$/.test(e.code)) main = e.code.slice(3);
  else if (/^Digit[0-9]$/.test(e.code)) main = e.code.slice(5);
  else if (/^F([1-9]|1[0-2])$/.test(e.key)) main = e.key;
  const hasBase = mods.includes('Win') || mods.includes('Ctrl') || mods.includes('Alt');
  if (!main || !hasBase) { SX.recMsg = st('needKey'); SX.rec = mods; renderSettings(); return true; }
  if (mods.includes('Ctrl') && mods.includes('Alt') && !mods.includes('Win')) { SX.recMsg = st('ctrlAlt'); SX.rec = []; renderSettings(); return true; }
  const combo = [...MOD_ORDER.filter(m => mods.includes(m)), main];
  if (combo.join('+') === 'Ctrl+Shift+S') { SX.recMsg = st('used'); SX.rec = null; renderSettings(); return true; }
  S.settings.shortcut = combo; S.scen.shortcutBusy = false; SX.rec = null; SX.recMsg = null;
  renderSettings();
  return true;
}
window.addEventListener('keyup', e => {
  if (!SX.rec) return;
  const mods = [];
  if (e.metaKey && e.key !== 'Meta') mods.push('Win');
  if (e.ctrlKey) mods.push('Ctrl');
  if (e.altKey) mods.push('Alt');
  if (e.shiftKey) mods.push('Shift');
  SX.rec = mods; renderSettings();
});

// Windows' folder picker, simulated with the same stand-in as Save As.
function pickFolder(onOk) {
  S.saveAs = ok => { if (ok) { onOk($('#fpPath').value); renderSettings(); } };
  saveEl.hidden = false;
  saveEl.innerHTML = `<div class="dlg"><div class="tt"><b>Selecionar pasta</b><span>janela do Windows, simulada</span></div>
    <div class="bd"><label>Pasta</label><input id="fpPath" value="C:\\Users\\ana\\Pictures\\Capturas de tela\\Projetos 2026\\Pixmeter"></div>
    <div class="ac"><button class="pri" id="fpOk">Selecionar pasta</button><button id="fpNo">Cancelar</button></div></div>`;
  $('#fpOk').onclick = () => closeSaveAs(true);
  $('#fpNo').onclick = () => closeSaveAs(false);
}
