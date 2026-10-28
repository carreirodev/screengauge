// PROTOTYPE — draws the fake "frozen screen": two monitors with ordinary app content to measure.
// One canvas pixel is one physical pixel. Content is laid out in logical units (u) and drawn at
// `scale` physical pixels per unit, the way Windows apps grow on a monitor set to 150%.

const SANS = '"Segoe UI", system-ui, sans-serif';
const MONO = '"Cascadia Mono", Consolas, ui-monospace, monospace';

function drawDesktop(ctx, mons) {
  ctx.fillStyle = '#0b0c0e';
  ctx.fillRect(0, 0, ctx.canvas.width, ctx.canvas.height);
  for (const m of mons) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(m.x, m.y, m.w, m.h);
    ctx.clip();
    if (m.id === 1) drawLightApp(ctx, m); else drawDarkApp(ctx, m);
    ctx.restore();
  }
}

// Helpers that map logical units to snapped physical pixels inside monitor m.
function pen(ctx, m) {
  const s = m.scale;
  const X = u => m.x + Math.round(u * s);
  const Y = u => m.y + Math.round(u * s);
  const p = {
    s, LW: m.w / s, LH: m.h / s,
    rect(x, y, w, h, c) { ctx.fillStyle = c; ctx.fillRect(X(x), Y(y), X(x + w) - X(x), Y(y + h) - Y(y)); },
    rrect(x, y, w, h, r, c, stroke) {
      ctx.beginPath();
      ctx.roundRect(X(x), Y(y), X(x + w) - X(x), Y(y + h) - Y(y), r * s);
      if (c) { ctx.fillStyle = c; ctx.fill(); }
      if (stroke) { ctx.strokeStyle = stroke; ctx.lineWidth = 1; ctx.beginPath(); ctx.roundRect(X(x) + .5, Y(y) + .5, X(x + w) - X(x) - 1, Y(y + h) - Y(y) - 1, r * s); ctx.stroke(); }
    },
    circle(cx, cy, r, c) { ctx.beginPath(); ctx.arc(m.x + cx * s, m.y + cy * s, r * s, 0, Math.PI * 2); ctx.fillStyle = c; ctx.fill(); },
    text(t, x, y, size, c, weight = 400, font = SANS, align = 'left') {
      ctx.font = `${weight} ${Math.round(size * s)}px ${font}`;
      ctx.fillStyle = c; ctx.textBaseline = 'alphabetic'; ctx.textAlign = align;
      ctx.fillText(t, X(x), Y(y));
      return ctx.measureText(t).width / s;
    },
    shadow(blur, oy, c, fn) { ctx.save(); ctx.shadowColor = c; ctx.shadowBlur = blur * s; ctx.shadowOffsetY = oy * s; fn(); ctx.restore(); },
    grad(x, y, w, h, stops, vertical = true) {
      const g = vertical ? ctx.createLinearGradient(0, Y(y), 0, Y(y + h)) : ctx.createLinearGradient(X(x), 0, X(x + w), 0);
      stops.forEach(([o, c]) => g.addColorStop(o, c));
      ctx.fillStyle = g; ctx.fillRect(X(x), Y(y), X(x + w) - X(x), Y(y + h) - Y(y));
    },
  };
  return p;
}

function taskbar(p, dark) {
  const h = 48, y = p.LH - h;
  p.rect(0, y, p.LW, h, dark ? '#1c1c1c' : '#eef0f3');
  p.rect(0, y, p.LW, 1, dark ? '#2e2e2e' : '#d3d7dd');
  const icons = ['#0a74da', '#f2b01e', '#2aa84a', '#d64545', '#7a5af8', '#0f9d9a'];
  const total = icons.length * 32 + (icons.length - 1) * 10;
  let x = p.LW / 2 - total / 2;
  icons.forEach((c, i) => {
    if (i === 0) { // Start button: four squares
      p.rect(x + 7, y + 15, 8, 8, '#0a74da'); p.rect(x + 17, y + 15, 8, 8, '#0a74da');
      p.rect(x + 7, y + 25, 8, 8, '#0a74da'); p.rect(x + 17, y + 25, 8, 8, '#0a74da');
    } else {
      p.rrect(x + 2, y + 10, 28, 28, 6, c);
    }
    x += 42;
  });
  const ink = dark ? '#ffffff' : '#1b1b1b';
  p.text('14:32', p.LW - 18, y + 21, 12, ink, 400, SANS, 'right');
  p.text('27/10/2026', p.LW - 18, y + 37, 12, ink, 400, SANS, 'right');
}

// Monitor 1 (150%): a light web dashboard in a browser window.
function drawLightApp(ctx, m) {
  const p = pen(ctx, m);
  p.grad(0, 0, p.LW, p.LH, [[0, '#4a7fb8'], [1, '#183a6b']]);
  p.circle(p.LW * .78, p.LH * .2, 160, 'rgba(255,255,255,.06)');
  taskbar(p, false);

  const wx = 18, wy = 16, ww = p.LW - 36, wh = p.LH - 48 - 30;
  p.shadow(24, 8, 'rgba(0,0,0,.45)', () => p.rrect(wx, wy, ww, wh, 8, '#ffffff'));
  ctx.save();
  ctx.beginPath(); ctx.roundRect(m.x + Math.round(wx * p.s), m.y + Math.round(wy * p.s), Math.round(ww * p.s), Math.round(wh * p.s), 8 * p.s); ctx.clip();
  // Tab strip and address bar.
  p.rect(wx, wy, ww, 34, '#e7eaf0');
  p.rrect(wx + 10, wy + 6, 190, 28, 7, '#ffffff');
  p.rect(wx + 10, wy + 26, 190, 8, '#ffffff');
  p.circle(wx + 26, wy + 20, 6, '#2563eb');
  p.text('Painel — Acme', wx + 38, wy + 24, 12, '#1f2937');
  p.text('—    ☐    ✕', wx + ww - 14, wy + 22, 12, '#4b5563', 400, SANS, 'right');
  p.rect(wx, wy + 34, ww, 38, '#ffffff');
  p.rrect(wx + 70, wy + 40, Math.min(520, ww - 90), 26, 13, '#f1f3f6');
  p.text('acme.com.br/painel/vendas', wx + 88, wy + 58, 12, '#374151');
  p.text('←   →   ⟳', wx + 14, wy + 58, 13, '#6b7280');
  p.rect(wx, wy + 72, ww, 1, '#e5e7eb');
  // Page.
  const px = wx, py = wy + 73, pw = ww;
  p.rect(px, py, pw, wh, '#f6f7f9');
  p.rect(px, py, pw, 50, '#111827');
  p.text('acme', px + 24, py + 32, 19, '#ffffff', 700);
  let nx = px + 100;
  for (const t of ['Início', 'Vendas', 'Relatórios', 'Clientes']) nx += p.text(t, nx, py + 31, 13, t === 'Vendas' ? '#ffffff' : '#9ca3af', 500) + 22;
  p.circle(px + pw - 36, py + 25, 14, '#f59e0b');
  p.text('AN', px + pw - 36, py + 30, 11, '#111827', 700, SANS, 'center');

  const cx = px + 28;
  let y = py + 50 + 34;
  p.text('Vendas de outubro', cx, y, 22, '#111827', 700);
  p.text('Atualizado há 5 minutos', cx, y + 20, 13, '#6b7280');
  y += 36;
  const gap = 24, cw = Math.min(220, (pw - 56 - 2 * gap) / 3), ch = 96;
  const cards = [['Receita', 'R$ 84.210', '+12%'], ['Pedidos', '1.284', '+4%'], ['Clientes novos', '312', '+18%']];
  cards.forEach(([label, value, trend], i) => {
    const x = cx + i * (cw + gap);
    if (i === 2) p.shadow(14, 4, 'rgba(17,24,39,.18)', () => p.rrect(x, y, cw, ch, 10, '#ffffff'));
    else p.rrect(x, y, cw, ch, 10, '#ffffff', '#e5e7eb');
    p.text(label, x + 16, y + 24, 13, '#6b7280', 500);
    p.text(value, x + 16, y + 56, 24, '#111827', 700);
    p.rrect(x + 16, y + 66, 46, 20, 10, '#dcfce7');
    p.text(trend, x + 39, y + 80, 11, '#166534', 600, SANS, 'center');
  });
  y += ch + 20;
  p.rrect(cx, y, 112, 40, 8, '#2563eb');
  p.text('Exportar', cx + 56, y + 25, 14, '#ffffff', 600, SANS, 'center');
  p.rrect(cx + 124, y, 96, 40, 8, '#ffffff', '#d1d5db');
  p.text('Filtrar', cx + 172, y + 25, 14, '#111827', 500, SANS, 'center');
  y += 40 + 20;
  // Chart card with bars (good gaps to measure) and a photo-like gradient.
  const chw = Math.min(pw - 56, 3 * cw + 2 * gap), chh = Math.max(90, Math.min(170, wy + wh - y - 18));
  p.rrect(cx, y, chw, chh, 10, '#ffffff', '#e5e7eb');
  p.text('Pedidos por dia', cx + 16, y + 24, 13, '#374151', 600);
  const bars = [52, 78, 64, 96, 88, 120, 70, 104, 92, 60, 84, 110], k = (chh - 50) / 120;
  bars.forEach((h, i) => p.rect(cx + 20 + i * 28, y + chh - 16 - h * k, 18, h * k, i === 5 ? '#2563eb' : '#93c5fd'));
  const phx = cx + 20 + bars.length * 28 + 16;
  const phw = chw - (phx - cx) - 16;
  if (phw > 60) {
    p.grad(phx, y + 36, phw, chh - 52, [[0, '#fde68a'], [.55, '#f97316'], [1, '#7c2d12']]);
    p.circle(phx + phw * .7, y + 36 + (chh - 52) * .3, 14, '#fff7ed');
  }
  ctx.restore();
}

// Monitor 2 (100%): a dark code editor.
function drawDarkApp(ctx, m) {
  const p = pen(ctx, m);
  p.grad(0, 0, p.LW, p.LH, [[0, '#2d1f3d'], [1, '#101623']]);
  taskbar(p, true);

  const wx = 26, wy = 22, ww = p.LW - 52, wh = p.LH - 48 - 44;
  p.shadow(26, 10, 'rgba(0,0,0,.6)', () => p.rrect(wx, wy, ww, wh, 8, '#1e1e1e'));
  ctx.save();
  ctx.beginPath(); ctx.roundRect(m.x + Math.round(wx * p.s), m.y + Math.round(wy * p.s), Math.round(ww * p.s), Math.round(wh * p.s), 8 * p.s); ctx.clip();
  p.rect(wx, wy, ww, 32, '#2d2d2d');
  p.text('app.rs — pixmeter-demo', wx + ww / 2, wy + 21, 12, '#cccccc', 400, SANS, 'center');
  p.text('—    ☐    ✕', wx + ww - 14, wy + 21, 12, '#cccccc', 400, SANS, 'right');
  // Activity bar and sidebar.
  p.rect(wx, wy + 32, 48, wh - 32, '#333333');
  for (let i = 0; i < 4; i++) p.rrect(wx + 12, wy + 46 + i * 46, 24, 24, 5, i === 0 ? '#d4d4d4' : '#858585');
  const sw = Math.min(210, ww * .3);
  p.rect(wx + 48, wy + 32, sw, wh - 32, '#252526');
  p.text('EXPLORER', wx + 64, wy + 56, 11, '#bbbbbb', 600);
  const files = [['src', '#c09553'], ['main.rs', '#dea584'], ['app.rs', '#dea584'], ['ui.rs', '#dea584'], ['Cargo.toml', '#9c9c9c']];
  files.forEach(([f, c], i) => {
    const fy = wy + 70 + i * 24;
    if (f === 'app.rs') p.rect(wx + 48, fy, sw, 24, '#37373d');
    p.rect(wx + (i ? 82 : 66), fy + 7, 10, 10, c);
    p.text(f, wx + (i ? 98 : 82), fy + 16, 13, '#cccccc');
  });
  // Tabs.
  const ex = wx + 48 + sw, ew = ww - 48 - sw;
  p.rect(ex, wy + 32, ew, 35, '#2d2d2d');
  p.rect(ex, wy + 32, 120, 35, '#1e1e1e');
  p.text('app.rs', ex + 18, wy + 54, 13, '#ffffff');
  p.text('main.rs', ex + 138, wy + 54, 13, '#969696');
  // Code.
  const code = [
    [['// Measures the span around the cursor', '#6a9955']],
    [['pub fn ', '#569cd6'], ['measure', '#dcdcaa'], ['(img: &', '#d4d4d4'], ['Frame', '#4ec9b0'], [', at: ', '#d4d4d4'], ['Point', '#4ec9b0'], [') -> ', '#d4d4d4'], ['u32', '#4ec9b0'], [' {', '#d4d4d4']],
    [['    let ', '#569cd6'], ['reference', '#9cdcfe'], [' = img.', '#d4d4d4'], ['pixel', '#dcdcaa'], ['(at);', '#d4d4d4']],
    [['    let ', '#569cd6'], ['tolerance', '#9cdcfe'], [' = ', '#d4d4d4'], ['16', '#b5cea8'], [';', '#d4d4d4']],
    [['    let ', '#569cd6'], ['left', '#9cdcfe'], [' = img.', '#d4d4d4'], ['edge', '#dcdcaa'], ['(at, ', '#d4d4d4'], ['Dir', '#4ec9b0'], ['::Left);', '#d4d4d4']],
    [['    let ', '#569cd6'], ['right', '#9cdcfe'], [' = img.', '#d4d4d4'], ['edge', '#dcdcaa'], ['(at, ', '#d4d4d4'], ['Dir', '#4ec9b0'], ['::Right);', '#d4d4d4']],
    [['    right - left - ', '#d4d4d4'], ['1', '#b5cea8']],
    [['}', '#d4d4d4']],
    [['', '#d4d4d4']],
    [['const ', '#569cd6'], ['LABEL', '#4fc1ff'], [': &str = ', '#d4d4d4'], ['"Copiado"', '#ce9178'], [';', '#d4d4d4']],
  ];
  code.forEach((tokens, i) => {
    const ly = wy + 67 + 24 + i * 20;
    p.text(String(i + 1), ex + 34, ly, 13, i === 3 ? '#c6c6c6' : '#858585', 400, MONO, 'right');
    let tx = ex + 56;
    for (const [t, c] of tokens) tx += p.text(t, tx, ly, 13, c, 400, MONO);
  });
  // Notification card with a button.
  const nw = 300, nh = 92, nx = ex + ew - nw - 18, ny = wy + wh - 22 - nh - 16;
  if (nx > ex + 20) {
    p.shadow(16, 6, 'rgba(0,0,0,.55)', () => p.rrect(nx, ny, nw, nh, 6, '#252526'));
    p.rrect(nx, ny, nw, nh, 6, null, '#454545');
    p.circle(nx + 22, ny + 24, 8, '#3794ff');
    p.text('Extensão recomendada: rust-analyzer', nx + 38, ny + 29, 12, '#cccccc');
    p.rrect(nx + nw - 96, ny + nh - 40, 80, 26, 3, '#0e639c');
    p.text('Instalar', nx + nw - 56, ny + nh - 22, 12, '#ffffff', 400, SANS, 'center');
    p.rrect(nx + nw - 190, ny + nh - 40, 82, 26, 3, '#3a3d41');
    p.text('Agora não', nx + nw - 149, ny + nh - 22, 12, '#ffffff', 400, SANS, 'center');
  }
  // Status bar.
  p.rect(wx, wy + wh - 22, ww, 22, '#007acc');
  p.text('main   ⊗ 0  ⚠ 0', wx + 12, wy + wh - 7, 12, '#ffffff');
  p.text('Ln 4, Col 22    UTF-8    Rust', wx + ww - 12, wy + wh - 7, 12, '#ffffff', 400, SANS, 'right');
  ctx.restore();
}
