// sshx-connect.js — Framework-agnostic SSHX WebSocket connector.
// Authenticates with Argon2id + AES-128-CTR and exposes:
//   window.__sshxConnect(url)    — connect to an SSHX session
//   window.__sshxDisconnect()    — close the connection
//   window.__sshxOpenFileCard()  — open a FileCard in the session
//   window.__sshxStatus          — 'disconnected' | 'connecting' | 'connected'
//   window.__sshxOnStatus(fn)    — register a status change callback
//
// Auto-connects if localStorage has a saved URL from a previous session.
(function () {
  'use strict';

  var STORAGE_KEY = '__sshx_url';
  var ws = null;
  var authenticated = false;
  var keyBytes = null;
  var currentUrl = null;
  var reconnectTimer = null;
  var reconnectDelay = 2000;
  var intentionalClose = false;
  var statusListeners = [];

  window.__sshxStatus = 'disconnected';

  function setStatus(s) {
    window.__sshxStatus = s;
    for (var i = 0; i < statusListeners.length; i++) {
      try { statusListeners[i](s); } catch (_) {}
    }
  }

  window.__sshxOnStatus = function (fn) {
    statusListeners.push(fn);
  };

  // ── URL parsing ───────────────────────────────────────────────────────────
  function parseSshxUrl(url) {
    var u = new URL(url);
    var sid = u.pathname.split('/').pop();
    var key = u.hash.replace(/^#/, '');
    var wsProto = u.protocol === 'https:' ? 'wss:' : 'ws:';
    return { sessionId: sid, key: key, wsUrl: wsProto + '//' + u.host + '/api/s/' + sid };
  }

  // ── Argon2 loader ─────────────────────────────────────────────────────────
  function loadArgon2() {
    if (window.argon2) return Promise.resolve(window.argon2);
    return new Promise(function (resolve, reject) {
      var s = document.createElement('script');
      s.src = 'https://cdn.jsdelivr.net/npm/argon2-browser@1.18.0/dist/argon2-bundled.min.js';
      s.onload = function () { resolve(window.argon2); };
      s.onerror = reject;
      document.head.appendChild(s);
    });
  }

  // ── Constants (must match encrypt.rs / encrypt.ts) ───────────────────────
  var SALT = 'This is a non-random salt for sshx.io, since we want to stretch the security of 83-bit keys!';

  // ── Key derivation ────────────────────────────────────────────────────────
  async function deriveKey(keyStr) {
    var argon2 = await loadArgon2();
    var result = await argon2.hash({
      pass: keyStr,
      salt: SALT,
      type: argon2.ArgonType.Argon2id,
      hashLen: 16,
      mem: 19 * 1024, // 19456 KiB
      time: 2,        // 2 iterations
      parallelism: 1,
    });
    // result.hashHex is a hex string; parse to Uint8Array for Web Crypto.
    var hex = result.hashHex;
    var bytes = new Uint8Array(hex.length / 2);
    for (var i = 0; i < bytes.length; i++) {
      bytes[i] = parseInt(hex.substr(i * 2, 2), 16);
    }
    return bytes;
  }

  // ── Encrypted zeros (auth proof) ──────────────────────────────────────────
  // Must match Encrypt::zeros() in encrypt.rs / encrypt.ts:
  //   nonce = 16 zero bytes, plaintext = 16 zero bytes → 16-byte ciphertext.
  async function computeEncryptedZeros(kb) {
    var zeros = new Uint8Array(16);
    var cryptoKey = await crypto.subtle.importKey(
      'raw', kb, { name: 'AES-CTR' }, false, ['encrypt']
    );
    var ct = await crypto.subtle.encrypt(
      { name: 'AES-CTR', counter: zeros, length: 64 },
      cryptoKey, zeros
    );
    return new Uint8Array(ct);
  }

  // ── Minimal CBOR encoder ──────────────────────────────────────────────────
  function encodeCbor(value) {
    var parts = [];
    function w(v) {
      if (v === null || v === undefined) {
        parts.push(new Uint8Array([0xf6]));
        return;
      }
      if (typeof v === 'boolean') {
        parts.push(new Uint8Array([v ? 0xf5 : 0xf4]));
        return;
      }
      if (typeof v === 'string') {
        var b = new TextEncoder().encode(v);
        parts.push(h(3, b.length));
        parts.push(b);
        return;
      }
      if (typeof v === 'number') {
        if (Number.isInteger(v) && v >= 0) { parts.push(h(0, v)); return; }
        if (Number.isInteger(v) && v < 0) { parts.push(h(1, -1 - v)); return; }
        var buf = new ArrayBuffer(9);
        var view = new DataView(buf);
        view.setUint8(0, 0xfb);
        view.setFloat64(1, v);
        parts.push(new Uint8Array(buf));
        return;
      }
      if (v instanceof Uint8Array) {
        // Wrap in CBOR tag 64 (typed byte string) to match cbor-x encoding.
        // Tag 64 = 0xd8 0x40, then the byte string.
        parts.push(new Uint8Array([0xd8, 0x40]));
        parts.push(h(2, v.length));
        parts.push(v);
        return;
      }
      if (Array.isArray(v)) {
        parts.push(h(4, v.length));
        v.forEach(w);
        return;
      }
      if (typeof v === 'object') {
        var keys = Object.keys(v);
        parts.push(h(5, keys.length));
        keys.forEach(function (k) { w(k); w(v[k]); });
        return;
      }
    }
    function h(major, n) {
      var tag = major << 5;
      if (n <= 23) return new Uint8Array([tag | n]);
      if (n <= 0xff) return new Uint8Array([tag | 24, n]);
      if (n <= 0xffff) return new Uint8Array([tag | 25, n >> 8, n & 0xff]);
      return new Uint8Array([tag | 26, (n >>> 24) & 0xff, (n >>> 16) & 0xff, (n >>> 8) & 0xff, n & 0xff]);
    }
    w(value);
    var total = parts.reduce(function (s, p) { return s + p.length; }, 0);
    var out = new Uint8Array(total);
    var off = 0;
    for (var i = 0; i < parts.length; i++) {
      out.set(parts[i], off);
      off += parts[i].length;
    }
    return out;
  }

  // ── Minimal CBOR decoder (enough for server messages) ─────────────────────
  function decodeCbor(data) {
    var buf = data instanceof ArrayBuffer ? new Uint8Array(data) : data;
    var pos = 0;

    function read() {
      var initial = buf[pos++];
      var major = initial >> 5;
      var info = initial & 0x1f;
      var val = readArgument(info);

      switch (major) {
        case 0: return val;
        case 1: return -1 - val;
        case 2: { var bs = buf.slice(pos, pos + val); pos += val; return bs; }
        case 3: { var ts = new TextDecoder().decode(buf.slice(pos, pos + val)); pos += val; return ts; }
        case 4: { var arr = []; for (var i = 0; i < val; i++) arr.push(read()); return arr; }
        case 5: { var obj = {}; for (var j = 0; j < val; j++) { var k = read(); obj[k] = read(); } return obj; }
        case 7: {
          if (info === 20) return false;
          if (info === 21) return true;
          if (info === 22) return null;
          if (info === 23) return undefined;
          if (info === 25) { pos += 2; return 0; }
          if (info === 26) { var f32 = new DataView(buf.buffer, buf.byteOffset + pos, 4).getFloat32(0); pos += 4; return f32; }
          if (info === 27) { var f64 = new DataView(buf.buffer, buf.byteOffset + pos, 8).getFloat64(0); pos += 8; return f64; }
          return null;
        }
        default: return null;
      }
    }

    function readArgument(info) {
      if (info <= 23) return info;
      if (info === 24) return buf[pos++];
      if (info === 25) { var v = (buf[pos] << 8) | buf[pos + 1]; pos += 2; return v; }
      if (info === 26) { var v2 = ((buf[pos] << 24) | (buf[pos + 1] << 16) | (buf[pos + 2] << 8) | buf[pos + 3]) >>> 0; pos += 4; return v2; }
      if (info === 27) { pos += 4; var lo = ((buf[pos] << 24) | (buf[pos + 1] << 16) | (buf[pos + 2] << 8) | buf[pos + 3]) >>> 0; pos += 4; return lo; }
      return 0;
    }

    try { return read(); } catch (_) { return null; }
  }

  // ── WebSocket connection ──────────────────────────────────────────────────
  async function doConnect(url) {
    var parsed = parseSshxUrl(url);
    if (!parsed.key || !parsed.sessionId) {
      throw new Error('Invalid SSHX URL — must include session ID and #key fragment');
    }

    setStatus('connecting');
    keyBytes = await deriveKey(parsed.key);

    ws = new WebSocket(parsed.wsUrl);
    ws.binaryType = 'arraybuffer';

    ws.onopen = function () {
      console.log('[SSHX connect] WebSocket open');
      reconnectDelay = 2000;
    };

    ws.onmessage = async function (event) {
      if (!authenticated) {
        var ez = await computeEncryptedZeros(keyBytes);
        ws.send(encodeCbor({ authenticate: [ez, null] }));
        authenticated = true;
        setStatus('connected');
        console.log('[SSHX connect] authenticated');
      }
      handleServerMessage(event.data);
    };

    ws.onclose = function (e) {
      console.log('[SSHX connect] closed', e.code);
      authenticated = false;
      setStatus('disconnected');
      if (!intentionalClose && e.code !== 4404) {
        reconnectTimer = setTimeout(function () {
          doConnect(currentUrl).catch(function (err) {
            console.warn('[SSHX connect] reconnect failed', err);
          });
        }, reconnectDelay);
        reconnectDelay = Math.min(reconnectDelay * 1.5, 30000);
      }
      if (e.code === 4404) {
        console.warn('[SSHX connect] session not found — clearing saved URL');
        localStorage.removeItem(STORAGE_KEY);
        currentUrl = null;
      }
    };

    ws.onerror = function () {
      console.warn('[SSHX connect] WebSocket error');
    };
  }

  // ── Handle server messages ────────────────────────────────────────────────
  function handleServerMessage(data) {
    try {
      var msg = decodeCbor(data);
      if (!msg || typeof msg !== 'object') return;

      if (msg.invalidAuth) {
        console.warn('[SSHX connect] invalid auth — wrong key?');
        localStorage.removeItem(STORAGE_KEY);
        currentUrl = null;
        intentionalClose = true;
        if (ws) ws.close();
        setStatus('disconnected');
      }

      // Extract workspaceRootPath from sourceFiles message: [rootName, rootPath, files[]]
      if (msg.sourceFiles && Array.isArray(msg.sourceFiles) && msg.sourceFiles.length >= 2) {
        window.__sshxWorkspaceRoot = msg.sourceFiles[1] || '';
        console.log('[SSHX connect] workspaceRoot:', window.__sshxWorkspaceRoot);
      }

      if (msg.highlightComponent) {
        try {
          var bc = new BroadcastChannel('sshx-overlay');
          bc.postMessage({ type: 'HighlightComponent', name: msg.highlightComponent });
          bc.close();
        } catch (_) {}
      }
    } catch (_) {}
  }

  // ── Public API ────────────────────────────────────────────────────────────
  window.__sshxConnect = function (url) {
    // Disconnect any existing connection first
    window.__sshxDisconnect();
    currentUrl = url;
    intentionalClose = false;
    localStorage.setItem(STORAGE_KEY, url);
    return doConnect(url);
  };

  window.__sshxDisconnect = function () {
    intentionalClose = true;
    if (reconnectTimer) { clearTimeout(reconnectTimer); reconnectTimer = null; }
    if (ws) { ws.close(); ws = null; }
    authenticated = false;
    currentUrl = null;
    localStorage.removeItem(STORAGE_KEY);
    setStatus('disconnected');
  };

  window.__sshxOpenFileCard = function (clientX, clientY, filePath) {
    if (!ws || ws.readyState !== WebSocket.OPEN) {
      console.warn('[SSHX connect] not connected');
      return;
    }
    var x = clientX + 200;
    var y = clientY - 50;
    ws.send(encodeCbor({ openFileCard: [x, y, filePath] }));
    console.log('[SSHX connect] openFileCard', filePath, 'at', x, y);
  };

  window.__sshxHighlightComponent = function (name) {
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    ws.send(encodeCbor({ highlightComponent: name }));
  };

  // ── Auto-connect from localStorage ────────────────────────────────────────
  var saved = localStorage.getItem(STORAGE_KEY);
  if (saved) {
    currentUrl = saved;
    doConnect(saved).catch(function (e) {
      console.warn('[SSHX connect] auto-connect failed', e);
    });
  }

  console.log('[SSHX connect] loaded');
})();
