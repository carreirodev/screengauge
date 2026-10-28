'use strict';
// PROTOTYPE — toolbar, tools and the three overlay variants for WPC-279. Throwaway code:
// no tests, no persistence, error handling only where it keeps the page running.

const $ = s => document.querySelector(s);
const screens = $('#screens'), desk = $('#desk'), ovl = $('#ovl'), pm = $('#pm');
const dctx = desk.getContext('2d', { willReadFrequently: true }), octx = ovl.getContext('2d');
const labelsEl = $('#labels'), fxEl = $('#fx'), magEl = $('#mag'), tb = $('#toolbar'), mini = $('#tbMini'), saveEl = $('#saveas');

let dpr = 1, MONS = [], IMG = null, CW = 0;

// ---------------------------------------------------------------- text
const TXT = {
  pt: { dim: 'Dimensions', bounds: 'Bounds', inspect: 'Inspect', snip: 'Snip', settings: 'Configurações', minimize: 'Minimizar', close: 'Fechar', expand: 'Mostrar a barra',
    H: 'Horizontal', V: 'Vertical', C: 'Cruz', tol: 'Tolerância', copied: 'Copiado', saved: 'Salvo', copiedSaved: 'Copiado e salvo',
    noContent: 'Nenhum conteúdo encontrado', cantSave: 'Não foi possível salvar.', imageCopied: 'A imagem foi copiada.',
    approx: 'Cor aproximada (HDR ligado)', phys: 'real', logi: 'de escala', drag: 'Arraste para medir', dragSnip: 'Arraste para capturar', image: 'imagem' },
  en: { dim: 'Dimensions', bounds: 'Bounds', inspect: 'Inspect', snip: 'Snip', settings: 'Settings', minimize: 'Minimize', close: 'Close', expand: 'Show the toolbar',
    H: 'Horizontal', V: 'Vertical', C: 'Cross', tol: 'Tolerance', copied: 'Copied', saved: 'Saved', copiedSaved: 'Copied and saved',
    noContent: 'No content found', cantSave: "Couldn't save.", imageCopied: 'The image was copied.',
    approx: 'Approximate color (HDR on)', phys: 'physical', logi: 'logical', drag: 'Drag to measure', dragSnip: 'Drag to capture', image: 'image' },
};
const lang = () => S.settings.lang !== 'auto' ? S.settings.lang : (navigator.language || 'en').toLowerCase().startsWith('pt') ? 'pt' : 'en';
const t = k => TXT[lang()][k];

// ---------------------------------------------------------------- variants
// Every choice a variant makes is listed in CHOICES, numbered the same in all three,
// so the owner can answer "2 de B, 5 de A".
const VARS = {
  A: { key: 'A', name: 'Etiqueta na medida', color: '#22d3ee', halo: 'rgba(0,0,0,.28)', modes: 'slot', mag: { shape: 'square', css: 176, grid: 8 } },
  B: { key: 'B', name: 'Cartão no cursor', color: '#ff2d95', halo: null, modes: 'tab', mag: { shape: 'round', css: 168, grid: 0 } },
  C: { key: 'C', name: 'Cota técnica', color: '#ff7a1a', halo: 'rgba(0,0,0,.45)', modes: 'slot', mag: { shape: 'square', css: 216, grid: 4 } },
};
const CHOICE_TOPICS = ['Segundo número (pixel de escala)', 'Onde fica o número', 'Ao vivo × fixada', 'A linha', 'A lupa (Inspect)', 'Avisos ("Copiado")', 'Indicadores (tolerância e ampliação)', 'Snip × Bounds', 'Botões de modo H/V/C'];
const CHOICES = {
  A: ['Na mesma etiqueta, depois de um traço, mais apagado: <code>240 │ 160</code>',
    'Etiqueta colada na medida: acima da linha horizontal, à direita da vertical, no canto do retângulo',
    'Ao vivo, a etiqueta é cheia na cor da linha; fixada, fica no tema com uma faixa colorida',
    'Ciano, 1 pixel, com tracinhos nas pontas e contorno escuro',
    'Quadrada, grade a partir de 8×, posição e cor numa faixa embaixo',
    'Pequenos, acima e à direita do cursor',
    'Pílula acima e à esquerda do cursor; a ampliação aparece no canto da lupa; os dois somem em 1 s',
    'Snip com cantoneiras de câmera, sem preenchimento; o Bounds tem contorno e fundo claro',
    'Num espaço guardado logo depois das ferramentas (fica vazio nas outras)'],
  B: ['Em linha própria, com nome: <code>real 240</code> / <code>de escala 160</code>',
    'Um cartão que segue o cursor mostra o número enquanto mede, igual em todas as ferramentas; a linha não tem etiqueta',
    'Fixada ganha uma etiqueta pequena na ponta, com os dois números empilhados',
    'Magenta, 1 pixel, sem pontas',
    'Redonda, sem grade, com mira; posição e cor no cartão embaixo dela',
    'Dentro do cartão',
    'Dentro do cartão, sempre à vista (a tolerância acende quando muda)',
    'Snip com contorno simples e o tamanho da imagem no cartão; o Bounds tem fundo claro',
    'Numa aba pendurada embaixo do botão do Dimensions'],
  C: ['Entre parênteses: <code>240 (160)</code>',
    'No meio da linha, num vão aberto nela, como cota de desenho técnico; no retângulo, uma cota em cima (largura) e outra ao lado (altura)',
    'Mesma forma; ao vivo o número é solto, fixado ganha uma caixa',
    'Laranja, 1 pixel, com setas nas pontas e linhas de chamada',
    'Quadrada e maior, grade a partir de 4×, informações ao lado; régua de ampliação sempre à vista',
    'No rodapé do monitor atual, centralizados',
    'Tolerância no rodapé do monitor, com barra, some em 1 s; ampliação na régua ao lado da lupa',
    'Snip com retângulo tracejado e etiqueta só com o tamanho da imagem',
    'Num espaço guardado logo depois das ferramentas (como A)'],
};

// ---------------------------------------------------------------- state
const S = {
  variant: (new URLSearchParams(location.search).get('variant') || 'A').toUpperCase(),
  open: true, tool: 'dim', mode: 'C', mag: 8, tol: 2,
  pinned: [], drag: null, alt: false,
  cur: { x: -1, y: -1, mon: null }, overTb: false,
  tbMin: false, tbPos: null, tbRel: { monId: 1, fx: .5, fy: 0 }, tbDrag: null,
  toast: null, tolAt: 0, magAt: 0,
  clip: '—', msg: '', saveAs: null,
  settings: { lang: 'auto', theme: 'auto', tolDefault: 2, showLogical: true, colorFmt: 'both', snipDest: 'clip', folderOn: false, folder: '', shortcut: ['Win', 'Ctrl', 'M'], startWin: false },
  scen: { hdr2: false, saveFail: false, shortcutBusy: false, startupBlocked: false, copy: 'installer' },
};
if (!VARS[S.variant]) S.variant = 'A';
const V = () => VARS[S.variant];
const TOL_MAX = 50, tolThr = () => S.tol * 3; // calibrated in the real build; any scale works here
const MAGS = [2, 4, 8, 16, 32];

// ---------------------------------------------------------------- layout and the frozen screen
function layout() {
  dpr = window.devicePixelRatio || 1;
  const r = screens.getBoundingClientRect();
  const cw = Math.round(r.width * dpr), ch = Math.round(r.height * dpr);
  for (const c of [desk, ovl]) { c.width = cw; c.height = ch; c.style.width = cw / dpr + 'px'; c.style.height = ch / dpr + 'px'; }
  const pad = Math.round(12 * dpr), top = Math.round(26 * dpr), gap = Math.round(16 * dpr);
  const mw = Math.floor((cw - 2 * pad - gap) / 2), mh = ch - top - pad;
  MONS = [
    { id: 1, scale: 1.5, x: pad, y: top, w: mw, h: mh },
    { id: 2, scale: 1.0, x: pad + mw + gap, y: top, w: mw, h: mh },
  ];
  drawDesktop(dctx, MONS);
  CW = cw;
  IMG = dctx.getImageData(0, 0, cw, ch).data;
  $('#caps').innerHTML = MONS.map(m =>
    `<div class="cap" style="left:${m.x / dpr}px;top:${m.y / dpr - 17}px"><b>Monitor ${m.id}</b> · escala ${m.scale * 100}% · ${m.w} × ${m.h} pixels reais${m.scale !== 1 ? '' : ' · conteúdo menor porque está a 100%'}</div>`).join('');
  S.pinned = []; S.drag = null;
  placeToolbar(MONS[0]);
  render();
}
const monAt = (x, y) => MONS.find(m => x >= m.x && x < m.x + m.w && y >= m.y && y < m.y + m.h) || null;
const at = (x, y) => (y * CW + x) * 4;
const differs = (i, j, thr) => Math.abs(IMG[i] - IMG[j]) > thr || Math.abs(IMG[i + 1] - IMG[j + 1]) > thr || Math.abs(IMG[i + 2] - IMG[j + 2]) > thr;
const clamp = (v, a, b) => Math.max(a, Math.min(b, v));

// Dimensions: the gap between the edges around the cursor, on its exact row and column.
function measureDim(cx, cy, mon, mode) {
  const thr = tolThr(), ref = at(cx, cy), m = { kind: 'dim', mode, mon, cx, cy };
  if (mode !== 'V') {
    let l = cx - 1; while (l >= mon.x && !differs(at(l, cy), ref, thr)) l--;
    let r = cx + 1; while (r < mon.x + mon.w && !differs(at(r, cy), ref, thr)) r++;
    m.x1 = l + 1; m.x2 = r - 1; m.w = m.x2 - m.x1 + 1;
  }
  if (mode !== 'H') {
    let u = cy - 1; while (u >= mon.y && !differs(at(cx, u), ref, thr)) u--;
    let d = cy + 1; while (d < mon.y + mon.h && !differs(at(cx, d), ref, thr)) d++;
    m.y1 = u + 1; m.y2 = d - 1; m.h = m.y2 - m.y1 + 1;
  }
  return m;
}

// Bounds and Snip: shrink the drawn rectangle to everything that differs from the reference color.
function contentBox(rc, ref) {
  const thr = tolThr(), x2 = rc.x + rc.w - 1, y2 = rc.y + rc.h - 1;
  const rowHas = y => { for (let x = rc.x; x <= x2; x++) if (differs(at(x, y), ref, thr)) return true; return false; };
  let top = rc.y; while (top <= y2 && !rowHas(top)) top++;
  if (top > y2) return null;
  let bot = y2; while (bot > top && !rowHas(bot)) bot--;
  const colHas = x => { for (let y = top; y <= bot; y++) if (differs(at(x, y), ref, thr)) return true; return false; };
  let left = rc.x; while (!colHas(left)) left++;
  let right = x2; while (right > left && !colHas(right)) right--;
  return { x: left, y: top, w: right - left + 1, h: bot - top + 1 };
}

function dragGeom() {
  const d = S.drag, mon = d.mon;
  const ex = clamp(S.cur.x, mon.x, mon.x + mon.w - 1), ey = clamp(S.cur.y, mon.y, mon.y + mon.h - 1);
  const drawn = { x: Math.min(d.x0, ex), y: Math.min(d.y0, ey), w: Math.abs(ex - d.x0) + 1, h: Math.abs(ey - d.y0) + 1 };
  const tiny = Math.abs(ex - d.x0) < 4 && Math.abs(ey - d.y0) < 4;
  const result = S.alt ? drawn : contentBox(drawn, at(d.x0, d.y0));
  return { mon, drawn, tiny, result, alt: S.alt };
}

// ---------------------------------------------------------------- numbers
function f1(v) { const r = Math.round(v * 10) / 10, s = Number.isInteger(r) ? String(r) : r.toFixed(1); return lang() === 'pt' ? s.replace('.', ',') : s; }
function f1copy(v) { const r = Math.round(v * 10) / 10; return Number.isInteger(r) ? String(r) : r.toFixed(1); }
const vals = m => m.mode === 'H' ? [m.w] : m.mode === 'V' ? [m.h] : [m.w, m.h];
const showLog = mon => S.settings.showLogical && mon.scale !== 1;
const physText = m => vals(m).join(' × ');
const logText = m => vals(m).map(v => f1(v / m.mon.scale)).join(' × ');
const copyText = (m, logical) => vals(m).map(v => logical ? f1copy(v / m.mon.scale) : String(v)).join(' x ');
const pxColor = (x, y) => { const i = at(x, y); return [IMG[i], IMG[i + 1], IMG[i + 2]]; };
const hex = c => '#' + c.map(v => v.toString(16).padStart(2, '0')).join('').toUpperCase();
const rgb = c => c.join(', ');

// ---------------------------------------------------------------- icons
const sv = (w, d, sw = 2) => `<svg width="${w}" height="${w}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="round">${d}</svg>`;
const IC = {
  dim: sv(20, '<rect x="2.5" y="8" width="19" height="8" rx="1.5"></rect><path d="M6.5 8v3M10.5 8v4.5M14.5 8v3M18.5 8v4.5"></path>'),
  bounds: sv(20, '<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"></path><rect x="9" y="9" width="6" height="6" rx="1"></rect>'),
  inspect: sv(20, '<circle cx="11" cy="11" r="6"></circle><path d="M15.5 15.5L20 20"></path>'),
  snip: sv(20, '<path d="M4 8h3l2-3h6l2 3h3v11H4z"></path><circle cx="12" cy="13" r="3.5"></circle>'),
  H: sv(16, '<path d="M5 12h14M8 8l-4 4 4 4M16 8l4 4-4 4"></path>', 2.2),
  V: sv(16, '<path d="M12 5v14M8 8l4-4 4 4M8 16l4 4 4-4"></path>', 2.2),
  C: sv(16, '<path d="M12 3v18M3 12h18M9 6l3-3 3 3M9 18l3 3 3-3M6 9l-3 3 3 3M18 9l3 3-3 3"></path>', 2.2),
  gear: sv(16, '<circle cx="12" cy="12" r="3"></circle><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"></path>', 2.1),
  min: sv(15, '<path d="M15 6l-6 6 6 6"></path>', 2.6),
  close: sv(14, '<path d="M6 6l12 12M18 6L6 18"></path>', 2.8),
  cam: sv(12, '<path d="M4 8h3l2-3h6l2 3h3v11H4z"></path><circle cx="12" cy="13" r="3.5"></circle>', 2.4),
  ok: sv(13, '<path d="M5 12.5l4.5 4.5L19 7.5"></path>', 3),
  bad: sv(13, '<path d="M12 7v6M12 17h.01"></path><circle cx="12" cy="12" r="9.5"></circle>', 2.4),
};

// ---------------------------------------------------------------- toolbar
const TOOLS = ['dim', 'bounds', 'inspect', 'snip'];
function renderToolbar() {
  const v = V();
  const modeBtns = ['H', 'V', 'C'].map(k => `<button class="mbtn ${S.mode === k ? 'on' : ''}" data-mode="${k}" title="${t(k)} (${k})">${IC[k]}</button>`).join('');
  const tools = TOOLS.map((k, i) => `<button class="tbtn ${S.tool === k ? 'on' : ''}" data-tool="${k}" title="${t(k)} (${i + 1})">${IC[k]}</button>`).join('');
  const tab = v.modes === 'tab' && S.tool === 'dim' ? `<div class="grp modes tab">${modeBtns}</div>` : '';
  const slot = v.modes === 'slot' ? `<div class="grp modes ${S.tool === 'dim' ? '' : 'hidden'}">${modeBtns}</div>` : '';
  tb.innerHTML = `<div class="grp tools">${tools}${tab}</div>${slot}
    <div class="grp small"><button class="sbtn" data-act="settings" title="${t('settings')}">${IC.gear}</button><button class="sbtn" data-act="min" title="${t('minimize')}">${IC.min}</button><button class="sbtn close" data-act="close" title="${t('close')}">${IC.close}</button></div>`;
  mini.innerHTML = `${IC[S.tool]}<span class="chev">${sv(10, '<path d="M9 6l6 6-6 6"></path>', 3)}</span>`;
  mini.title = t('expand');
  tb.hidden = !S.open || S.tbMin;
  mini.hidden = !S.open || !S.tbMin;
  if (S.tbPos) {
    for (const el of [tb, mini]) { el.style.left = S.tbPos.x + 'px'; el.style.top = S.tbPos.y + 'px'; }
  }
}
function placeToolbar(mon) {
  renderToolbar();
  const wasHidden = tb.hidden; tb.hidden = false;
  const w = tb.offsetWidth, h = tb.offsetHeight; tb.hidden = wasHidden;
  const mx = mon.x / dpr, my = mon.y / dpr, mw = mon.w / dpr, mh = mon.h / dpr, m = 14;
  const x = mx + m + S.tbRel.fx * Math.max(0, mw - w - 2 * m), y = my + m + S.tbRel.fy * Math.max(0, mh - h - 2 * m);
  S.tbPos = { x: clamp(x, mx, mx + mw - w), y: clamp(y, my, my + mh - h) };
  renderToolbar();
}
tb.addEventListener('click', e => {
  const b = e.target.closest('button'); if (!b) return;
  if (b.dataset.tool) setTool(b.dataset.tool);
  else if (b.dataset.mode) { S.mode = b.dataset.mode; renderToolbar(); render(); }
  else if (b.dataset.act === 'close') closeAll();
  else if (b.dataset.act === 'min') { S.tbMin = true; renderToolbar(); render(); }
  else if (b.dataset.act === 'settings') { closeAll(); openSettings(); }
});
mini.addEventListener('click', () => { S.tbMin = false; renderToolbar(); render(); });
tb.addEventListener('pointerdown', e => {
  if (e.target.closest('button') || e.button !== 0) return;
  S.tbDrag = { dx: e.clientX - S.tbPos.x, dy: e.clientY - S.tbPos.y };
  tb.classList.add('dragging'); tb.setPointerCapture(e.pointerId); e.stopPropagation();
});
tb.addEventListener('pointermove', e => {
  if (!S.tbDrag) return;
  const r = screens.getBoundingClientRect();
  S.tbPos = { x: clamp(e.clientX - S.tbDrag.dx, 0, r.width - tb.offsetWidth), y: clamp(e.clientY - S.tbDrag.dy, 0, r.height - tb.offsetHeight) };
  renderToolbar();
});
tb.addEventListener('pointerup', () => {
  if (!S.tbDrag) return;
  S.tbDrag = null; tb.classList.remove('dragging');
  // Remember the place inside the monitor the toolbar ended up on.
  const cx = (S.tbPos.x + tb.offsetWidth / 2) * dpr, cy = (S.tbPos.y + tb.offsetHeight / 2) * dpr;
  const mon = monAt(cx, cy) || MONS[0], m = 14, w = tb.offsetWidth, h = tb.offsetHeight;
  const fx = (S.tbPos.x - mon.x / dpr - m) / Math.max(1, mon.w / dpr - w - 2 * m), fy = (S.tbPos.y - mon.y / dpr - m) / Math.max(1, mon.h / dpr - h - 2 * m);
  S.tbRel = { monId: mon.id, fx: clamp(fx, 0, 1), fy: clamp(fy, 0, 1) };
});

function setTool(k) { S.tool = k; S.drag = null; renderToolbar(); render(); }
function closeAll() { S.open = false; S.pinned = []; S.drag = null; S.toast = null; S.tbMin = false; S.saveAs = null; saveEl.hidden = true; renderToolbar(); render(); }
function callPixmeter() {
  S.open = true; S.tol = S.settings.tolDefault; S.tbMin = false; S.msg = '';
  placeToolbar(S.cur.mon || MONS[0]);
  render();
}

// ---------------------------------------------------------------- input on the screens
function setCursor(e) {
  const r = screens.getBoundingClientRect();
  S.cur.x = Math.floor((e.clientX - r.left) * dpr); S.cur.y = Math.floor((e.clientY - r.top) * dpr);
  S.cur.mon = monAt(S.cur.x, S.cur.y);
}
screens.addEventListener('pointermove', e => {
  if (S.tbDrag) return;
  setCursor(e);
  S.overTb = !!e.target.closest('#toolbar, #tbMini, #saveas, #settings');
  render();
});
screens.addEventListener('pointerleave', () => { S.cur.mon = null; render(); });
screens.addEventListener('pointerdown', e => {
  if (!S.open || e.button !== 0 || e.target.closest('#toolbar, #tbMini, #saveas, #settings') || S.saveAs) return;
  setCursor(e); S.alt = e.altKey;
  if ((S.tool === 'bounds' || S.tool === 'snip') && S.cur.mon) {
    S.drag = { x0: S.cur.x, y0: S.cur.y, mon: S.cur.mon };
    screens.setPointerCapture(e.pointerId);
  }
  render();
});
screens.addEventListener('pointerup', e => {
  if (!S.open || e.button !== 0 || S.saveAs) return;
  if (S.drag) { finishDrag(e.shiftKey); return; }
  if (e.target.closest('#toolbar, #tbMini, #saveas, #settings') || !S.cur.mon) return;
  if (S.tool === 'dim') {
    const m = measureDim(S.cur.x, S.cur.y, S.cur.mon, S.mode);
    S.pinned.push(m); copy(copyText(m, e.shiftKey)); toast(t('copied'));
  } else if (S.tool === 'inspect') {
    const c = pxColor(S.cur.x, S.cur.y), f = S.settings.colorFmt;
    const first = f === 'rgb' ? rgb(c) : hex(c), other = f === 'rgb' ? hex(c) : rgb(c);
    copy(e.shiftKey ? other : first); toast(t('copied'));
  }
  render();
});
screens.addEventListener('wheel', e => {
  e.preventDefault();
  if (!S.open || e.target.closest('#toolbar, #tbMini, #saveas, #settings') || S.saveAs) return;
  const up = e.deltaY < 0;
  if (S.tool === 'inspect') {
    const i = clamp(MAGS.indexOf(S.mag) + (up ? 1 : -1), 0, MAGS.length - 1);
    S.mag = MAGS[i]; S.magAt = Date.now();
  } else {
    S.tol = clamp(S.tol + (up ? 1 : -1), 0, TOL_MAX); S.tolAt = Date.now();
  }
  setTimeout(render, 1050);
  render();
}, { passive: false });
screens.addEventListener('contextmenu', e => e.preventDefault());

function finishDrag(shift) {
  const g = dragGeom(); S.drag = null;
  if (g.tiny) { render(); return; }
  if (!g.result) { toast(t('noContent'), null, true); render(); return; }
  if (S.tool === 'bounds') {
    const p = { kind: 'bounds', mode: 'R', mon: g.mon, ...g.result };
    S.pinned.push(p); copy(copyText(p, shift)); toast(t('copied'));
  } else snip(g.result);
  render();
}

// Snip destinations, as decided in WPC-273 / WPC-278.
function snip(rc) {
  showThumb(rc);
  const st = S.settings, toClip = st.snipDest !== 'file', toFile = st.snipDest !== 'clip';
  const img = `${t('image')} ${rc.w} × ${rc.h}`;
  if (toClip) copy(img);
  if (!toFile) { toast(t('copied')); return; }
  const done = saved => {
    if (!saved) { if (toClip) toast(t('copied')); return; }
    if (S.scen.saveFail) { toast(t('cantSave'), toClip ? t('imageCopied') : null, true); return; }
    toast(toClip ? t('copiedSaved') : t('saved'));
  };
  if (st.folderOn) done(true); else openSaveAs(done);
}
function stamp() { const d = new Date(), p = n => String(n).padStart(2, '0'); return `pixmeter-${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}.png`; }
function openSaveAs(done) {
  S.saveAs = done; saveEl.hidden = false;
  saveEl.innerHTML = `<div class="dlg"><div class="tt"><b>Salvar como</b><span>janela do Windows, simulada</span></div>
    <div class="bd"><label>Nome do arquivo</label><input id="saName" value="${stamp()}"><div style="color:#666;font-size:12px">Pasta: Imagens</div></div>
    <div class="ac"><button class="pri" id="saOk">Salvar</button><button id="saNo">Cancelar</button></div></div>`;
  $('#saOk').onclick = () => closeSaveAs(true);
  $('#saNo').onclick = () => closeSaveAs(false);
  render();
}
function closeSaveAs(ok) { const done = S.saveAs; S.saveAs = null; saveEl.hidden = true; if (done) done(ok); render(); }
function showThumb(rc) {
  const c = $('#pThumb'); c.width = rc.w; c.height = rc.h;
  c.getContext('2d').drawImage(desk, rc.x, rc.y, rc.w, rc.h, 0, 0, rc.w, rc.h);
  c.style.display = 'block'; c.style.width = Math.min(120, 30 * rc.w / rc.h) + 'px';
  c.title = 'A imagem que o Snip copiou (sem o Pixmeter e sem o ponteiro)';
}
function copy(text) { S.clip = text; }
function toast(text, sub, bad) {
  S.toast = { text, sub, bad, at: Date.now(), x: S.cur.x, y: S.cur.y, mon: S.cur.mon || (S.drag && S.drag.mon) || MONS[0] };
  setTimeout(render, 1400);
}

window.addEventListener('keydown', e => {
  if (settingsKey(e)) return; // the shortcut recorder takes every key while it records
  if (e.target.closest && e.target.closest('input, textarea, [contenteditable]')) {
    if (e.key === 'Escape' && S.saveAs) closeSaveAs(false);
    if (e.key === 'Enter' && S.saveAs) closeSaveAs(true);
    return;
  }
  if (e.key === 'Alt') { e.preventDefault(); if (!S.alt) { S.alt = true; render(); } return; }
  if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') { e.preventDefault(); cycleVariant(e.key === 'ArrowRight' ? 1 : -1); return; }
  if (S.saveAs) { if (e.key === 'Escape') closeSaveAs(false); return; }
  if (!S.open) { if (e.key === 'm' || e.key === 'M') callPixmeter(); return; }
  const k = e.key.toLowerCase();
  if (e.key === 'Escape') { if (S.drag) { S.drag = null; render(); } else closeAll(); }
  else if (['1', '2', '3', '4'].includes(e.key)) setTool(TOOLS[+e.key - 1]);
  else if (S.tool === 'dim' && ['h', 'v', 'c'].includes(k)) { S.mode = k.toUpperCase(); renderToolbar(); render(); }
  else if (e.key === 'Backspace' || e.key === 'Delete') { e.preventDefault(); S.pinned.pop(); render(); }
});
window.addEventListener('keyup', e => { if (e.key === 'Alt') { e.preventDefault(); S.alt = false; render(); } });
window.addEventListener('blur', () => { S.alt = false; render(); });

// ---------------------------------------------------------------- drawing
let raf = 0;
function render() { if (!raf) raf = requestAnimationFrame(draw); }
const live = () => S.open && S.cur.mon && !S.overTb && !S.saveAs && !S.tbDrag;

function draw() {
  raf = 0;
  const isDark = S.settings.theme === 'dark' || (S.settings.theme === 'auto' && matchMedia('(prefers-color-scheme: dark)').matches);
  pm.className = isDark ? 'pm-dark' : 'pm-light';
  screens.classList.toggle('tools-on', S.open && !S.overTb);
  // The frozen screen covers everything while the tools are open, Settings included.
  $('#settings').style.pointerEvents = S.open ? 'none' : '';
  octx.clearRect(0, 0, ovl.width, ovl.height);
  const L = [], F = [];
  if (S.open) {
    for (const p of S.pinned) p.kind === 'dim' ? drawDim(p, false, L) : drawResult(p, p.mon, false, 'bounds', L);
    if (S.drag) {
      const g = dragGeom();
      if (!g.alt) dashedRect(g.drawn);
      if (g.result && !g.tiny) drawResult(g.result, g.mon, true, S.tool, L);
      if (S.variant === 'B') F.push(cardB(g));
    } else if (live()) {
      if (S.tool === 'dim') { const m = measureDim(S.cur.x, S.cur.y, S.cur.mon, S.mode); drawDim(m, true, L); if (S.variant === 'B') F.push(cardB(null, m)); }
      else if ((S.tool === 'bounds' || S.tool === 'snip') && S.variant === 'B') F.push(cardB(null));
    }
    F.push(indicators());
    if (S.variant !== 'B' || !(live() || S.drag)) F.push(toastHTML());
  }
  labelsEl.innerHTML = L.join('');
  fxEl.innerHTML = F.join('');
  positionFloating();
  drawMag(isDark);
  updateBar();
}

// --- lines on the overlay canvas, in physical pixels
function fill(c) { octx.fillStyle = c; }
function hseg(a, b, y, v) { if (b < a) return; if (v.halo) { fill(v.halo); octx.fillRect(a, y - 1, b - a + 1, 1); octx.fillRect(a, y + 1, b - a + 1, 1); } fill(v.color); octx.fillRect(a, y, b - a + 1, 1); }
function vseg(x, a, b, v) { if (b < a) return; if (v.halo) { fill(v.halo); octx.fillRect(x - 1, a, 1, b - a + 1); octx.fillRect(x + 1, a, 1, b - a + 1); } fill(v.color); octx.fillRect(x, a, 1, b - a + 1); }
function arrow(x, y, dx, dy) {
  const ah = Math.round(6 * dpr), aw = Math.round(3 * dpr), cx = x + .5, cy = y + .5;
  octx.beginPath(); octx.moveTo(cx, cy);
  if (dx) { octx.lineTo(cx + dx * ah, cy - aw); octx.lineTo(cx + dx * ah, cy + aw); }
  else { octx.lineTo(cx - aw, cy + dy * ah); octx.lineTo(cx + aw, cy + dy * ah); }
  octx.closePath(); fill(V().color); octx.fill();
}
// A span along a row (horiz) or column, styled by the variant. gap = [from, to] leaves room for C's number.
function span(horiz, a, b, fixed, gap) {
  const v = V(), seg = (p, q) => horiz ? hseg(p, q, fixed, v) : vseg(fixed, p, q, v);
  if (gap) { seg(a, Math.floor(gap[0])); seg(Math.ceil(gap[1]), b); } else seg(a, b);
  const k = Math.round((v.key === 'C' ? 7 : 4) * dpr), cross = (p) => horiz ? vseg(p, fixed - k, fixed + k, v) : hseg(fixed - k, fixed + k, p, v);
  if (v.key === 'A') { cross(a); cross(b); }
  if (v.key === 'C') {
    cross(a); cross(b);
    if (b - a > 16 * dpr) { horiz ? (arrow(a, fixed, 1, 0), arrow(b, fixed, -1, 0)) : (arrow(fixed, a, 0, 1), arrow(fixed, b, 0, -1)); }
  }
}
function dashedRect(r) {
  octx.save(); octx.lineWidth = 1;
  octx.strokeStyle = 'rgba(0,0,0,.85)'; octx.strokeRect(r.x + .5, r.y + .5, r.w - 1, r.h - 1);
  octx.setLineDash([4 * dpr, 4 * dpr]); octx.strokeStyle = '#ffffff'; octx.strokeRect(r.x + .5, r.y + .5, r.w - 1, r.h - 1);
  octx.restore();
}

// --- labels (HTML, positioned in CSS px)
const lbl = (x, y, tf, cls, html, style = '') => `<div class="lbl ${cls}" style="left:${x / dpr}px;top:${y / dpr}px;transform:${tf};${style}">${html}</div>`;
function numHTML(m, kind) {
  const p = physText(m), l = showLog(m.mon) ? logText(m) : null;
  if (S.variant === 'A') return p + (l ? `<span class="dv"></span><span class="dm">${l}</span>` : '');
  if (S.variant === 'B') return `<div>${p}</div>` + (l ? `<div class="dm">${l}</div>` : '');
  return p + (l ? ` <span class="dm">(${l})</span>` : '');
}
function textWidth(s) { octx.font = `600 ${12 * dpr}px "Segoe UI", system-ui, sans-serif`; return octx.measureText(s).width; }

function drawDim(m, isLive, L) {
  const v = V(), mon = m.mon, hasH = m.mode !== 'V', hasV = m.mode !== 'H';
  const midX = hasH ? (m.x1 + m.x2 + 1) / 2 : m.cx, midY = hasV ? (m.y1 + m.y2 + 1) / 2 : m.cy;
  let gap = null;
  if (v.key === 'C' && m.mode !== 'C') {
    const plain = physText(m) + (showLog(mon) ? ` (${logText(m)})` : '');
    const need = (hasH ? textWidth(plain) + 14 * dpr : 20 * dpr);
    const len = hasH ? m.w : m.h, mid = hasH ? midX : midY;
    if (len > need + 34 * dpr) gap = [mid - need / 2, mid + need / 2];
  }
  if (hasH) span(true, m.x1, m.x2, m.cy, m.mode === 'H' ? gap : null);
  if (hasV) span(false, m.y1, m.y2, m.cx, m.mode === 'V' ? gap : null);
  const html = numHTML(m), nearTop = m.cy - mon.y < 40 * dpr, nearRight = mon.x + mon.w - m.cx < 120 * dpr;
  if (v.key === 'A') {
    const cls = isLive ? 'pill live' : 'pill pin', st = isLive ? `background:${v.color}` : `border-left:3px solid ${v.color}`;
    if (m.mode === 'H') L.push(lbl(midX, m.cy, nearTop ? 'translate(-50%, 9px)' : 'translate(-50%, calc(-100% - 9px))', cls, html, st));
    else if (m.mode === 'V') L.push(lbl(m.cx, midY, nearRight ? 'translate(calc(-100% - 10px), -50%)' : 'translate(10px, -50%)', cls, html, st));
    else L.push(lbl(m.cx, m.cy, 'translate(12px, 12px)', cls, html, st));
  } else if (v.key === 'B') {
    if (isLive) return;
    const st = `border-left:3px solid ${v.color}`;
    if (m.mode === 'H') L.push(lbl(m.x2, m.cy, 'translate(7px, -50%)', 'tag', html, st));
    else if (m.mode === 'V') L.push(lbl(m.cx, m.y2, 'translate(-50%, 7px)', 'tag', html, st));
    else L.push(lbl(m.cx, m.cy, 'translate(9px, 9px)', 'tag', html, st));
  } else {
    const cls = isLive ? 'cad live' : 'cad pin', st = isLive ? `color:${v.color}` : `border-color:${v.color}`;
    if (m.mode === 'C') L.push(lbl(m.cx, m.cy, 'translate(12px, 12px)', cls, html, st));
    else if (gap) L.push(lbl(hasH ? midX : m.cx, hasH ? m.cy : midY, 'translate(-50%, -50%)', cls, html, st));
    else if (hasH) L.push(lbl(midX, m.cy, nearTop ? 'translate(-50%, 10px)' : 'translate(-50%, calc(-100% - 10px))', cls, html, st));
    else L.push(lbl(m.cx, midY, nearRight ? 'translate(calc(-100% - 12px), -50%)' : 'translate(12px, -50%)', cls, html, st));
  }
}

// A Bounds result (live or pinned) or a Snip preview.
function drawResult(r, mon, isLive, kind, L) {
  const v = V(), m = { mode: 'R', mon, w: r.w, h: r.h };
  octx.save(); octx.lineWidth = 1; octx.strokeStyle = v.color;
  if (kind === 'bounds') {
    fill(v.color); octx.globalAlpha = isLive ? .16 : .1; octx.fillRect(r.x, r.y, r.w, r.h); octx.globalAlpha = 1;
    if (v.halo) { octx.strokeStyle = v.halo; octx.strokeRect(r.x - 1.5, r.y - 1.5, r.w + 3, r.h + 3); octx.strokeStyle = v.color; }
    octx.strokeRect(r.x - .5, r.y - .5, r.w + 1, r.h + 1);
  } else if (v.key === 'A') {
    const arm = Math.max(3, Math.min(Math.round(14 * dpr), Math.floor(r.w / 2), Math.floor(r.h / 2))), x0 = r.x - 2, y0 = r.y - 2, x1 = r.x + r.w, y1 = r.y + r.h;
    fill(v.color);
    for (const [cx, cy, sx, sy] of [[x0, y0, 1, 1], [x1, y0, -1, 1], [x0, y1, 1, -1], [x1, y1, -1, -1]]) {
      octx.fillRect(sx > 0 ? cx : cx - arm + 2, cy, arm, 2); octx.fillRect(cx, sy > 0 ? cy : cy - arm + 2, 2, arm);
    }
  } else if (v.key === 'B') {
    octx.strokeRect(r.x - .5, r.y - .5, r.w + 1, r.h + 1);
  } else {
    octx.setLineDash([6 * dpr, 4 * dpr]); octx.strokeRect(r.x - .5, r.y - .5, r.w + 1, r.h + 1);
  }
  octx.restore();
  const nearTop = r.y - mon.y < 34 * dpr;
  if (kind === 'snip') {
    const html = `${r.w} × ${r.h}`;
    if (v.key === 'A') L.push(lbl(r.x - 2, r.y - 2, nearTop ? 'translate(4px, 6px)' : 'translate(0, calc(-100% - 6px))', 'pill live', `<span style="display:inline-flex;gap:5px;align-items:center">${IC.cam}${html}</span>`, `background:${v.color}`));
    if (v.key === 'C') L.push(lbl(r.x + r.w, r.y + r.h, 'translate(-100%, 8px)', 'cad pin', `${html} px`, `border-color:${v.color}`));
    return;
  }
  if (v.key === 'A') {
    const cls = isLive ? 'pill live' : 'pill pin', st = isLive ? `background:${v.color}` : `border-left:3px solid ${v.color}`;
    L.push(lbl(r.x - 1, r.y - 1, nearTop ? 'translate(4px, 5px)' : 'translate(0, calc(-100% - 6px))', cls, numHTML(m), st));
  } else if (v.key === 'B') {
    if (!isLive) L.push(lbl(r.x + r.w, r.y + r.h, 'translate(7px, 7px)', 'tag', numHTML(m), `border-left:3px solid ${v.color}`));
  } else {
    // Technical-drawing dimensions: width above (or below) and height to the right (or left).
    const off = Math.round(12 * dpr), cls = isLive ? 'cad live' : 'cad pin', st = isLive ? `color:${v.color}` : `border-color:${v.color}`;
    const lg = showLog(mon), wTxt = `${r.w}` + (lg ? ` <span class="dm">(${f1(r.w / mon.scale)})</span>` : ''), hTxt = `${r.h}` + (lg ? ` <span class="dm">(${f1(r.h / mon.scale)})</span>` : '');
    const above = r.y - off - 4 * dpr > mon.y, yy = above ? r.y - 1 - off : r.y + r.h + off;
    const right = r.x + r.w + off + 70 * dpr < mon.x + mon.w, xx = right ? r.x + r.w + off : r.x - 1 - off;
    const ext = Math.round(3 * dpr);
    vseg(r.x, Math.min(yy - ext, r.y - 1), Math.max(yy + ext, r.y - 1), { color: v.color, halo: null });
    vseg(r.x + r.w - 1, Math.min(yy - ext, r.y - 1), Math.max(yy + ext, r.y - 1), { color: v.color, halo: null });
    if (!above) { vseg(r.x, r.y + r.h, yy + ext, { color: v.color, halo: null }); vseg(r.x + r.w - 1, r.y + r.h, yy + ext, { color: v.color, halo: null }); }
    const gw = textWidth(`${r.w}` + (lg ? ` (${f1(r.w / mon.scale)})` : '')) + 14 * dpr, midX = r.x + r.w / 2;
    span(true, r.x, r.x + r.w - 1, yy, r.w > gw + 34 * dpr ? [midX - gw / 2, midX + gw / 2] : null);
    const gh = 20 * dpr, midY = r.y + r.h / 2;
    span(false, r.y, r.y + r.h - 1, xx, r.h > gh + 34 * dpr ? [midY - gh / 2, midY + gh / 2] : null);
    hseg(Math.min(xx - ext, r.x + r.w), Math.max(xx + ext, r.x + r.w), r.y, { color: v.color, halo: null });
    hseg(Math.min(xx - ext, r.x + r.w), Math.max(xx + ext, r.x + r.w), r.y + r.h - 1, { color: v.color, halo: null });
    L.push(lbl(midX, yy, r.w > gw + 34 * dpr ? 'translate(-50%, -50%)' : `translate(-50%, ${above ? 'calc(-100% - 8px)' : '8px'})`, cls, wTxt, st));
    L.push(lbl(xx, midY, r.h > gh + 34 * dpr ? 'translate(-50%, -50%)' : `translate(${right ? '8px' : 'calc(-100% - 8px)'}, -50%)`, cls, hTxt, st));
  }
}

// --- variant B: the card that follows the cursor
function cardB(g, m) {
  const tol = `<div class="ft ${Date.now() - S.tolAt < 1000 ? 'hot' : ''}">${t('tol')} ${S.tol} ${meter()}</div>`;
  const tst = S.toast && Date.now() - S.toast.at < 1300 ? `<div class="tst ${S.toast.bad ? 'bad' : ''}">${S.toast.text}${S.toast.sub ? '<br>' + S.toast.sub : ''}</div>` : '';
  let hd, body;
  if (S.tool === 'dim') {
    hd = `${IC[S.mode].replace(/width="16" height="16"/, 'width="12" height="12"')} ${t('dim')} · ${t(S.mode)}`;
    body = rowsB(m);
  } else {
    hd = `${S.tool === 'snip' ? IC.cam : IC.bounds.replace(/width="20" height="20"/, 'width="12" height="12"')} ${t(S.tool)}`;
    if (!g) body = `<div class="sub">${t(S.tool === 'snip' ? 'dragSnip' : 'drag')}</div>`;
    else if (!g.result || g.tiny) body = `<div class="sub">${g.tiny ? '—' : t('noContent')}</div>`;
    else body = S.tool === 'snip' ? `<div class="big">${g.result.w} × ${g.result.h}<small>px</small></div><div class="sub">${t('image')} PNG</div>` : rowsB({ mode: 'R', mon: g.mon, w: g.result.w, h: g.result.h });
  }
  return `<div class="card panel" data-follow="1"><div class="hd">${hd}</div>${body}${tst}${tol}</div>`;
}
function rowsB(m) {
  return `<div class="big"><small style="margin:0 6px 0 0">${t('phys')}</small>${physText(m)}</div>` + (showLog(m.mon) ? `<div class="sub">${t('logi')} <b>${logText(m)}</b></div>` : '');
}
function meter() { const n = Math.round(S.tol / TOL_MAX * 10); return `<span class="meter">${Array.from({ length: 10 }, (_, i) => `<i class="${i < n ? 'f' : ''}"></i>`).join('')}</span>`; }

// --- indicators and notices for A and C
function indicators() {
  if (!S.open || S.variant === 'B' || S.tool === 'inspect') return '';
  if (Date.now() - S.tolAt > 1000) return '';
  const html = `${t('tol')} <b>${S.tol}</b> ${meter()}`;
  if (S.variant === 'A') return `<div class="ind panel" data-at="cursor-tl">${html}</div>`;
  return `<div class="ind panel" data-at="bottom" style="padding:8px 12px">${html}</div>`;
}
function toastHTML() {
  const ts = S.toast;
  if (!ts || Date.now() - ts.at > 1300) return '';
  const ic = ts.bad ? `<span class="bad">${IC.bad}</span>` : `<span class="ok">${IC.ok}</span>`;
  const where = S.variant === 'C' ? 'bottom-up' : 'toast';
  return `<div class="toast panel" data-at="${where}">${ic}<span>${ts.text}${ts.sub ? `<small>${ts.sub}</small>` : ''}</span></div>`;
}

// Place cursor-following elements after they exist, so their real size is known.
function positionFloating() {
  const mon = S.cur.mon || (S.drag && S.drag.mon) || (S.toast && S.toast.mon) || MONS[0];
  const mx = mon.x / dpr, my = mon.y / dpr, mw = mon.w / dpr, mh = mon.h / dpr;
  const cx = S.cur.x / dpr, cy = S.cur.y / dpr;
  for (const el of fxEl.children) {
    const w = el.offsetWidth, h = el.offsetHeight;
    let x, y;
    if (el.dataset.follow) {
      x = cx + 20; y = cy + 20;
      if (x + w > mx + mw - 4) x = cx - 20 - w;
      if (y + h > my + mh - 4) y = cy - 20 - h;
    } else if (el.dataset.at === 'cursor-tl') { x = cx - 14 - w; y = cy - 14 - h; }
    else if (el.dataset.at === 'toast') {
      const tx = S.toast ? S.toast.x / dpr : cx, ty = S.toast ? S.toast.y / dpr : cy;
      x = tx + 14; y = ty - 14 - h;
      if (x + w > mx + mw - 4) x = tx - 14 - w;
      if (y < my + 4) y = ty + 18;
    } else if (el.dataset.at === 'bottom') { x = mx + mw / 2 - w / 2; y = my + mh - 70 - h; }
    else if (el.dataset.at === 'bottom-up') { x = mx + mw / 2 - w / 2; y = my + mh - 70 - h - (fxEl.querySelector('[data-at="bottom"]') ? 44 : 0); }
    el.style.left = clamp(x, mx + 2, mx + mw - w - 2) + 'px';
    el.style.top = clamp(y, my + 2, my + mh - h - 2) + 'px';
  }
}

// --- Inspect's magnified view
let magKey = '';
function drawMag(isDark) {
  const show = S.open && S.tool === 'inspect' && live();
  magEl.hidden = !show;
  if (!show) return;
  const v = V(), mon = S.cur.mon, mag = S.mag;
  if (magKey !== v.key) {
    magKey = v.key;
    if (v.key === 'A') magEl.innerHTML = `<div class="panel" style="border-radius:14px;padding:6px"><div class="magframe" style="border-radius:9px"><canvas></canvas><div class="magtag" hidden></div></div><div class="info" style="padding:7px 4px 2px"></div></div>`;
    else if (v.key === 'B') magEl.innerHTML = `<div style="display:flex;flex-direction:column;align-items:flex-start;gap:8px"><div class="magframe" style="border-radius:50%;border:3px solid var(--panel-solid);box-shadow:var(--panel-shadow)"><canvas></canvas></div><div class="card panel" style="position:relative"></div></div>`;
    else magEl.innerHTML = `<div class="cwrap" style="display:flex;gap:8px;align-items:flex-start"><div class="panel" style="padding:5px;border-radius:6px"><div class="magframe" style="border-radius:2px"><canvas></canvas></div></div><div class="info panel" style="border-radius:6px;padding:9px 11px"></div></div>`;
  }
  // Canvas: N source pixels across (odd, so the cursor's pixel is the exact center), each mag × mag physical pixels.
  let N = Math.floor(v.mag.css * dpr / mag); if (N % 2 === 0) N--; N = Math.max(N, 3);
  const size = N * mag, cv = magEl.querySelector('canvas'), c = cv.getContext('2d');
  if (cv.width !== size) { cv.width = size; cv.height = size; cv.style.width = size / dpr + 'px'; cv.style.height = size / dpr + 'px'; }
  c.imageSmoothingEnabled = false;
  c.fillStyle = isDark ? '#2a2c31' : '#d9d6d0'; c.fillRect(0, 0, size, size);
  const half = (N - 1) / 2, sx = S.cur.x - half, sy = S.cur.y - half;
  const ix0 = Math.max(sx, mon.x), iy0 = Math.max(sy, mon.y), ix1 = Math.min(sx + N, mon.x + mon.w), iy1 = Math.min(sy + N, mon.y + mon.h);
  if (ix1 > ix0 && iy1 > iy0) c.drawImage(desk, ix0, iy0, ix1 - ix0, iy1 - iy0, (ix0 - sx) * mag, (iy0 - sy) * mag, (ix1 - ix0) * mag, (iy1 - iy0) * mag);
  if (v.mag.grid && mag >= v.mag.grid && ix1 > ix0 && iy1 > iy0) {
    // Grid only over real pixels; the part past the monitor's edge stays empty.
    const gx0 = (ix0 - sx) * mag, gy0 = (iy0 - sy) * mag, gx1 = (ix1 - sx) * mag, gy1 = (iy1 - sy) * mag;
    c.fillStyle = 'rgba(0,0,0,.2)';
    for (let x = gx0 + mag; x < gx1; x += mag) c.fillRect(x, gy0, 1, gy1 - gy0);
    for (let y = gy0 + mag; y < gy1; y += mag) c.fillRect(gx0, y, gx1 - gx0, 1);
  }
  const c0 = half * mag;
  if (v.key === 'B') {
    c.fillStyle = 'rgba(255,45,149,.6)';
    c.fillRect(0, c0 + mag / 2, c0 - 2, 1); c.fillRect(c0 + mag + 2, c0 + mag / 2, size, 1);
    c.fillRect(c0 + mag / 2, 0, 1, c0 - 2); c.fillRect(c0 + mag / 2, c0 + mag + 2, 1, size);
  }
  c.lineWidth = 1;
  c.strokeStyle = '#000'; c.strokeRect(c0 - .5, c0 - .5, mag + 1, mag + 1);
  c.strokeStyle = '#fff'; c.strokeRect(c0 - 1.5, c0 - 1.5, mag + 3, mag + 3);
  // Info.
  const col = pxColor(S.cur.x, S.cur.y), px = S.cur.x - mon.x, py = S.cur.y - mon.y, lg = showLog(mon), f = S.settings.colorFmt;
  const hdr = S.scen.hdr2 && mon.id === 2 ? `<div class="hdr">${IC.bad}${t('approx')}</div>` : '';
  const sw = `<span class="sw" style="background:${hex(col)}"></span>`;
  const hexS = f !== 'rgb' ? `<span class="v">${hex(col)}</span>` : '', rgbS = f !== 'hex' ? `<span class="${f === 'rgb' ? 'v' : 'dm'}">${rgb(col)}</span>` : '';
  const recent = Date.now() - S.magAt < 1000;
  const ts = S.toast && Date.now() - S.toast.at < 1300 ? S.toast : null;
  if (v.key === 'A') {
    const pos = (k, n) => `<div class="row"><span class="k">${k}</span><span class="v">${n}</span>${lg ? `<span class="dv"></span><span class="dm">${f1(n / mon.scale)}</span>` : ''}</div>`;
    const info = magEl.querySelector('.info');
    info.style.width = size / dpr + 'px';
    info.innerHTML = `<div style="display:grid;grid-template-columns:1fr 1fr;gap:0 8px">${pos('X', px)}${pos('Y', py)}</div><div class="row" style="margin-top:5px;flex-wrap:wrap">${sw}${hexS}${rgbS}</div>${hdr ? `<div style="margin-top:4px">${hdr}</div>` : ''}`;
    const tag = magEl.querySelector('.magtag'); tag.hidden = !recent; tag.textContent = mag + '×';
  } else if (v.key === 'B') {
    const tst = ts ? `<div class="tst ${ts.bad ? 'bad' : ''}">${ts.text}</div>` : '';
    magEl.querySelector('.card').innerHTML = `<div class="hd">${IC.inspect.replace(/width="20" height="20"/, 'width="12" height="12"')} ${t('inspect')} · <span style="color:var(--panel-ink)">${mag}×</span></div>
      <div class="row" style="display:flex;gap:8px;align-items:center"><span class="sw" style="width:28px;height:28px;background:${hex(col)}"></span><div><div class="big" style="font-size:16px">${f === 'rgb' ? rgb(col) : hex(col)}</div>${f === 'both' ? `<div class="sub">${rgb(col)}</div>` : ''}</div></div>
      <div class="sub" style="margin-top:6px">X <b>${px}</b>&nbsp;&nbsp; Y <b>${py}</b>${lg ? `<br>${t('logi')}: X <b>${f1(px / mon.scale)}</b>&nbsp;&nbsp; Y <b>${f1(py / mon.scale)}</b>` : ''}</div>${hdr ? `<div style="margin-top:5px">${hdr}</div>` : ''}${tst}`;
  } else {
    const ladder = `<div style="display:flex;gap:3px;margin-bottom:8px">${MAGS.map(z => `<span style="padding:2px 4px;border-radius:4px;font-weight:700;font-size:10px;${z === mag ? `background:${v.color};color:#1a0d00` : 'color:var(--panel-dim)'}">${z}×</span>`).join('')}</div>`;
    const pos = (k, n) => `<div class="row"><span class="k">${k}</span><span class="v">${n}</span>${lg ? `<span class="dm">(${f1(n / mon.scale)})</span>` : ''}</div>`;
    magEl.querySelector('.info').innerHTML = `${ladder}${pos('X', px)}${pos('Y', py)}<div class="row" style="margin-top:6px">${sw}<div>${f !== 'rgb' ? `<div class="v">${hex(col)}</div>` : ''}${f !== 'hex' ? `<div class="${f === 'rgb' ? 'v' : 'dm'}">${rgb(col)}</div>` : ''}</div></div>${hdr ? `<div style="margin-top:6px">${hdr}</div>` : ''}`;
  }
  // Below and to the right of the cursor; flips near the edges; never leaves the current monitor.
  const w = magEl.offsetWidth, h = magEl.offsetHeight, mx = mon.x / dpr, my = mon.y / dpr, mw = mon.w / dpr, mh = mon.h / dpr, cx = S.cur.x / dpr, cy = S.cur.y / dpr;
  let x = cx + 22, y = cy + 22, flipX = false;
  if (x + w > mx + mw - 4) { x = cx - 22 - w; flipX = true; }
  if (y + h > my + mh - 4) y = cy - 22 - h;
  if (v.key === 'C') magEl.querySelector('.cwrap').style.flexDirection = flipX ? 'row-reverse' : 'row';
  magEl.style.left = clamp(x, mx + 2, mx + mw - w - 2) + 'px';
  magEl.style.top = clamp(y, my + 2, my + mh - h - 2) + 'px';
}

// ---------------------------------------------------------------- prototype bar and panels
const THEMES = { auto: 'Automático', light: 'Claro', dark: 'Escuro' };
function cycleVariant(d) {
  const ks = Object.keys(VARS); S.variant = ks[(ks.indexOf(S.variant) + d + ks.length) % ks.length];
  history.replaceState(null, '', '?variant=' + S.variant);
  magKey = ''; renderToolbar(); render();
  if (!$('#pHelpPanel').hidden) renderHelp();
}
function updateBar() {
  $('#pName').textContent = `${S.variant} · ${V().name}`;
  $('#pTheme').textContent = `Tema: ${THEMES[S.settings.theme]}`;
  $('#pCall').hidden = S.open;
  let st;
  if (!S.open) st = S.msg || 'Ferramentas fechadas. Aperte <b>M</b> (no lugar do atalho global) ou clique em Chamar.';
  else {
    st = `<b>${t(S.tool)}</b>${S.tool === 'dim' ? ' · ' + t(S.mode) : ''}`;
    st += S.tool === 'inspect' ? ` · ampliação ${S.mag}×` : ` · tolerância ${S.tol}${S.tol !== S.settings.tolDefault ? ` (padrão ${S.settings.tolDefault})` : ''}`;
    if (S.pinned.length) st += ` · ${S.pinned.length} fixada(s)`;
  }
  st += ` · área de transferência: <b>${S.clip}</b>`;
  $('#pState').innerHTML = st;
}
$('#pPrev').onclick = () => cycleVariant(-1);
$('#pNext').onclick = () => cycleVariant(1);
$('#pTheme').onclick = () => { const ks = Object.keys(THEMES); S.settings.theme = ks[(ks.indexOf(S.settings.theme) + 1) % 3]; render(); renderSettings(); };
$('#pCall').onclick = () => callPixmeter();
$('#pSettings').onclick = () => { if (S.open) closeAll(); openSettings(); };
$('#pOpts').onclick = () => { const p = $('#pOptsPanel'); p.hidden = !p.hidden; $('#pHelpPanel').hidden = true; if (!p.hidden) renderOpts(); };
$('#pHelp').onclick = () => { const p = $('#pHelpPanel'); p.hidden = !p.hidden; $('#pOptsPanel').hidden = true; if (!p.hidden) renderHelp(); };
matchMedia('(prefers-color-scheme: dark)').addEventListener('change', render);

function seg(path, opts) {
  const [obj, key] = path.split('.'), cur = S[obj][key];
  return `<span class="seg">${opts.map(([v, l]) => `<button data-set="${path}" data-val="${v}" class="${String(cur) === String(v) ? 'on' : ''}">${l}</button>`).join('')}</span>`;
}
function renderOpts() {
  const p = $('#pOptsPanel');
  p.innerHTML = `<h3>Cenários</h3>
  <div class="note" style="margin-bottom:8px">Situações que não acontecem sozinhas no navegador. As opções do Pixmeter ficam na janela de Configurações (engrenagem da barra, ou o botão "Configurações" aqui embaixo, que faz o papel do menu do ícone perto do relógio).</div>
  <div class="grid">
    <span>HDR ligado sem correção no Monitor 2</span>${seg('scen.hdr2', [[true, 'Sim'], [false, 'Não']])}
    <span>Falha ao gravar o arquivo do Snip</span>${seg('scen.saveFail', [[true, 'Sim'], [false, 'Não']])}
    <span>Atalho global ocupado ao iniciar</span>${seg('scen.shortcutBusy', [[true, 'Sim'], [false, 'Não']])}
    <span>O Windows desligou o "Iniciar com o Windows"</span>${seg('scen.startupBlocked', [[true, 'Sim'], [false, 'Não']])}
    <span>Cópia instalada</span>${seg('scen.copy', [['installer', 'Instalador (GitHub)'], ['store', 'Microsoft Store']])}
  </div>
  <div class="note" style="margin-top:8px">No gravador de atalho, <code>Ctrl + Shift + S</code> faz o papel de uma combinação já usada por outro programa. O navegador pode tomar para si algumas combinações (como <code>Ctrl + T</code>); no app de verdade isso não acontece.</div>`;
}
$('#pOptsPanel').addEventListener('click', e => {
  const b = e.target.closest('button'); if (!b) return;
  const [obj, key] = b.dataset.set.split('.'), raw = b.dataset.val;
  S[obj][key] = raw === 'true' ? true : raw === 'false' ? false : raw;
  // The busy-shortcut notification's click opens Settings with the row highlighted.
  if (b.dataset.set === 'scen.shortcutBusy' && S.scen.shortcutBusy) { closeAll(); openSettings(); }
  if (b.dataset.set === 'scen.startupBlocked' && S.scen.startupBlocked) S.settings.startWin = false;
  renderOpts(); renderToolbar(); render(); renderSettings();
});

function renderHelp() {
  const cols = Object.keys(VARS).map(k => `<div><h4 style="color:${VARS[k].color}">${k} · ${VARS[k].name}</h4><ol>${CHOICES[k].map((c, i) => `<li><b>${CHOICE_TOPICS[i]}:</b> ${c}</li>`).join('')}</ol></div>`).join('');
  $('#pHelpPanel').innerHTML = `<h3>Como usar</h3>
  <div class="grid">
    <span><kbd>1</kbd> <kbd>2</kbd> <kbd>3</kbd> <kbd>4</kbd></span><span>Dimensions, Bounds, Inspect, Snip (ou clique na barra)</span>
    <span><kbd>H</kbd> <kbd>V</kbd> <kbd>C</kbd></span><span>Modos do Dimensions: horizontal, vertical, cruz</span>
    <span>Roda do mouse</span><span>Tolerância (Dimensions, Bounds, Snip) ou ampliação (Inspect)</span>
    <span>Clique / <kbd>Shift</kbd> + clique</span><span>Fixa e copia o pixel real / o pixel de escala (Dimensions); copia a cor / o outro formato (Inspect)</span>
    <span>Arrastar / <kbd>Alt</kbd> + arrastar</span><span>Bounds e Snip: encolhe até o conteúdo / mede ou captura o retângulo desenhado</span>
    <span><kbd>Backspace</kbd></span><span>Apaga a última medida fixada</span>
    <span><kbd>Esc</kbd></span><span>Cancela o arraste; sem arraste, fecha tudo. Depois, <kbd>M</kbd> chama de novo</span>
    <span><kbd>←</kbd> <kbd>→</kbd></span><span>Troca a variante (no app de verdade, as setas movem o cursor 1 pixel; isso não está simulado)</span>
  </div>
  <div class="note" style="margin-top:8px">Simulação: cada ponto da tela falsa é um pixel real; o pixel de escala é o real dividido pela escala do monitor (150% no 1, 100% no 2). A barra, as etiquetas e a lupa aparecem no tamanho do seu monitor de verdade, nos dois monitores. A barra e as Configurações são iguais nas três variantes, menos o lugar dos botões H/V/C.</div>
  <h3 style="margin-top:12px">O que muda em cada variante</h3>
  <div class="note">Os itens têm o mesmo número nas três. Dá para misturar: "1 de C, 5 de A".</div>
  <div class="cols">${cols}</div>`;
}

window.addEventListener('resize', layout);
layout();
renderToolbar();
