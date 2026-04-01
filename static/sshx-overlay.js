// sshx-overlay.js — React component inspector overlay for SSHX observability.
// Toggle inspector with Alt+I. Connection panel in bottom-left corner.
// Works in React dev mode only (_debugSource required).
(function () {
  'use strict';

  // ── Inspector state ────────────────────────────────────────────────────────
  var active = false;
  var locked = false; // true when a click has pinned the badge
  var currentTarget = null;
  var badge = null;

  // ── Fiber walker ──────────────────────────────────────────────────────────
  function getFiberFromDom(node) {
    var key = Object.keys(node).find(function (k) {
      return k.startsWith('__reactFiber$');
    });
    return key ? node[key] : null;
  }

  function findComponentFiber(node) {
    var fiber = getFiberFromDom(node);
    while (fiber) {
      if (typeof fiber.type === 'function' && fiber.type.name) return fiber;
      fiber = fiber.return;
    }
    return null;
  }

  // ── Stack trace parser (React 19 _debugStack) ───────────────────────────
  // React 19 replaced _debugSource with _debugStack (an Error object).
  // Parse its .stack to extract the first meaningful file path and line.
  function parseSourceFromStack(err) {
    if (!err) return null;
    // _debugStack can be an Error, a string, or an object with a non-string stack.
    var raw = typeof err === 'string' ? err
            : typeof err.stack === 'string' ? err.stack
            : String(err.stack || err);
    if (!raw) return null;
    var lines = raw.split('\n');
    for (var i = 0; i < lines.length; i++) {
      var line = lines[i].trim();
      // Skip empty lines and the "Error" header
      if (!line || line === 'Error') continue;

      // Match Chrome/V8 format: "at Name (url:line:col)" or "at url:line:col"
      // Also matches rsc:// protocol used by React Server Components
      var chromeMatch = line.match(/at\s+(?:.*?\s+)?\(?((?:rsc:\/\/|webpack-internal:\/\/\/|https?:\/\/|file:\/\/\/).*?):(\d+)(?::(\d+))?\)?$/);
      if (chromeMatch) {
        var filePath = cleanFilePath(chromeMatch[1]);
        if (filePath && !isFrameworkInternal(filePath)) {
          return { file: filePath, line: parseInt(chromeMatch[2], 10) };
        }
        continue;
      }

      // Match Firefox/Safari format: "name@url:line:col"
      var ffMatch = line.match(/@((?:rsc:\/\/|webpack-internal:\/\/\/|https?:\/\/|file:\/\/\/).*?):(\d+)(?::(\d+))?$/);
      if (ffMatch) {
        var filePath2 = cleanFilePath(ffMatch[1]);
        if (filePath2 && !isFrameworkInternal(filePath2)) {
          return { file: filePath2, line: parseInt(ffMatch[2], 10) };
        }
      }
    }
    return null;
  }

  // Strip protocol prefixes and query strings to get a clean path.
  function cleanFilePath(raw) {
    var p = raw;
    // rsc://React/Server/webpack-internal:///(rsc)/./app/foo.tsx → app/foo.tsx
    // Strip the rsc:// wrapper first, then handle inner webpack-internal:///
    p = p.replace(/^rsc:\/\/[^/]+\/[^/]+\//, '');
    // webpack-internal:///(rsc)/./src/foo.tsx → ./src/foo.tsx
    // webpack-internal:///./src/foo.tsx → ./src/foo.tsx
    p = p.replace(/^webpack-internal:\/\/\/(?:\([^)]*\)\/)?/, '');
    // http://host:port/_next/static/... → skip (not useful)
    if (p.match(/^\(?https?:\/\/.*\/_next\/static\//)) return null;
    // http://host:port/path → /path
    p = p.replace(/^https?:\/\/[^/]+/, '');
    // Strip query string and hash
    p = p.replace(/[?#].*$/, '');
    // Strip leading ./ for consistency
    p = p.replace(/^\.\//, '');
    return p || null;
  }

  // Skip React/Next.js internals in stack frames.
  function isFrameworkInternal(path) {
    return /node_modules\//.test(path) ||
           /\/_next\//.test(path) ||
           /\/react(-dom)?\//.test(path) ||
           /\/next\//.test(path) ||
           /\/scheduler\//.test(path) ||
           /\/chunk-.*\.js$/.test(path) ||
           /fakeJSXCallSite/.test(path) ||
           /react-stack-bottom-frame/.test(path);
  }

  function getComponentInfo(node) {
    var fiber = findComponentFiber(node);
    if (!fiber) return null;
    var file = null;
    var line = null;

    // React 18: _debugSource has fileName/lineNumber directly.
    if (fiber._debugSource) {
      file = fiber._debugSource.fileName;
      line = fiber._debugSource.lineNumber;
    }

    // React 19: _debugStack is an Error whose stack trace contains file info.
    if (!file && fiber._debugStack) {
      var parsed = parseSourceFromStack(fiber._debugStack);
      if (parsed) {
        file = parsed.file;
        line = parsed.line;
      }
    }

    // React 19 alt: some builds use fiber.type._debugTask or _debugInfo.
    if (!file && fiber._debugInfo && Array.isArray(fiber._debugInfo)) {
      for (var i = 0; i < fiber._debugInfo.length; i++) {
        var info = fiber._debugInfo[i];
        if (info && info.stack) {
          var parsed2 = parseSourceFromStack({ stack: info.stack });
          if (parsed2) { file = parsed2.file; line = parsed2.line; break; }
        }
      }
    }

    return {
      name: fiber.type.name || fiber.type.displayName || '(anonymous)',
      file: file,
      line: line,
    };
  }

  // ── Inspector badge ───────────────────────────────────────────────────────
  function createBadge() {
    var el = document.createElement('div');
    el.id = '__sshx_badge';
    Object.assign(el.style, {
      position: 'fixed',
      zIndex: '999999',
      background: '#18181b',
      border: '1px solid #52525b',
      borderRadius: '6px',
      padding: '5px 10px',
      fontSize: '12px',
      fontFamily: '"Fira Code VF", "Fira Code", monospace',
      color: '#d4d4d8',
      pointerEvents: 'auto',
      boxShadow: '0 2px 8px rgba(0,0,0,0.5)',
      display: 'none',
      maxWidth: '420px',
      lineHeight: '1.6',
    });
    document.body.appendChild(el);
    return el;
  }

  function showBadge(x, y, info) {
    if (!badge) badge = createBadge();
    var filePart = info.file
      ? '<span style="color:#818cf8">' +
        escapeHtml(info.file) +
        (info.line ? ':' + info.line : '') +
        '</span>'
      : '<span style="color:#71717a">no source</span>';
    badge.innerHTML =
      '<div><b style="color:#a5b4fc">' + escapeHtml(info.name) + '</b></div>' +
      '<div style="font-size:11px;margin-top:2px">' + filePart + '</div>' +
      '<button id="__sshx_open_btn" style="' +
      'margin-top:6px;background:#3730a3;color:#e0e7ff;border:none;' +
      'border-radius:4px;padding:3px 8px;font-size:11px;cursor:pointer;' +
      'font-family:inherit' +
      '">Show in SSHX</button>';
    badge.style.display = 'block';

    var bw = badge.offsetWidth || 240;
    var bh = badge.offsetHeight || 90;
    badge.style.left = Math.min(x + 14, window.innerWidth - bw - 8) + 'px';
    badge.style.top = Math.min(y + 14, window.innerHeight - bh - 8) + 'px';

    document.getElementById('__sshx_open_btn').onclick = function (e) {
      e.stopPropagation();
      if (info.file) openFileCardInSshx(x, y, info.file);
    };
  }

  function hideBadge() {
    if (badge) badge.style.display = 'none';
  }

  function escapeHtml(s) {
    var d = document.createElement('span');
    d.textContent = s;
    return d.innerHTML;
  }

  // ── Element highlight ─────────────────────────────────────────────────────
  function highlightElement(el) {
    if (!el) return;
    el.style.outline = '2px solid #818cf8';
    el.style.outlineOffset = '1px';
  }

  function unhighlightElement(el) {
    if (!el) return;
    el.style.outline = '';
    el.style.outlineOffset = '';
  }

  // ── Inspector event handlers ──────────────────────────────────────────────
  function onMouseMove(e) {
    if (locked) return; // badge is pinned — don't change selection
    var target = e.target;
    if (isOverlayElement(target)) return;
    if (target === currentTarget) return;

    unhighlightElement(currentTarget);
    currentTarget = target;

    var info = getComponentInfo(target);
    if (info) {
      highlightElement(target);
    } else {
      // no component here — clear highlight but leave badge alone
    }
  }

  function onMouseClick(e) {
    var target = e.target;
    if (isOverlayElement(target)) return; // let badge button clicks through

    e.preventDefault();
    e.stopPropagation();

    // Reset lock so we can re-evaluate the new target.
    locked = false;
    unhighlightElement(currentTarget);
    currentTarget = target;

    var info = getComponentInfo(target);
    if (info) {
      highlightElement(target);
      locked = true;
      showBadge(e.clientX, e.clientY, info);
    } else {
      hideBadge();
    }
  }

  function onKeyDown(e) {
    if (e.key === 'Escape' && active) {
      unhighlightElement(currentTarget);
      hideBadge();
      currentTarget = null;
      locked = false;
    }
    if ((e.altKey || e.metaKey) && e.key === 'i') {
      e.preventDefault();
      toggleInspector();
    }
  }

  function toggleInspector() {
    active = !active;
    if (active) {
      document.addEventListener('mousemove', onMouseMove, true);
      document.addEventListener('click', onMouseClick, true);
      console.log('[SSHX overlay] inspector active — click to select');
    } else {
      document.removeEventListener('mousemove', onMouseMove, true);
      document.removeEventListener('click', onMouseClick, true);
      unhighlightElement(currentTarget);
      hideBadge();
      currentTarget = null;
      locked = false;
      console.log('[SSHX overlay] inspector inactive');
    }
    updatePanelUI();
  }

  // ── SSHX bridge ───────────────────────────────────────────────────────────
  function openFileCardInSshx(x, y, filePath) {
    if (window.__sshxOpenFileCard) {
      window.__sshxOpenFileCard(x, y, filePath);
    } else {
      console.warn('[SSHX overlay] not connected to SSHX');
    }
  }

  // ── Connection panel UI ───────────────────────────────────────────────────
  var panel = null;
  var panelExpanded = false;

  function isOverlayElement(el) {
    return (panel && panel.contains(el)) || (badge && badge.contains(el)) || el === panel || el === badge;
  }

  var CSS = [
    '#__sshx_panel { position:fixed; bottom:12px; left:12px; z-index:999998; font-family:system-ui,-apple-system,sans-serif; font-size:12px; }',
    '#__sshx_panel * { box-sizing:border-box; }',
    '#__sshx_fab { width:36px; height:36px; border-radius:50%; border:1px solid #52525b; background:#18181b; color:#a5b4fc; cursor:pointer; display:flex; align-items:center; justify-content:center; box-shadow:0 2px 8px rgba(0,0,0,0.4); transition:opacity 0.15s; }',
    '#__sshx_fab:hover { opacity:1; }',
    '#__sshx_fab.connected { border-color:#34d399; }',
    '#__sshx_fab.connecting { border-color:#fbbf24; }',
    '#__sshx_card { position:absolute; bottom:44px; left:0; width:320px; background:#18181b; border:1px solid #52525b; border-radius:8px; padding:10px; box-shadow:0 4px 16px rgba(0,0,0,0.5); display:none; }',
    '#__sshx_card.open { display:block; }',
    '#__sshx_card label { color:#a1a1aa; font-size:11px; display:block; margin-bottom:4px; }',
    '#__sshx_card input { width:100%; background:#27272a; border:1px solid #3f3f46; border-radius:4px; color:#e4e4e7; padding:5px 8px; font-size:12px; font-family:monospace; outline:none; }',
    '#__sshx_card input:focus { border-color:#818cf8; }',
    '#__sshx_card .actions { display:flex; gap:6px; margin-top:8px; }',
    '#__sshx_card button { flex:1; padding:5px 0; border:none; border-radius:4px; font-size:11px; cursor:pointer; font-family:inherit; }',
    '#__sshx_card .btn-connect { background:#3730a3; color:#e0e7ff; }',
    '#__sshx_card .btn-connect:hover { background:#4338ca; }',
    '#__sshx_card .btn-disconnect { background:#7f1d1d; color:#fecaca; }',
    '#__sshx_card .btn-disconnect:hover { background:#991b1b; }',
    '#__sshx_card .btn-inspect { background:#27272a; color:#d4d4d8; border:1px solid #3f3f46; }',
    '#__sshx_card .btn-inspect:hover { background:#3f3f46; }',
    '#__sshx_card .btn-inspect.active { background:#312e81; color:#c7d2fe; border-color:#4338ca; }',
    '#__sshx_status { margin-top:6px; font-size:11px; color:#71717a; }',
    '#__sshx_status .dot { display:inline-block; width:6px; height:6px; border-radius:50%; margin-right:4px; vertical-align:middle; }',
    '#__sshx_status .dot.connected { background:#34d399; }',
    '#__sshx_status .dot.connecting { background:#fbbf24; }',
    '#__sshx_status .dot.disconnected { background:#71717a; }',
  ].join('\n');

  function injectStyles() {
    var style = document.createElement('style');
    style.textContent = CSS;
    document.head.appendChild(style);
  }

  function createPanel() {
    injectStyles();

    panel = document.createElement('div');
    panel.id = '__sshx_panel';
    panel.innerHTML =
      '<div id="__sshx_card">' +
        '<label>SSHX Session URL</label>' +
        '<input id="__sshx_url_input" type="text" placeholder="https://host/s/SESSION#KEY" spellcheck="false" autocomplete="off" />' +
        '<div class="actions">' +
          '<button id="__sshx_btn_connect" class="btn-connect">Connect</button>' +
          '<button id="__sshx_btn_disconnect" class="btn-disconnect" style="display:none">Disconnect</button>' +
          '<button id="__sshx_btn_inspect" class="btn-inspect">Inspect</button>' +
        '</div>' +
        '<div id="__sshx_status"><span class="dot disconnected"></span>Disconnected</div>' +
      '</div>' +
      '<div id="__sshx_fab" title="SSHX Overlay">' +
        // Simple terminal icon as SVG
        '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="4 17 10 11 4 5"></polyline><line x1="12" y1="19" x2="20" y2="19"></line></svg>' +
      '</div>';
    document.body.appendChild(panel);

    // Populate saved URL
    var saved = localStorage.getItem('__sshx_url');
    if (saved) {
      document.getElementById('__sshx_url_input').value = saved;
    }

    // FAB click → toggle card
    document.getElementById('__sshx_fab').onclick = function () {
      panelExpanded = !panelExpanded;
      updatePanelUI();
    };

    // Connect button
    document.getElementById('__sshx_btn_connect').onclick = function () {
      var url = document.getElementById('__sshx_url_input').value.trim();
      if (!url) return;
      if (window.__sshxConnect) {
        window.__sshxConnect(url).catch(function (e) {
          console.error('[SSHX overlay] connect failed', e);
        });
      }
    };

    // Disconnect button
    document.getElementById('__sshx_btn_disconnect').onclick = function () {
      if (window.__sshxDisconnect) window.__sshxDisconnect();
    };

    // Inspect toggle button
    document.getElementById('__sshx_btn_inspect').onclick = function () {
      toggleInspector();
    };

    // Enter key in URL input → connect
    document.getElementById('__sshx_url_input').onkeydown = function (e) {
      if (e.key === 'Enter') {
        document.getElementById('__sshx_btn_connect').click();
      }
    };

    // Listen for status changes from sshx-connect.js
    if (window.__sshxOnStatus) {
      window.__sshxOnStatus(function () { updatePanelUI(); });
    }

    updatePanelUI();
  }

  function updatePanelUI() {
    if (!panel) return;
    var card = document.getElementById('__sshx_card');
    var fab = document.getElementById('__sshx_fab');
    var btnConnect = document.getElementById('__sshx_btn_connect');
    var btnDisconnect = document.getElementById('__sshx_btn_disconnect');
    var btnInspect = document.getElementById('__sshx_btn_inspect');
    var statusEl = document.getElementById('__sshx_status');

    var status = window.__sshxStatus || 'disconnected';

    // Card visibility
    if (panelExpanded) card.classList.add('open');
    else card.classList.remove('open');

    // FAB color
    fab.className = status;

    // Show/hide connect vs disconnect
    if (status === 'connected') {
      btnConnect.style.display = 'none';
      btnDisconnect.style.display = '';
    } else {
      btnConnect.style.display = '';
      btnDisconnect.style.display = 'none';
      if (status === 'connecting') btnConnect.textContent = 'Connecting...';
      else btnConnect.textContent = 'Connect';
    }

    // Inspect button active state
    if (active) btnInspect.classList.add('active');
    else btnInspect.classList.remove('active');
    btnInspect.textContent = active ? 'Inspector ON' : 'Inspect (Alt+I)';

    // Status line
    var label = status.charAt(0).toUpperCase() + status.slice(1);
    statusEl.innerHTML = '<span class="dot ' + status + '"></span>' + label;
  }

  // ── Bidirectional highlight (from SSHX → overlay) ─────────────────────────
  try {
    var bc = new BroadcastChannel('sshx-overlay');
    bc.onmessage = function (e) {
      if (e.data && e.data.type === 'HighlightComponent' && e.data.name) {
        flashComponent(e.data.name);
      }
    };
  } catch (_) {}

  function flashComponent(name) {
    var targets = findByFiberName(name);
    targets.forEach(function (el) {
      el.style.outline = '3px solid #f59e0b';
      el.style.outlineOffset = '2px';
      setTimeout(function () {
        el.style.outline = '';
        el.style.outlineOffset = '';
      }, 1500);
    });
    if (targets.length) {
      console.log('[SSHX overlay] highlighted', targets.length, 'element(s) for', name);
    }
  }

  function findByFiberName(name) {
    var results = [];
    var walker = document.createTreeWalker(document.body, NodeFilter.SHOW_ELEMENT);
    var node;
    while ((node = walker.nextNode())) {
      var key = Object.keys(node).find(function (k) {
        return k.startsWith('__reactFiber$');
      });
      if (!key) continue;
      var fiber = node[key];
      while (fiber) {
        if (typeof fiber.type === 'function' && fiber.type.name === name) {
          results.push(node);
          break;
        }
        fiber = fiber.return;
      }
    }
    return results;
  }

  // ── Init ──────────────────────────────────────────────────────────────────
  document.addEventListener('keydown', onKeyDown, true);

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', createPanel);
  } else {
    createPanel();
  }

  console.log('[SSHX overlay] loaded — press Alt+I to toggle inspector');
})();
