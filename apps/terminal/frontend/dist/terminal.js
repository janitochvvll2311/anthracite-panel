// node_modules/@xterm/addon-fit/lib/addon-fit.mjs
var h = 2;
var _ = 1;
var o = class {
  activate(e) {
    this._terminal = e;
  }
  dispose() {}
  fit() {
    let e = this.proposeDimensions();
    if (!e || !this._terminal || isNaN(e.cols) || isNaN(e.rows))
      return;
    let t = this._terminal._core;
    (this._terminal.rows !== e.rows || this._terminal.cols !== e.cols) && (t._renderService.clear(), this._terminal.resize(e.cols, e.rows));
  }
  proposeDimensions() {
    if (!this._terminal || !this._terminal.element || !this._terminal.element.parentElement)
      return;
    let t = this._terminal._core._renderService.dimensions;
    if (t.css.cell.width === 0 || t.css.cell.height === 0)
      return;
    let s = this._terminal.options.scrollback === 0 ? 0 : this._terminal.options.overviewRuler?.width || 14, r = window.getComputedStyle(this._terminal.element.parentElement), l = parseInt(r.getPropertyValue("height")), a = Math.max(0, parseInt(r.getPropertyValue("width"))), i = window.getComputedStyle(this._terminal.element), n = { top: parseInt(i.getPropertyValue("padding-top")), bottom: parseInt(i.getPropertyValue("padding-bottom")), right: parseInt(i.getPropertyValue("padding-right")), left: parseInt(i.getPropertyValue("padding-left")) }, m = n.top + n.bottom, d = n.right + n.left, c = l - m, p = a - d - s;
    return { cols: Math.max(h, Math.floor(p / t.css.cell.width)), rows: Math.max(_, Math.floor(c / t.css.cell.height)) };
  }
};

// node_modules/@xterm/addon-webgl/lib/addon-webgl.mjs
var Lr = Object.defineProperty;
var wr = Object.getOwnPropertyDescriptor;
var Yi = (i, e, t, n) => {
  for (var s = n > 1 ? undefined : n ? wr(e, t) : e, o = i.length - 1, r;o >= 0; o--)
    (r = i[o]) && (s = (n ? r(e, t, s) : r(s)) || s);
  return n && s && Lr(e, t, s), s;
};
var Qi = (i, e) => (t, n) => e(t, n, i);
var pi = class {
  constructor() {
    this.listeners = [], this.unexpectedErrorHandler = function(e) {
      setTimeout(() => {
        throw e.stack ? bt.isErrorNoTelemetry(e) ? new bt(e.message + `

` + e.stack) : new Error(e.message + `

` + e.stack) : e;
      }, 0);
    };
  }
  addListener(e) {
    return this.listeners.push(e), () => {
      this._removeListener(e);
    };
  }
  emit(e) {
    this.listeners.forEach((t) => {
      t(e);
    });
  }
  _removeListener(e) {
    this.listeners.splice(this.listeners.indexOf(e), 1);
  }
  setUnexpectedErrorHandler(e) {
    this.unexpectedErrorHandler = e;
  }
  getUnexpectedErrorHandler() {
    return this.unexpectedErrorHandler;
  }
  onUnexpectedError(e) {
    this.unexpectedErrorHandler(e), this.emit(e);
  }
  onUnexpectedExternalError(e) {
    this.unexpectedErrorHandler(e);
  }
};
var Rr = new pi;
function Pe(i) {
  Dr(i) || Rr.onUnexpectedError(i);
}
var fi = "Canceled";
function Dr(i) {
  return i instanceof Ye ? true : i instanceof Error && i.name === fi && i.message === fi;
}
var Ye = class extends Error {
  constructor() {
    super(fi), this.name = this.message;
  }
};
var bt = class i extends Error {
  constructor(e) {
    super(e), this.name = "CodeExpectedError";
  }
  static fromError(e) {
    if (e instanceof i)
      return e;
    let t = new i;
    return t.message = e.message, t.stack = e.stack, t;
  }
  static isErrorNoTelemetry(e) {
    return e.name === "CodeExpectedError";
  }
};
function Mr(i, e, t = 0, n = i.length) {
  let s = t, o = n;
  for (;s < o; ) {
    let r = Math.floor((s + o) / 2);
    e(i[r]) ? s = r + 1 : o = r;
  }
  return s - 1;
}
var vt = class vt {
  constructor(e) {
    this._array = e;
    this._findLastMonotonousLastIdx = 0;
  }
  findLastMonotonous(e) {
    if (vt.assertInvariants) {
      if (this._prevFindLastPredicate) {
        for (let n of this._array)
          if (this._prevFindLastPredicate(n) && !e(n))
            throw new Error("MonotonousArray: current predicate must be weaker than (or equal to) the previous predicate.");
      }
      this._prevFindLastPredicate = e;
    }
    let t = Mr(this._array, e, this._findLastMonotonousLastIdx);
    return this._findLastMonotonousLastIdx = t + 1, t === -1 ? undefined : this._array[t];
  }
};
vt.assertInvariants = false;
var en;
((a) => {
  function i(l) {
    return l < 0;
  }
  a.isLessThan = i;
  function e(l) {
    return l <= 0;
  }
  a.isLessThanOrEqual = e;
  function t(l) {
    return l > 0;
  }
  a.isGreaterThan = t;
  function n(l) {
    return l === 0;
  }
  a.isNeitherLessOrGreaterThan = n, a.greaterThan = 1, a.lessThan = -1, a.neitherLessOrGreaterThan = 0;
})(en ||= {});
function tn(i, e) {
  return (t, n) => e(i(t), i(n));
}
var nn = (i, e) => i - e;
var Be = class Be {
  constructor(e) {
    this.iterate = e;
  }
  forEach(e) {
    this.iterate((t) => (e(t), true));
  }
  toArray() {
    let e = [];
    return this.iterate((t) => (e.push(t), true)), e;
  }
  filter(e) {
    return new Be((t) => this.iterate((n) => e(n) ? t(n) : true));
  }
  map(e) {
    return new Be((t) => this.iterate((n) => t(e(n))));
  }
  some(e) {
    let t = false;
    return this.iterate((n) => (t = e(n), !t)), t;
  }
  findFirst(e) {
    let t;
    return this.iterate((n) => e(n) ? (t = n, false) : true), t;
  }
  findLast(e) {
    let t;
    return this.iterate((n) => (e(n) && (t = n), true)), t;
  }
  findLastMaxBy(e) {
    let t, n = true;
    return this.iterate((s) => ((n || en.isGreaterThan(e(s, t))) && (n = false, t = s), true)), t;
  }
};
Be.empty = new Be((e) => {});
function an(i, e) {
  let t = Object.create(null);
  for (let n of i) {
    let s = e(n), o = t[s];
    o || (o = t[s] = []), o.push(n);
  }
  return t;
}
var sn;
var on;
var rn = class {
  constructor(e, t) {
    this.toKey = t;
    this._map = new Map;
    this[sn] = "SetWithKey";
    for (let n of e)
      this.add(n);
  }
  get size() {
    return this._map.size;
  }
  add(e) {
    let t = this.toKey(e);
    return this._map.set(t, e), this;
  }
  delete(e) {
    return this._map.delete(this.toKey(e));
  }
  has(e) {
    return this._map.has(this.toKey(e));
  }
  *entries() {
    for (let e of this._map.values())
      yield [e, e];
  }
  keys() {
    return this.values();
  }
  *values() {
    for (let e of this._map.values())
      yield e;
  }
  clear() {
    this._map.clear();
  }
  forEach(e, t) {
    this._map.forEach((n) => e.call(t, n, n, this));
  }
  [(on = Symbol.iterator, sn = Symbol.toStringTag, on)]() {
    return this.values();
  }
};
var Tt = class {
  constructor() {
    this.map = new Map;
  }
  add(e, t) {
    let n = this.map.get(e);
    n || (n = new Set, this.map.set(e, n)), n.add(t);
  }
  delete(e, t) {
    let n = this.map.get(e);
    n && (n.delete(t), n.size === 0 && this.map.delete(e));
  }
  forEach(e, t) {
    let n = this.map.get(e);
    n && n.forEach(t);
  }
  get(e) {
    let t = this.map.get(e);
    return t || new Set;
  }
};
function mi(i, e) {
  let t = this, n = false, s;
  return function() {
    if (n)
      return s;
    if (n = true, e)
      try {
        s = i.apply(t, arguments);
      } finally {
        e();
      }
    else
      s = i.apply(t, arguments);
    return s;
  };
}
var _i;
((W) => {
  function i(E) {
    return E && typeof E == "object" && typeof E[Symbol.iterator] == "function";
  }
  W.is = i;
  let e = Object.freeze([]);
  function t() {
    return e;
  }
  W.empty = t;
  function* n(E) {
    yield E;
  }
  W.single = n;
  function s(E) {
    return i(E) ? E : n(E);
  }
  W.wrap = s;
  function o(E) {
    return E || e;
  }
  W.from = o;
  function* r(E) {
    for (let y = E.length - 1;y >= 0; y--)
      yield E[y];
  }
  W.reverse = r;
  function a(E) {
    return !E || E[Symbol.iterator]().next().done === true;
  }
  W.isEmpty = a;
  function l(E) {
    return E[Symbol.iterator]().next().value;
  }
  W.first = l;
  function u(E, y) {
    let w = 0;
    for (let G of E)
      if (y(G, w++))
        return true;
    return false;
  }
  W.some = u;
  function c(E, y) {
    for (let w of E)
      if (y(w))
        return w;
  }
  W.find = c;
  function* d(E, y) {
    for (let w of E)
      y(w) && (yield w);
  }
  W.filter = d;
  function* h(E, y) {
    let w = 0;
    for (let G of E)
      yield y(G, w++);
  }
  W.map = h;
  function* f(E, y) {
    let w = 0;
    for (let G of E)
      yield* y(G, w++);
  }
  W.flatMap = f;
  function* I(...E) {
    for (let y of E)
      yield* y;
  }
  W.concat = I;
  function L(E, y, w) {
    let G = w;
    for (let ue of E)
      G = y(G, ue);
    return G;
  }
  W.reduce = L;
  function* M(E, y, w = E.length) {
    for (y < 0 && (y += E.length), w < 0 ? w += E.length : w > E.length && (w = E.length);y < w; y++)
      yield E[y];
  }
  W.slice = M;
  function q(E, y = Number.POSITIVE_INFINITY) {
    let w = [];
    if (y === 0)
      return [w, E];
    let G = E[Symbol.iterator]();
    for (let ue = 0;ue < y; ue++) {
      let Se = G.next();
      if (Se.done)
        return [w, W.empty()];
      w.push(Se.value);
    }
    return [w, { [Symbol.iterator]() {
      return G;
    } }];
  }
  W.consume = q;
  async function S(E) {
    let y = [];
    for await (let w of E)
      y.push(w);
    return Promise.resolve(y);
  }
  W.asyncToArray = S;
})(_i ||= {});
var Ar = false;
var Ne = null;
var gt = class gt {
  constructor() {
    this.livingDisposables = new Map;
  }
  getDisposableData(e) {
    let t = this.livingDisposables.get(e);
    return t || (t = { parent: null, source: null, isSingleton: false, value: e, idx: gt.idx++ }, this.livingDisposables.set(e, t)), t;
  }
  trackDisposable(e) {
    let t = this.getDisposableData(e);
    t.source || (t.source = new Error().stack);
  }
  setParent(e, t) {
    let n = this.getDisposableData(e);
    n.parent = t;
  }
  markAsDisposed(e) {
    this.livingDisposables.delete(e);
  }
  markAsSingleton(e) {
    this.getDisposableData(e).isSingleton = true;
  }
  getRootParent(e, t) {
    let n = t.get(e);
    if (n)
      return n;
    let s = e.parent ? this.getRootParent(this.getDisposableData(e.parent), t) : e;
    return t.set(e, s), s;
  }
  getTrackedDisposables() {
    let e = new Map;
    return [...this.livingDisposables.entries()].filter(([, n]) => n.source !== null && !this.getRootParent(n, e).isSingleton).flatMap(([n]) => n);
  }
  computeLeakingDisposables(e = 10, t) {
    let n;
    if (t)
      n = t;
    else {
      let l = new Map, u = [...this.livingDisposables.values()].filter((d) => d.source !== null && !this.getRootParent(d, l).isSingleton);
      if (u.length === 0)
        return;
      let c = new Set(u.map((d) => d.value));
      if (n = u.filter((d) => !(d.parent && c.has(d.parent))), n.length === 0)
        throw new Error("There are cyclic diposable chains!");
    }
    if (!n)
      return;
    function s(l) {
      function u(d, h) {
        for (;d.length > 0 && h.some((f) => typeof f == "string" ? f === d[0] : d[0].match(f)); )
          d.shift();
      }
      let c = l.source.split(`
`).map((d) => d.trim().replace("at ", "")).filter((d) => d !== "");
      return u(c, ["Error", /^trackDisposable \(.*\)$/, /^DisposableTracker.trackDisposable \(.*\)$/]), c.reverse();
    }
    let o = new Tt;
    for (let l of n) {
      let u = s(l);
      for (let c = 0;c <= u.length; c++)
        o.add(u.slice(0, c).join(`
`), l);
    }
    n.sort(tn((l) => l.idx, nn));
    let r = "", a = 0;
    for (let l of n.slice(0, e)) {
      a++;
      let u = s(l), c = [];
      for (let d = 0;d < u.length; d++) {
        let h = u[d];
        h = `(shared with ${o.get(u.slice(0, d + 1).join(`
`)).size}/${n.length} leaks) at ${h}`;
        let I = o.get(u.slice(0, d).join(`
`)), L = an([...I].map((M) => s(M)[d]), (M) => M);
        delete L[u[d]];
        for (let [M, q] of Object.entries(L))
          c.unshift(`    - stacktraces of ${q.length} other leaks continue with ${M}`);
        c.unshift(h);
      }
      r += `


==================== Leaking disposable ${a}/${n.length}: ${l.value.constructor.name} ====================
${c.join(`
`)}
============================================================

`;
    }
    return n.length > e && (r += `


... and ${n.length - e} more leaking disposables

`), { leaks: n, details: r };
  }
};
gt.idx = 0;
function Sr(i) {
  Ne = i;
}
if (Ar) {
  let i = "__is_disposable_tracked__";
  Sr(new class {
    trackDisposable(e) {
      let t = new Error("Potentially leaked disposable").stack;
      setTimeout(() => {
        e[i] || console.log(t);
      }, 3000);
    }
    setParent(e, t) {
      if (e && e !== B.None)
        try {
          e[i] = true;
        } catch {}
    }
    markAsDisposed(e) {
      if (e && e !== B.None)
        try {
          e[i] = true;
        } catch {}
    }
    markAsSingleton(e) {}
  });
}
function Et(i) {
  return Ne?.trackDisposable(i), i;
}
function yt(i) {
  Ne?.markAsDisposed(i);
}
function Qe(i, e) {
  Ne?.setParent(i, e);
}
function Or(i, e) {
  if (Ne)
    for (let t of i)
      Ne.setParent(t, e);
}
function un(i) {
  if (_i.is(i)) {
    let e = [];
    for (let t of i)
      if (t)
        try {
          t.dispose();
        } catch (n) {
          e.push(n);
        }
    if (e.length === 1)
      throw e[0];
    if (e.length > 1)
      throw new AggregateError(e, "Encountered errors while disposing of store");
    return Array.isArray(i) ? [] : i;
  } else if (i)
    return i.dispose(), i;
}
function It(...i) {
  let e = O(() => un(i));
  return Or(i, e), e;
}
function O(i) {
  let e = Et({ dispose: mi(() => {
    yt(e), i();
  }) });
  return e;
}
var xt = class xt {
  constructor() {
    this._toDispose = new Set;
    this._isDisposed = false;
    Et(this);
  }
  dispose() {
    this._isDisposed || (yt(this), this._isDisposed = true, this.clear());
  }
  get isDisposed() {
    return this._isDisposed;
  }
  clear() {
    if (this._toDispose.size !== 0)
      try {
        un(this._toDispose);
      } finally {
        this._toDispose.clear();
      }
  }
  add(e) {
    if (!e)
      return e;
    if (e === this)
      throw new Error("Cannot register a disposable on itself!");
    return Qe(e, this), this._isDisposed ? xt.DISABLE_DISPOSED_WARNING || console.warn(new Error("Trying to add a disposable to a DisposableStore that has already been disposed of. The added object will be leaked!").stack) : this._toDispose.add(e), e;
  }
  delete(e) {
    if (e) {
      if (e === this)
        throw new Error("Cannot dispose a disposable on itself!");
      this._toDispose.delete(e), e.dispose();
    }
  }
  deleteAndLeak(e) {
    e && this._toDispose.has(e) && (this._toDispose.delete(e), Qe(e, null));
  }
};
xt.DISABLE_DISPOSED_WARNING = false;
var fe = xt;
var B = class {
  constructor() {
    this._store = new fe;
    Et(this), Qe(this._store, this);
  }
  dispose() {
    yt(this), this._store.dispose();
  }
  _register(e) {
    if (e === this)
      throw new Error("Cannot register a disposable on itself!");
    return this._store.add(e);
  }
};
B.None = Object.freeze({ dispose() {} });
var be = class {
  constructor() {
    this._isDisposed = false;
    Et(this);
  }
  get value() {
    return this._isDisposed ? undefined : this._value;
  }
  set value(e) {
    this._isDisposed || e === this._value || (this._value?.dispose(), e && Qe(e, this), this._value = e);
  }
  clear() {
    this.value = undefined;
  }
  dispose() {
    this._isDisposed = true, yt(this), this._value?.dispose(), this._value = undefined;
  }
  clearAndLeak() {
    let e = this._value;
    return this._value = undefined, e && Qe(e, null), e;
  }
};
var Lt = typeof process < "u" && "title" in process;
var Ze = Lt ? "node" : navigator.userAgent;
var bi = Lt ? "node" : navigator.platform;
var cn = Ze.includes("Firefox");
var dn = Ze.includes("Edge");
var vi = /^((?!chrome|android).)*safari/i.test(Ze);
function hn() {
  if (!vi)
    return 0;
  let i = Ze.match(/Version\/(\d+)/);
  return i === null || i.length < 2 ? 0 : parseInt(i[1]);
}
var oo = ["Macintosh", "MacIntel", "MacPPC", "Mac68K"].includes(bi);
var ao = ["Windows", "Win16", "Win32", "WinCE"].includes(bi);
var lo = bi.indexOf("Linux") >= 0;
var uo = /\bCrOS\b/.test(Ze);
var pn = "";
var K = 0;
var V = 0;
var C = 0;
var U = 0;
var Z = { css: "#00000000", rgba: 0 };
var X;
((n) => {
  function i(s, o, r, a) {
    return a !== undefined ? `#${Oe(s)}${Oe(o)}${Oe(r)}${Oe(a)}` : `#${Oe(s)}${Oe(o)}${Oe(r)}`;
  }
  n.toCss = i;
  function e(s, o, r, a = 255) {
    return (s << 24 | o << 16 | r << 8 | a) >>> 0;
  }
  n.toRgba = e;
  function t(s, o, r, a) {
    return { css: n.toCss(s, o, r, a), rgba: n.toRgba(s, o, r, a) };
  }
  n.toColor = t;
})(X ||= {});
var Ue;
((a) => {
  function i(l, u) {
    if (U = (u.rgba & 255) / 255, U === 1)
      return { css: u.css, rgba: u.rgba };
    let c = u.rgba >> 24 & 255, d = u.rgba >> 16 & 255, h = u.rgba >> 8 & 255, f = l.rgba >> 24 & 255, I = l.rgba >> 16 & 255, L = l.rgba >> 8 & 255;
    K = f + Math.round((c - f) * U), V = I + Math.round((d - I) * U), C = L + Math.round((h - L) * U);
    let M = X.toCss(K, V, C), q = X.toRgba(K, V, C);
    return { css: M, rgba: q };
  }
  a.blend = i;
  function e(l) {
    return (l.rgba & 255) === 255;
  }
  a.isOpaque = e;
  function t(l, u, c) {
    let d = Te.ensureContrastRatio(l.rgba, u.rgba, c);
    if (d)
      return X.toColor(d >> 24 & 255, d >> 16 & 255, d >> 8 & 255);
  }
  a.ensureContrastRatio = t;
  function n(l) {
    let u = (l.rgba | 255) >>> 0;
    return [K, V, C] = Te.toChannels(u), { css: X.toCss(K, V, C), rgba: u };
  }
  a.opaque = n;
  function s(l, u) {
    return U = Math.round(u * 255), [K, V, C] = Te.toChannels(l.rgba), { css: X.toCss(K, V, C, U), rgba: X.toRgba(K, V, C, U) };
  }
  a.opacity = s;
  function o(l, u) {
    return U = l.rgba & 255, s(l, U * u / 255);
  }
  a.multiplyOpacity = o;
  function r(l) {
    return [l.rgba >> 24 & 255, l.rgba >> 16 & 255, l.rgba >> 8 & 255];
  }
  a.toColorRGB = r;
})(Ue ||= {});
var Fr;
((n) => {
  let i, e;
  try {
    let s = document.createElement("canvas");
    s.width = 1, s.height = 1;
    let o = s.getContext("2d", { willReadFrequently: true });
    o && (i = o, i.globalCompositeOperation = "copy", e = i.createLinearGradient(0, 0, 1, 1));
  } catch {}
  function t(s) {
    if (s.match(/#[\da-f]{3,8}/i))
      switch (s.length) {
        case 4:
          return K = parseInt(s.slice(1, 2).repeat(2), 16), V = parseInt(s.slice(2, 3).repeat(2), 16), C = parseInt(s.slice(3, 4).repeat(2), 16), X.toColor(K, V, C);
        case 5:
          return K = parseInt(s.slice(1, 2).repeat(2), 16), V = parseInt(s.slice(2, 3).repeat(2), 16), C = parseInt(s.slice(3, 4).repeat(2), 16), U = parseInt(s.slice(4, 5).repeat(2), 16), X.toColor(K, V, C, U);
        case 7:
          return { css: s, rgba: (parseInt(s.slice(1), 16) << 8 | 255) >>> 0 };
        case 9:
          return { css: s, rgba: parseInt(s.slice(1), 16) >>> 0 };
      }
    let o = s.match(/rgba?\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*(,\s*(0|1|\d?\.(\d+))\s*)?\)/);
    if (o)
      return K = parseInt(o[1]), V = parseInt(o[2]), C = parseInt(o[3]), U = Math.round((o[5] === undefined ? 1 : parseFloat(o[5])) * 255), X.toColor(K, V, C, U);
    if (!i || !e)
      throw new Error("css.toColor: Unsupported css format");
    if (i.fillStyle = e, i.fillStyle = s, typeof i.fillStyle != "string")
      throw new Error("css.toColor: Unsupported css format");
    if (i.fillRect(0, 0, 1, 1), [K, V, C, U] = i.getImageData(0, 0, 1, 1).data, U !== 255)
      throw new Error("css.toColor: Unsupported css format");
    return { rgba: X.toRgba(K, V, C, U), css: s };
  }
  n.toColor = t;
})(Fr ||= {});
var Y;
((t) => {
  function i(n) {
    return e(n >> 16 & 255, n >> 8 & 255, n & 255);
  }
  t.relativeLuminance = i;
  function e(n, s, o) {
    let r = n / 255, a = s / 255, l = o / 255, u = r <= 0.03928 ? r / 12.92 : Math.pow((r + 0.055) / 1.055, 2.4), c = a <= 0.03928 ? a / 12.92 : Math.pow((a + 0.055) / 1.055, 2.4), d = l <= 0.03928 ? l / 12.92 : Math.pow((l + 0.055) / 1.055, 2.4);
    return u * 0.2126 + c * 0.7152 + d * 0.0722;
  }
  t.relativeLuminance2 = e;
})(Y ||= {});
var Te;
((o) => {
  function i(r, a) {
    if (U = (a & 255) / 255, U === 1)
      return a;
    let l = a >> 24 & 255, u = a >> 16 & 255, c = a >> 8 & 255, d = r >> 24 & 255, h = r >> 16 & 255, f = r >> 8 & 255;
    return K = d + Math.round((l - d) * U), V = h + Math.round((u - h) * U), C = f + Math.round((c - f) * U), X.toRgba(K, V, C);
  }
  o.blend = i;
  function e(r, a, l) {
    let u = Y.relativeLuminance(r >> 8), c = Y.relativeLuminance(a >> 8);
    if (ve(u, c) < l) {
      if (c < u) {
        let I = t(r, a, l), L = ve(u, Y.relativeLuminance(I >> 8));
        if (L < l) {
          let M = n(r, a, l), q = ve(u, Y.relativeLuminance(M >> 8));
          return L > q ? I : M;
        }
        return I;
      }
      let h = n(r, a, l), f = ve(u, Y.relativeLuminance(h >> 8));
      if (f < l) {
        let I = t(r, a, l), L = ve(u, Y.relativeLuminance(I >> 8));
        return f > L ? h : I;
      }
      return h;
    }
  }
  o.ensureContrastRatio = e;
  function t(r, a, l) {
    let u = r >> 24 & 255, c = r >> 16 & 255, d = r >> 8 & 255, h = a >> 24 & 255, f = a >> 16 & 255, I = a >> 8 & 255, L = ve(Y.relativeLuminance2(h, f, I), Y.relativeLuminance2(u, c, d));
    for (;L < l && (h > 0 || f > 0 || I > 0); )
      h -= Math.max(0, Math.ceil(h * 0.1)), f -= Math.max(0, Math.ceil(f * 0.1)), I -= Math.max(0, Math.ceil(I * 0.1)), L = ve(Y.relativeLuminance2(h, f, I), Y.relativeLuminance2(u, c, d));
    return (h << 24 | f << 16 | I << 8 | 255) >>> 0;
  }
  o.reduceLuminance = t;
  function n(r, a, l) {
    let u = r >> 24 & 255, c = r >> 16 & 255, d = r >> 8 & 255, h = a >> 24 & 255, f = a >> 16 & 255, I = a >> 8 & 255, L = ve(Y.relativeLuminance2(h, f, I), Y.relativeLuminance2(u, c, d));
    for (;L < l && (h < 255 || f < 255 || I < 255); )
      h = Math.min(255, h + Math.ceil((255 - h) * 0.1)), f = Math.min(255, f + Math.ceil((255 - f) * 0.1)), I = Math.min(255, I + Math.ceil((255 - I) * 0.1)), L = ve(Y.relativeLuminance2(h, f, I), Y.relativeLuminance2(u, c, d));
    return (h << 24 | f << 16 | I << 8 | 255) >>> 0;
  }
  o.increaseLuminance = n;
  function s(r) {
    return [r >> 24 & 255, r >> 16 & 255, r >> 8 & 255, r & 255];
  }
  o.toChannels = s;
})(Te ||= {});
function Oe(i) {
  let e = i.toString(16);
  return e.length < 2 ? "0" + e : e;
}
function ve(i, e) {
  return i < e ? (e + 0.05) / (i + 0.05) : (i + 0.05) / (e + 0.05);
}
function F(i) {
  if (!i)
    throw new Error("value must not be falsy");
  return i;
}
function Rt(i) {
  return 57508 <= i && i <= 57558;
}
function fn(i) {
  return 57520 <= i && i <= 57527;
}
function kr(i) {
  return 57344 <= i && i <= 63743;
}
function Pr(i) {
  return 9472 <= i && i <= 9631;
}
function Br(i) {
  return i >= 128512 && i <= 128591 || i >= 127744 && i <= 128511 || i >= 128640 && i <= 128767 || i >= 9728 && i <= 9983 || i >= 9984 && i <= 10175 || i >= 65024 && i <= 65039 || i >= 129280 && i <= 129535 || i >= 127462 && i <= 127487;
}
function mn(i, e, t, n) {
  return e === 1 && t > Math.ceil(n * 1.5) && i !== undefined && i > 255 && !Br(i) && !Rt(i) && !kr(i);
}
function Dt(i) {
  return Rt(i) || Pr(i);
}
function _n() {
  return { css: { canvas: wt(), cell: wt() }, device: { canvas: wt(), cell: wt(), char: { width: 0, height: 0, left: 0, top: 0 } } };
}
function wt() {
  return { width: 0, height: 0 };
}
function bn(i, e, t = 0) {
  return (i - (Math.round(e) * 2 - t)) % (Math.round(e) * 2);
}
var j = 0;
var z = 0;
var me = false;
var ge = false;
var Mt = false;
var J;
var Ti = 0;
var At = class {
  constructor(e, t, n, s, o, r) {
    this._terminal = e;
    this._optionService = t;
    this._selectionRenderModel = n;
    this._decorationService = s;
    this._coreBrowserService = o;
    this._themeService = r;
    this.result = { fg: 0, bg: 0, ext: 0 };
  }
  resolve(e, t, n, s) {
    if (this.result.bg = e.bg, this.result.fg = e.fg, this.result.ext = e.bg & 268435456 ? e.extended.ext : 0, z = 0, j = 0, ge = false, me = false, Mt = false, J = this._themeService.colors, Ti = 0, e.getCode() !== 0 && e.extended.underlineStyle === 4) {
      let r = Math.max(1, Math.floor(this._optionService.rawOptions.fontSize * this._coreBrowserService.dpr / 15));
      Ti = t * s % (Math.round(r) * 2);
    }
    if (this._decorationService.forEachDecorationAtCell(t, n, "bottom", (r) => {
      r.backgroundColorRGB && (z = r.backgroundColorRGB.rgba >> 8 & 16777215, ge = true), r.foregroundColorRGB && (j = r.foregroundColorRGB.rgba >> 8 & 16777215, me = true);
    }), Mt = this._selectionRenderModel.isCellSelected(this._terminal, t, n), Mt) {
      if (this.result.fg & 67108864 || (this.result.bg & 50331648) !== 0) {
        if (this.result.fg & 67108864)
          switch (this.result.fg & 50331648) {
            case 16777216:
            case 33554432:
              z = this._themeService.colors.ansi[this.result.fg & 255].rgba;
              break;
            case 50331648:
              z = (this.result.fg & 16777215) << 8 | 255;
              break;
            case 0:
            default:
              z = this._themeService.colors.foreground.rgba;
          }
        else
          switch (this.result.bg & 50331648) {
            case 16777216:
            case 33554432:
              z = this._themeService.colors.ansi[this.result.bg & 255].rgba;
              break;
            case 50331648:
              z = (this.result.bg & 16777215) << 8 | 255;
              break;
          }
        z = Te.blend(z, (this._coreBrowserService.isFocused ? J.selectionBackgroundOpaque : J.selectionInactiveBackgroundOpaque).rgba & 4294967040 | 128) >> 8 & 16777215;
      } else
        z = (this._coreBrowserService.isFocused ? J.selectionBackgroundOpaque : J.selectionInactiveBackgroundOpaque).rgba >> 8 & 16777215;
      if (ge = true, J.selectionForeground && (j = J.selectionForeground.rgba >> 8 & 16777215, me = true), Dt(e.getCode())) {
        if (this.result.fg & 67108864 && (this.result.bg & 50331648) === 0)
          j = (this._coreBrowserService.isFocused ? J.selectionBackgroundOpaque : J.selectionInactiveBackgroundOpaque).rgba >> 8 & 16777215;
        else {
          if (this.result.fg & 67108864)
            switch (this.result.bg & 50331648) {
              case 16777216:
              case 33554432:
                j = this._themeService.colors.ansi[this.result.bg & 255].rgba;
                break;
              case 50331648:
                j = (this.result.bg & 16777215) << 8 | 255;
                break;
            }
          else
            switch (this.result.fg & 50331648) {
              case 16777216:
              case 33554432:
                j = this._themeService.colors.ansi[this.result.fg & 255].rgba;
                break;
              case 50331648:
                j = (this.result.fg & 16777215) << 8 | 255;
                break;
              case 0:
              default:
                j = this._themeService.colors.foreground.rgba;
            }
          j = Te.blend(j, (this._coreBrowserService.isFocused ? J.selectionBackgroundOpaque : J.selectionInactiveBackgroundOpaque).rgba & 4294967040 | 128) >> 8 & 16777215;
        }
        me = true;
      }
    }
    this._decorationService.forEachDecorationAtCell(t, n, "top", (r) => {
      r.backgroundColorRGB && (z = r.backgroundColorRGB.rgba >> 8 & 16777215, ge = true), r.foregroundColorRGB && (j = r.foregroundColorRGB.rgba >> 8 & 16777215, me = true);
    }), ge && (Mt ? z = e.bg & -16777216 & -134217729 | z | 50331648 : z = e.bg & -16777216 | z | 50331648), me && (j = e.fg & -16777216 & -67108865 | j | 50331648), this.result.fg & 67108864 && (ge && !me && ((this.result.bg & 50331648) === 0 ? j = this.result.fg & -134217728 | J.background.rgba >> 8 & 16777215 & 16777215 | 50331648 : j = this.result.fg & -134217728 | this.result.bg & 67108863, me = true), !ge && me && ((this.result.fg & 50331648) === 0 ? z = this.result.bg & -67108864 | J.foreground.rgba >> 8 & 16777215 & 16777215 | 50331648 : z = this.result.bg & -67108864 | this.result.fg & 67108863, ge = true)), J = undefined, this.result.bg = ge ? z : this.result.bg, this.result.fg = me ? j : this.result.fg, this.result.ext &= 536870911, this.result.ext |= Ti << 29 & 3758096384;
  }
};
var gn = 0.5;
var St = cn || dn ? "bottom" : "ideographic";
var Hr = { "▀": [{ x: 0, y: 0, w: 8, h: 4 }], "▁": [{ x: 0, y: 7, w: 8, h: 1 }], "▂": [{ x: 0, y: 6, w: 8, h: 2 }], "▃": [{ x: 0, y: 5, w: 8, h: 3 }], "▄": [{ x: 0, y: 4, w: 8, h: 4 }], "▅": [{ x: 0, y: 3, w: 8, h: 5 }], "▆": [{ x: 0, y: 2, w: 8, h: 6 }], "▇": [{ x: 0, y: 1, w: 8, h: 7 }], "█": [{ x: 0, y: 0, w: 8, h: 8 }], "▉": [{ x: 0, y: 0, w: 7, h: 8 }], "▊": [{ x: 0, y: 0, w: 6, h: 8 }], "▋": [{ x: 0, y: 0, w: 5, h: 8 }], "▌": [{ x: 0, y: 0, w: 4, h: 8 }], "▍": [{ x: 0, y: 0, w: 3, h: 8 }], "▎": [{ x: 0, y: 0, w: 2, h: 8 }], "▏": [{ x: 0, y: 0, w: 1, h: 8 }], "▐": [{ x: 4, y: 0, w: 4, h: 8 }], "▔": [{ x: 0, y: 0, w: 8, h: 1 }], "▕": [{ x: 7, y: 0, w: 1, h: 8 }], "▖": [{ x: 0, y: 4, w: 4, h: 4 }], "▗": [{ x: 4, y: 4, w: 4, h: 4 }], "▘": [{ x: 0, y: 0, w: 4, h: 4 }], "▙": [{ x: 0, y: 0, w: 4, h: 8 }, { x: 0, y: 4, w: 8, h: 4 }], "▚": [{ x: 0, y: 0, w: 4, h: 4 }, { x: 4, y: 4, w: 4, h: 4 }], "▛": [{ x: 0, y: 0, w: 4, h: 8 }, { x: 4, y: 0, w: 4, h: 4 }], "▜": [{ x: 0, y: 0, w: 8, h: 4 }, { x: 4, y: 0, w: 4, h: 8 }], "▝": [{ x: 4, y: 0, w: 4, h: 4 }], "▞": [{ x: 4, y: 0, w: 4, h: 4 }, { x: 0, y: 4, w: 4, h: 4 }], "▟": [{ x: 4, y: 0, w: 4, h: 8 }, { x: 0, y: 4, w: 8, h: 4 }], "\uD83E\uDF70": [{ x: 1, y: 0, w: 1, h: 8 }], "\uD83E\uDF71": [{ x: 2, y: 0, w: 1, h: 8 }], "\uD83E\uDF72": [{ x: 3, y: 0, w: 1, h: 8 }], "\uD83E\uDF73": [{ x: 4, y: 0, w: 1, h: 8 }], "\uD83E\uDF74": [{ x: 5, y: 0, w: 1, h: 8 }], "\uD83E\uDF75": [{ x: 6, y: 0, w: 1, h: 8 }], "\uD83E\uDF76": [{ x: 0, y: 1, w: 8, h: 1 }], "\uD83E\uDF77": [{ x: 0, y: 2, w: 8, h: 1 }], "\uD83E\uDF78": [{ x: 0, y: 3, w: 8, h: 1 }], "\uD83E\uDF79": [{ x: 0, y: 4, w: 8, h: 1 }], "\uD83E\uDF7A": [{ x: 0, y: 5, w: 8, h: 1 }], "\uD83E\uDF7B": [{ x: 0, y: 6, w: 8, h: 1 }], "\uD83E\uDF7C": [{ x: 0, y: 0, w: 1, h: 8 }, { x: 0, y: 7, w: 8, h: 1 }], "\uD83E\uDF7D": [{ x: 0, y: 0, w: 1, h: 8 }, { x: 0, y: 0, w: 8, h: 1 }], "\uD83E\uDF7E": [{ x: 7, y: 0, w: 1, h: 8 }, { x: 0, y: 0, w: 8, h: 1 }], "\uD83E\uDF7F": [{ x: 7, y: 0, w: 1, h: 8 }, { x: 0, y: 7, w: 8, h: 1 }], "\uD83E\uDF80": [{ x: 0, y: 0, w: 8, h: 1 }, { x: 0, y: 7, w: 8, h: 1 }], "\uD83E\uDF81": [{ x: 0, y: 0, w: 8, h: 1 }, { x: 0, y: 2, w: 8, h: 1 }, { x: 0, y: 4, w: 8, h: 1 }, { x: 0, y: 7, w: 8, h: 1 }], "\uD83E\uDF82": [{ x: 0, y: 0, w: 8, h: 2 }], "\uD83E\uDF83": [{ x: 0, y: 0, w: 8, h: 3 }], "\uD83E\uDF84": [{ x: 0, y: 0, w: 8, h: 5 }], "\uD83E\uDF85": [{ x: 0, y: 0, w: 8, h: 6 }], "\uD83E\uDF86": [{ x: 0, y: 0, w: 8, h: 7 }], "\uD83E\uDF87": [{ x: 6, y: 0, w: 2, h: 8 }], "\uD83E\uDF88": [{ x: 5, y: 0, w: 3, h: 8 }], "\uD83E\uDF89": [{ x: 3, y: 0, w: 5, h: 8 }], "\uD83E\uDF8A": [{ x: 2, y: 0, w: 6, h: 8 }], "\uD83E\uDF8B": [{ x: 1, y: 0, w: 7, h: 8 }], "\uD83E\uDF95": [{ x: 0, y: 0, w: 2, h: 2 }, { x: 4, y: 0, w: 2, h: 2 }, { x: 2, y: 2, w: 2, h: 2 }, { x: 6, y: 2, w: 2, h: 2 }, { x: 0, y: 4, w: 2, h: 2 }, { x: 4, y: 4, w: 2, h: 2 }, { x: 2, y: 6, w: 2, h: 2 }, { x: 6, y: 6, w: 2, h: 2 }], "\uD83E\uDF96": [{ x: 2, y: 0, w: 2, h: 2 }, { x: 6, y: 0, w: 2, h: 2 }, { x: 0, y: 2, w: 2, h: 2 }, { x: 4, y: 2, w: 2, h: 2 }, { x: 2, y: 4, w: 2, h: 2 }, { x: 6, y: 4, w: 2, h: 2 }, { x: 0, y: 6, w: 2, h: 2 }, { x: 4, y: 6, w: 2, h: 2 }], "\uD83E\uDF97": [{ x: 0, y: 2, w: 8, h: 2 }, { x: 0, y: 6, w: 8, h: 2 }] };
var Wr = { "░": [[1, 0, 0, 0], [0, 0, 0, 0], [0, 0, 1, 0], [0, 0, 0, 0]], "▒": [[1, 0], [0, 0], [0, 1], [0, 0]], "▓": [[0, 1], [1, 1], [1, 0], [1, 1]] };
var Gr = { "─": { 1: "M0,.5 L1,.5" }, "━": { 3: "M0,.5 L1,.5" }, "│": { 1: "M.5,0 L.5,1" }, "┃": { 3: "M.5,0 L.5,1" }, "┌": { 1: "M0.5,1 L.5,.5 L1,.5" }, "┏": { 3: "M0.5,1 L.5,.5 L1,.5" }, "┐": { 1: "M0,.5 L.5,.5 L.5,1" }, "┓": { 3: "M0,.5 L.5,.5 L.5,1" }, "└": { 1: "M.5,0 L.5,.5 L1,.5" }, "┗": { 3: "M.5,0 L.5,.5 L1,.5" }, "┘": { 1: "M.5,0 L.5,.5 L0,.5" }, "┛": { 3: "M.5,0 L.5,.5 L0,.5" }, "├": { 1: "M.5,0 L.5,1 M.5,.5 L1,.5" }, "┣": { 3: "M.5,0 L.5,1 M.5,.5 L1,.5" }, "┤": { 1: "M.5,0 L.5,1 M.5,.5 L0,.5" }, "┫": { 3: "M.5,0 L.5,1 M.5,.5 L0,.5" }, "┬": { 1: "M0,.5 L1,.5 M.5,.5 L.5,1" }, "┳": { 3: "M0,.5 L1,.5 M.5,.5 L.5,1" }, "┴": { 1: "M0,.5 L1,.5 M.5,.5 L.5,0" }, "┻": { 3: "M0,.5 L1,.5 M.5,.5 L.5,0" }, "┼": { 1: "M0,.5 L1,.5 M.5,0 L.5,1" }, "╋": { 3: "M0,.5 L1,.5 M.5,0 L.5,1" }, "╴": { 1: "M.5,.5 L0,.5" }, "╸": { 3: "M.5,.5 L0,.5" }, "╵": { 1: "M.5,.5 L.5,0" }, "╹": { 3: "M.5,.5 L.5,0" }, "╶": { 1: "M.5,.5 L1,.5" }, "╺": { 3: "M.5,.5 L1,.5" }, "╷": { 1: "M.5,.5 L.5,1" }, "╻": { 3: "M.5,.5 L.5,1" }, "═": { 1: (i, e) => `M0,${0.5 - e} L1,${0.5 - e} M0,${0.5 + e} L1,${0.5 + e}` }, "║": { 1: (i, e) => `M${0.5 - i},0 L${0.5 - i},1 M${0.5 + i},0 L${0.5 + i},1` }, "╒": { 1: (i, e) => `M.5,1 L.5,${0.5 - e} L1,${0.5 - e} M.5,${0.5 + e} L1,${0.5 + e}` }, "╓": { 1: (i, e) => `M${0.5 - i},1 L${0.5 - i},.5 L1,.5 M${0.5 + i},.5 L${0.5 + i},1` }, "╔": { 1: (i, e) => `M1,${0.5 - e} L${0.5 - i},${0.5 - e} L${0.5 - i},1 M1,${0.5 + e} L${0.5 + i},${0.5 + e} L${0.5 + i},1` }, "╕": { 1: (i, e) => `M0,${0.5 - e} L.5,${0.5 - e} L.5,1 M0,${0.5 + e} L.5,${0.5 + e}` }, "╖": { 1: (i, e) => `M${0.5 + i},1 L${0.5 + i},.5 L0,.5 M${0.5 - i},.5 L${0.5 - i},1` }, "╗": { 1: (i, e) => `M0,${0.5 + e} L${0.5 - i},${0.5 + e} L${0.5 - i},1 M0,${0.5 - e} L${0.5 + i},${0.5 - e} L${0.5 + i},1` }, "╘": { 1: (i, e) => `M.5,0 L.5,${0.5 + e} L1,${0.5 + e} M.5,${0.5 - e} L1,${0.5 - e}` }, "╙": { 1: (i, e) => `M1,.5 L${0.5 - i},.5 L${0.5 - i},0 M${0.5 + i},.5 L${0.5 + i},0` }, "╚": { 1: (i, e) => `M1,${0.5 - e} L${0.5 + i},${0.5 - e} L${0.5 + i},0 M1,${0.5 + e} L${0.5 - i},${0.5 + e} L${0.5 - i},0` }, "╛": { 1: (i, e) => `M0,${0.5 + e} L.5,${0.5 + e} L.5,0 M0,${0.5 - e} L.5,${0.5 - e}` }, "╜": { 1: (i, e) => `M0,.5 L${0.5 + i},.5 L${0.5 + i},0 M${0.5 - i},.5 L${0.5 - i},0` }, "╝": { 1: (i, e) => `M0,${0.5 - e} L${0.5 - i},${0.5 - e} L${0.5 - i},0 M0,${0.5 + e} L${0.5 + i},${0.5 + e} L${0.5 + i},0` }, "╞": { 1: (i, e) => `M.5,0 L.5,1 M.5,${0.5 - e} L1,${0.5 - e} M.5,${0.5 + e} L1,${0.5 + e}` }, "╟": { 1: (i, e) => `M${0.5 - i},0 L${0.5 - i},1 M${0.5 + i},0 L${0.5 + i},1 M${0.5 + i},.5 L1,.5` }, "╠": { 1: (i, e) => `M${0.5 - i},0 L${0.5 - i},1 M1,${0.5 + e} L${0.5 + i},${0.5 + e} L${0.5 + i},1 M1,${0.5 - e} L${0.5 + i},${0.5 - e} L${0.5 + i},0` }, "╡": { 1: (i, e) => `M.5,0 L.5,1 M0,${0.5 - e} L.5,${0.5 - e} M0,${0.5 + e} L.5,${0.5 + e}` }, "╢": { 1: (i, e) => `M0,.5 L${0.5 - i},.5 M${0.5 - i},0 L${0.5 - i},1 M${0.5 + i},0 L${0.5 + i},1` }, "╣": { 1: (i, e) => `M${0.5 + i},0 L${0.5 + i},1 M0,${0.5 + e} L${0.5 - i},${0.5 + e} L${0.5 - i},1 M0,${0.5 - e} L${0.5 - i},${0.5 - e} L${0.5 - i},0` }, "╤": { 1: (i, e) => `M0,${0.5 - e} L1,${0.5 - e} M0,${0.5 + e} L1,${0.5 + e} M.5,${0.5 + e} L.5,1` }, "╥": { 1: (i, e) => `M0,.5 L1,.5 M${0.5 - i},.5 L${0.5 - i},1 M${0.5 + i},.5 L${0.5 + i},1` }, "╦": { 1: (i, e) => `M0,${0.5 - e} L1,${0.5 - e} M0,${0.5 + e} L${0.5 - i},${0.5 + e} L${0.5 - i},1 M1,${0.5 + e} L${0.5 + i},${0.5 + e} L${0.5 + i},1` }, "╧": { 1: (i, e) => `M.5,0 L.5,${0.5 - e} M0,${0.5 - e} L1,${0.5 - e} M0,${0.5 + e} L1,${0.5 + e}` }, "╨": { 1: (i, e) => `M0,.5 L1,.5 M${0.5 - i},.5 L${0.5 - i},0 M${0.5 + i},.5 L${0.5 + i},0` }, "╩": { 1: (i, e) => `M0,${0.5 + e} L1,${0.5 + e} M0,${0.5 - e} L${0.5 - i},${0.5 - e} L${0.5 - i},0 M1,${0.5 - e} L${0.5 + i},${0.5 - e} L${0.5 + i},0` }, "╪": { 1: (i, e) => `M.5,0 L.5,1 M0,${0.5 - e} L1,${0.5 - e} M0,${0.5 + e} L1,${0.5 + e}` }, "╫": { 1: (i, e) => `M0,.5 L1,.5 M${0.5 - i},0 L${0.5 - i},1 M${0.5 + i},0 L${0.5 + i},1` }, "╬": { 1: (i, e) => `M0,${0.5 + e} L${0.5 - i},${0.5 + e} L${0.5 - i},1 M1,${0.5 + e} L${0.5 + i},${0.5 + e} L${0.5 + i},1 M0,${0.5 - e} L${0.5 - i},${0.5 - e} L${0.5 - i},0 M1,${0.5 - e} L${0.5 + i},${0.5 - e} L${0.5 + i},0` }, "╱": { 1: "M1,0 L0,1" }, "╲": { 1: "M0,0 L1,1" }, "╳": { 1: "M1,0 L0,1 M0,0 L1,1" }, "╼": { 1: "M.5,.5 L0,.5", 3: "M.5,.5 L1,.5" }, "╽": { 1: "M.5,.5 L.5,0", 3: "M.5,.5 L.5,1" }, "╾": { 1: "M.5,.5 L1,.5", 3: "M.5,.5 L0,.5" }, "╿": { 1: "M.5,.5 L.5,1", 3: "M.5,.5 L.5,0" }, "┍": { 1: "M.5,.5 L.5,1", 3: "M.5,.5 L1,.5" }, "┎": { 1: "M.5,.5 L1,.5", 3: "M.5,.5 L.5,1" }, "┑": { 1: "M.5,.5 L.5,1", 3: "M.5,.5 L0,.5" }, "┒": { 1: "M.5,.5 L0,.5", 3: "M.5,.5 L.5,1" }, "┕": { 1: "M.5,.5 L.5,0", 3: "M.5,.5 L1,.5" }, "┖": { 1: "M.5,.5 L1,.5", 3: "M.5,.5 L.5,0" }, "┙": { 1: "M.5,.5 L.5,0", 3: "M.5,.5 L0,.5" }, "┚": { 1: "M.5,.5 L0,.5", 3: "M.5,.5 L.5,0" }, "┝": { 1: "M.5,0 L.5,1", 3: "M.5,.5 L1,.5" }, "┞": { 1: "M0.5,1 L.5,.5 L1,.5", 3: "M.5,.5 L.5,0" }, "┟": { 1: "M.5,0 L.5,.5 L1,.5", 3: "M.5,.5 L.5,1" }, "┠": { 1: "M.5,.5 L1,.5", 3: "M.5,0 L.5,1" }, "┡": { 1: "M.5,.5 L.5,1", 3: "M.5,0 L.5,.5 L1,.5" }, "┢": { 1: "M.5,.5 L.5,0", 3: "M0.5,1 L.5,.5 L1,.5" }, "┥": { 1: "M.5,0 L.5,1", 3: "M.5,.5 L0,.5" }, "┦": { 1: "M0,.5 L.5,.5 L.5,1", 3: "M.5,.5 L.5,0" }, "┧": { 1: "M.5,0 L.5,.5 L0,.5", 3: "M.5,.5 L.5,1" }, "┨": { 1: "M.5,.5 L0,.5", 3: "M.5,0 L.5,1" }, "┩": { 1: "M.5,.5 L.5,1", 3: "M.5,0 L.5,.5 L0,.5" }, "┪": { 1: "M.5,.5 L.5,0", 3: "M0,.5 L.5,.5 L.5,1" }, "┭": { 1: "M0.5,1 L.5,.5 L1,.5", 3: "M.5,.5 L0,.5" }, "┮": { 1: "M0,.5 L.5,.5 L.5,1", 3: "M.5,.5 L1,.5" }, "┯": { 1: "M.5,.5 L.5,1", 3: "M0,.5 L1,.5" }, "┰": { 1: "M0,.5 L1,.5", 3: "M.5,.5 L.5,1" }, "┱": { 1: "M.5,.5 L1,.5", 3: "M0,.5 L.5,.5 L.5,1" }, "┲": { 1: "M.5,.5 L0,.5", 3: "M0.5,1 L.5,.5 L1,.5" }, "┵": { 1: "M.5,0 L.5,.5 L1,.5", 3: "M.5,.5 L0,.5" }, "┶": { 1: "M.5,0 L.5,.5 L0,.5", 3: "M.5,.5 L1,.5" }, "┷": { 1: "M.5,.5 L.5,0", 3: "M0,.5 L1,.5" }, "┸": { 1: "M0,.5 L1,.5", 3: "M.5,.5 L.5,0" }, "┹": { 1: "M.5,.5 L1,.5", 3: "M.5,0 L.5,.5 L0,.5" }, "┺": { 1: "M.5,.5 L0,.5", 3: "M.5,0 L.5,.5 L1,.5" }, "┽": { 1: "M.5,0 L.5,1 M.5,.5 L1,.5", 3: "M.5,.5 L0,.5" }, "┾": { 1: "M.5,0 L.5,1 M.5,.5 L0,.5", 3: "M.5,.5 L1,.5" }, "┿": { 1: "M.5,0 L.5,1", 3: "M0,.5 L1,.5" }, "╀": { 1: "M0,.5 L1,.5 M.5,.5 L.5,1", 3: "M.5,.5 L.5,0" }, "╁": { 1: "M.5,.5 L.5,0 M0,.5 L1,.5", 3: "M.5,.5 L.5,1" }, "╂": { 1: "M0,.5 L1,.5", 3: "M.5,0 L.5,1" }, "╃": { 1: "M0.5,1 L.5,.5 L1,.5", 3: "M.5,0 L.5,.5 L0,.5" }, "╄": { 1: "M0,.5 L.5,.5 L.5,1", 3: "M.5,0 L.5,.5 L1,.5" }, "╅": { 1: "M.5,0 L.5,.5 L1,.5", 3: "M0,.5 L.5,.5 L.5,1" }, "╆": { 1: "M.5,0 L.5,.5 L0,.5", 3: "M0.5,1 L.5,.5 L1,.5" }, "╇": { 1: "M.5,.5 L.5,1", 3: "M.5,.5 L.5,0 M0,.5 L1,.5" }, "╈": { 1: "M.5,.5 L.5,0", 3: "M0,.5 L1,.5 M.5,.5 L.5,1" }, "╉": { 1: "M.5,.5 L1,.5", 3: "M.5,0 L.5,1 M.5,.5 L0,.5" }, "╊": { 1: "M.5,.5 L0,.5", 3: "M.5,0 L.5,1 M.5,.5 L1,.5" }, "╌": { 1: "M.1,.5 L.4,.5 M.6,.5 L.9,.5" }, "╍": { 3: "M.1,.5 L.4,.5 M.6,.5 L.9,.5" }, "┄": { 1: "M.0667,.5 L.2667,.5 M.4,.5 L.6,.5 M.7333,.5 L.9333,.5" }, "┅": { 3: "M.0667,.5 L.2667,.5 M.4,.5 L.6,.5 M.7333,.5 L.9333,.5" }, "┈": { 1: "M.05,.5 L.2,.5 M.3,.5 L.45,.5 M.55,.5 L.7,.5 M.8,.5 L.95,.5" }, "┉": { 3: "M.05,.5 L.2,.5 M.3,.5 L.45,.5 M.55,.5 L.7,.5 M.8,.5 L.95,.5" }, "╎": { 1: "M.5,.1 L.5,.4 M.5,.6 L.5,.9" }, "╏": { 3: "M.5,.1 L.5,.4 M.5,.6 L.5,.9" }, "┆": { 1: "M.5,.0667 L.5,.2667 M.5,.4 L.5,.6 M.5,.7333 L.5,.9333" }, "┇": { 3: "M.5,.0667 L.5,.2667 M.5,.4 L.5,.6 M.5,.7333 L.5,.9333" }, "┊": { 1: "M.5,.05 L.5,.2 M.5,.3 L.5,.45 L.5,.55 M.5,.7 L.5,.95" }, "┋": { 3: "M.5,.05 L.5,.2 M.5,.3 L.5,.45 L.5,.55 M.5,.7 L.5,.95" }, "╭": { 1: (i, e) => `M.5,1 L.5,${0.5 + e / 0.15 * 0.5} C.5,${0.5 + e / 0.15 * 0.5},.5,.5,1,.5` }, "╮": { 1: (i, e) => `M.5,1 L.5,${0.5 + e / 0.15 * 0.5} C.5,${0.5 + e / 0.15 * 0.5},.5,.5,0,.5` }, "╯": { 1: (i, e) => `M.5,0 L.5,${0.5 - e / 0.15 * 0.5} C.5,${0.5 - e / 0.15 * 0.5},.5,.5,0,.5` }, "╰": { 1: (i, e) => `M.5,0 L.5,${0.5 - e / 0.15 * 0.5} C.5,${0.5 - e / 0.15 * 0.5},.5,.5,1,.5` } };
var et = { "": { d: "M.3,1 L.03,1 L.03,.88 C.03,.82,.06,.78,.11,.73 C.15,.7,.2,.68,.28,.65 L.43,.6 C.49,.58,.53,.56,.56,.53 C.59,.5,.6,.47,.6,.43 L.6,.27 L.4,.27 L.69,.1 L.98,.27 L.78,.27 L.78,.46 C.78,.52,.76,.56,.72,.61 C.68,.66,.63,.67,.56,.7 L.48,.72 C.42,.74,.38,.76,.35,.78 C.32,.8,.31,.84,.31,.88 L.31,1 M.3,.5 L.03,.59 L.03,.09 L.3,.09 L.3,.655", type: 0 }, "": { d: "M.7,.4 L.7,.47 L.2,.47 L.2,.03 L.355,.03 L.355,.4 L.705,.4 M.7,.5 L.86,.5 L.86,.95 L.69,.95 L.44,.66 L.46,.86 L.46,.95 L.3,.95 L.3,.49 L.46,.49 L.71,.78 L.69,.565 L.69,.5", type: 0 }, "": { d: "M.25,.94 C.16,.94,.11,.92,.11,.87 L.11,.53 C.11,.48,.15,.455,.23,.45 L.23,.3 C.23,.25,.26,.22,.31,.19 C.36,.16,.43,.15,.51,.15 C.59,.15,.66,.16,.71,.19 C.77,.22,.79,.26,.79,.3 L.79,.45 C.87,.45,.91,.48,.91,.53 L.91,.87 C.91,.92,.86,.94,.77,.94 L.24,.94 M.53,.2 C.49,.2,.45,.21,.42,.23 C.39,.25,.38,.27,.38,.3 L.38,.45 L.68,.45 L.68,.3 C.68,.27,.67,.25,.64,.23 C.61,.21,.58,.2,.53,.2 M.58,.82 L.58,.66 C.63,.65,.65,.63,.65,.6 C.65,.58,.64,.57,.61,.56 C.58,.55,.56,.54,.52,.54 C.48,.54,.46,.55,.43,.56 C.4,.57,.39,.59,.39,.6 C.39,.63,.41,.64,.46,.66 L.46,.82 L.57,.82", type: 0 }, "": { d: "M0,0 L1,.5 L0,1", type: 0, rightPadding: 2 }, "": { d: "M-1,-.5 L1,.5 L-1,1.5", type: 1, leftPadding: 1, rightPadding: 1 }, "": { d: "M1,0 L0,.5 L1,1", type: 0, leftPadding: 2 }, "": { d: "M2,-.5 L0,.5 L2,1.5", type: 1, leftPadding: 1, rightPadding: 1 }, "": { d: "M0,0 L0,1 C0.552,1,1,0.776,1,.5 C1,0.224,0.552,0,0,0", type: 0, rightPadding: 1 }, "": { d: "M.2,1 C.422,1,.8,.826,.78,.5 C.8,.174,0.422,0,.2,0", type: 1, rightPadding: 1 }, "": { d: "M1,0 L1,1 C0.448,1,0,0.776,0,.5 C0,0.224,0.448,0,1,0", type: 0, leftPadding: 1 }, "": { d: "M.8,1 C0.578,1,0.2,.826,.22,.5 C0.2,0.174,0.578,0,0.8,0", type: 1, leftPadding: 1 }, "": { d: "M-.5,-.5 L1.5,1.5 L-.5,1.5", type: 0 }, "": { d: "M-.5,-.5 L1.5,1.5", type: 1, leftPadding: 1, rightPadding: 1 }, "": { d: "M1.5,-.5 L-.5,1.5 L1.5,1.5", type: 0 }, "": { d: "M1.5,-.5 L-.5,1.5 L-.5,-.5", type: 0 }, "": { d: "M1.5,-.5 L-.5,1.5", type: 1, leftPadding: 1, rightPadding: 1 }, "": { d: "M-.5,-.5 L1.5,1.5 L1.5,-.5", type: 0 } };
et[""] = et[""];
et[""] = et[""];
function yn(i, e, t, n, s, o, r, a) {
  let l = Hr[e];
  if (l)
    return $r(i, l, t, n, s, o), true;
  let u = Wr[e];
  if (u)
    return Kr(i, u, t, n, s, o), true;
  let c = Gr[e];
  if (c)
    return Vr(i, c, t, n, s, o, a), true;
  let d = et[e];
  return d ? (Cr(i, d, t, n, s, o, r, a), true) : false;
}
function $r(i, e, t, n, s, o) {
  for (let r = 0;r < e.length; r++) {
    let a = e[r], l = s / 8, u = o / 8;
    i.fillRect(t + a.x * l, n + a.y * u, a.w * l, a.h * u);
  }
}
var xn = new Map;
function Kr(i, e, t, n, s, o) {
  let r = xn.get(e);
  r || (r = new Map, xn.set(e, r));
  let a = i.fillStyle;
  if (typeof a != "string")
    throw new Error(`Unexpected fillStyle type "${a}"`);
  let l = r.get(a);
  if (!l) {
    let u = e[0].length, c = e.length, d = i.canvas.ownerDocument.createElement("canvas");
    d.width = u, d.height = c;
    let h = F(d.getContext("2d")), f = new ImageData(u, c), I, L, M, q;
    if (a.startsWith("#"))
      I = parseInt(a.slice(1, 3), 16), L = parseInt(a.slice(3, 5), 16), M = parseInt(a.slice(5, 7), 16), q = a.length > 7 && parseInt(a.slice(7, 9), 16) || 1;
    else if (a.startsWith("rgba"))
      [I, L, M, q] = a.substring(5, a.length - 1).split(",").map((S) => parseFloat(S));
    else
      throw new Error(`Unexpected fillStyle color format "${a}" when drawing pattern glyph`);
    for (let S = 0;S < c; S++)
      for (let W = 0;W < u; W++)
        f.data[(S * u + W) * 4] = I, f.data[(S * u + W) * 4 + 1] = L, f.data[(S * u + W) * 4 + 2] = M, f.data[(S * u + W) * 4 + 3] = e[S][W] * (q * 255);
    h.putImageData(f, 0, 0), l = F(i.createPattern(d, null)), r.set(a, l);
  }
  i.fillStyle = l, i.fillRect(t, n, s, o);
}
function Vr(i, e, t, n, s, o, r) {
  i.strokeStyle = i.fillStyle;
  for (let [a, l] of Object.entries(e)) {
    i.beginPath(), i.lineWidth = r * Number.parseInt(a);
    let u;
    if (typeof l == "function") {
      let d = 0.15 / o * s;
      u = l(0.15, d);
    } else
      u = l;
    for (let c of u.split(" ")) {
      let d = c[0], h = In[d];
      if (!h) {
        console.error(`Could not find drawing instructions for "${d}"`);
        continue;
      }
      let f = c.substring(1).split(",");
      !f[0] || !f[1] || h(i, Ln(f, s, o, t, n, true, r));
    }
    i.stroke(), i.closePath();
  }
}
function Cr(i, e, t, n, s, o, r, a) {
  let l = new Path2D;
  l.rect(t, n, s, o), i.clip(l), i.beginPath();
  let u = r / 12;
  i.lineWidth = a * u;
  for (let c of e.d.split(" ")) {
    let d = c[0], h = In[d];
    if (!h) {
      console.error(`Could not find drawing instructions for "${d}"`);
      continue;
    }
    let f = c.substring(1).split(",");
    !f[0] || !f[1] || h(i, Ln(f, s, o, t, n, false, a, (e.leftPadding ?? 0) * (u / 2), (e.rightPadding ?? 0) * (u / 2)));
  }
  e.type === 1 ? (i.strokeStyle = i.fillStyle, i.stroke()) : i.fill(), i.closePath();
}
function En(i, e, t = 0) {
  return Math.max(Math.min(i, e), t);
}
var In = { C: (i, e) => i.bezierCurveTo(e[0], e[1], e[2], e[3], e[4], e[5]), L: (i, e) => i.lineTo(e[0], e[1]), M: (i, e) => i.moveTo(e[0], e[1]) };
function Ln(i, e, t, n, s, o, r, a = 0, l = 0) {
  let u = i.map((c) => parseFloat(c) || parseInt(c));
  if (u.length < 2)
    throw new Error("Too few arguments for instruction");
  for (let c = 0;c < u.length; c += 2)
    u[c] *= e - a * r - l * r, o && u[c] !== 0 && (u[c] = En(Math.round(u[c] + 0.5) - 0.5, e, 0)), u[c] += n + a * r;
  for (let c = 1;c < u.length; c += 2)
    u[c] *= t, o && u[c] !== 0 && (u[c] = En(Math.round(u[c] + 0.5) - 0.5, t, 0)), u[c] += s;
  return u;
}
var Ot = class {
  constructor() {
    this._data = {};
  }
  set(e, t, n) {
    this._data[e] || (this._data[e] = {}), this._data[e][t] = n;
  }
  get(e, t) {
    return this._data[e] ? this._data[e][t] : undefined;
  }
  clear() {
    this._data = {};
  }
};
var tt = class {
  constructor() {
    this._data = new Ot;
  }
  set(e, t, n, s, o) {
    this._data.get(e, t) || this._data.set(e, t, new Ot), this._data.get(e, t).set(n, s, o);
  }
  get(e, t, n, s) {
    return this._data.get(e, t)?.get(n, s);
  }
  clear() {
    this._data.clear();
  }
};
var Ft = class {
  constructor() {
    this._tasks = [];
    this._i = 0;
  }
  enqueue(e) {
    this._tasks.push(e), this._start();
  }
  flush() {
    for (;this._i < this._tasks.length; )
      this._tasks[this._i]() || this._i++;
    this.clear();
  }
  clear() {
    this._idleCallback && (this._cancelCallback(this._idleCallback), this._idleCallback = undefined), this._i = 0, this._tasks.length = 0;
  }
  _start() {
    this._idleCallback || (this._idleCallback = this._requestCallback(this._process.bind(this)));
  }
  _process(e) {
    this._idleCallback = undefined;
    let t = 0, n = 0, s = e.timeRemaining(), o = 0;
    for (;this._i < this._tasks.length; ) {
      if (t = performance.now(), this._tasks[this._i]() || this._i++, t = Math.max(1, performance.now() - t), n = Math.max(t, n), o = e.timeRemaining(), n * 1.5 > o) {
        s - t < -20 && console.warn(`task queue exceeded allotted deadline by ${Math.abs(Math.round(s - t))}ms`), this._start();
        return;
      }
      s = o;
    }
    this.clear();
  }
};
var gi = class extends Ft {
  _requestCallback(e) {
    return setTimeout(() => e(this._createDeadline(16)));
  }
  _cancelCallback(e) {
    clearTimeout(e);
  }
  _createDeadline(e) {
    let t = performance.now() + e;
    return { timeRemaining: () => Math.max(0, t - performance.now()) };
  }
};
var xi = class extends Ft {
  _requestCallback(e) {
    return requestIdleCallback(e);
  }
  _cancelCallback(e) {
    cancelIdleCallback(e);
  }
};
var wn = !Lt && "requestIdleCallback" in window ? xi : gi;
var he = class i {
  constructor() {
    this.fg = 0;
    this.bg = 0;
    this.extended = new it;
  }
  static toColorRGB(e) {
    return [e >>> 16 & 255, e >>> 8 & 255, e & 255];
  }
  static fromColorRGB(e) {
    return (e[0] & 255) << 16 | (e[1] & 255) << 8 | e[2] & 255;
  }
  clone() {
    let e = new i;
    return e.fg = this.fg, e.bg = this.bg, e.extended = this.extended.clone(), e;
  }
  isInverse() {
    return this.fg & 67108864;
  }
  isBold() {
    return this.fg & 134217728;
  }
  isUnderline() {
    return this.hasExtendedAttrs() && this.extended.underlineStyle !== 0 ? 1 : this.fg & 268435456;
  }
  isBlink() {
    return this.fg & 536870912;
  }
  isInvisible() {
    return this.fg & 1073741824;
  }
  isItalic() {
    return this.bg & 67108864;
  }
  isDim() {
    return this.bg & 134217728;
  }
  isStrikethrough() {
    return this.fg & 2147483648;
  }
  isProtected() {
    return this.bg & 536870912;
  }
  isOverline() {
    return this.bg & 1073741824;
  }
  getFgColorMode() {
    return this.fg & 50331648;
  }
  getBgColorMode() {
    return this.bg & 50331648;
  }
  isFgRGB() {
    return (this.fg & 50331648) === 50331648;
  }
  isBgRGB() {
    return (this.bg & 50331648) === 50331648;
  }
  isFgPalette() {
    return (this.fg & 50331648) === 16777216 || (this.fg & 50331648) === 33554432;
  }
  isBgPalette() {
    return (this.bg & 50331648) === 16777216 || (this.bg & 50331648) === 33554432;
  }
  isFgDefault() {
    return (this.fg & 50331648) === 0;
  }
  isBgDefault() {
    return (this.bg & 50331648) === 0;
  }
  isAttributeDefault() {
    return this.fg === 0 && this.bg === 0;
  }
  getFgColor() {
    switch (this.fg & 50331648) {
      case 16777216:
      case 33554432:
        return this.fg & 255;
      case 50331648:
        return this.fg & 16777215;
      default:
        return -1;
    }
  }
  getBgColor() {
    switch (this.bg & 50331648) {
      case 16777216:
      case 33554432:
        return this.bg & 255;
      case 50331648:
        return this.bg & 16777215;
      default:
        return -1;
    }
  }
  hasExtendedAttrs() {
    return this.bg & 268435456;
  }
  updateExtended() {
    this.extended.isEmpty() ? this.bg &= -268435457 : this.bg |= 268435456;
  }
  getUnderlineColor() {
    if (this.bg & 268435456 && ~this.extended.underlineColor)
      switch (this.extended.underlineColor & 50331648) {
        case 16777216:
        case 33554432:
          return this.extended.underlineColor & 255;
        case 50331648:
          return this.extended.underlineColor & 16777215;
        default:
          return this.getFgColor();
      }
    return this.getFgColor();
  }
  getUnderlineColorMode() {
    return this.bg & 268435456 && ~this.extended.underlineColor ? this.extended.underlineColor & 50331648 : this.getFgColorMode();
  }
  isUnderlineColorRGB() {
    return this.bg & 268435456 && ~this.extended.underlineColor ? (this.extended.underlineColor & 50331648) === 50331648 : this.isFgRGB();
  }
  isUnderlineColorPalette() {
    return this.bg & 268435456 && ~this.extended.underlineColor ? (this.extended.underlineColor & 50331648) === 16777216 || (this.extended.underlineColor & 50331648) === 33554432 : this.isFgPalette();
  }
  isUnderlineColorDefault() {
    return this.bg & 268435456 && ~this.extended.underlineColor ? (this.extended.underlineColor & 50331648) === 0 : this.isFgDefault();
  }
  getUnderlineStyle() {
    return this.fg & 268435456 ? this.bg & 268435456 ? this.extended.underlineStyle : 1 : 0;
  }
  getUnderlineVariantOffset() {
    return this.extended.underlineVariantOffset;
  }
};
var it = class i {
  constructor(e = 0, t = 0) {
    this._ext = 0;
    this._urlId = 0;
    this._ext = e, this._urlId = t;
  }
  get ext() {
    return this._urlId ? this._ext & -469762049 | this.underlineStyle << 26 : this._ext;
  }
  set ext(e) {
    this._ext = e;
  }
  get underlineStyle() {
    return this._urlId ? 5 : (this._ext & 469762048) >> 26;
  }
  set underlineStyle(e) {
    this._ext &= -469762049, this._ext |= e << 26 & 469762048;
  }
  get underlineColor() {
    return this._ext & 67108863;
  }
  set underlineColor(e) {
    this._ext &= -67108864, this._ext |= e & 67108863;
  }
  get urlId() {
    return this._urlId;
  }
  set urlId(e) {
    this._urlId = e;
  }
  get underlineVariantOffset() {
    let e = (this._ext & 3758096384) >> 29;
    return e < 0 ? e ^ 4294967288 : e;
  }
  set underlineVariantOffset(e) {
    this._ext &= 536870911, this._ext |= e << 29 & 3758096384;
  }
  clone() {
    return new i(this._ext, this._urlId);
  }
  isEmpty() {
    return this.underlineStyle === 0 && this._urlId === 0;
  }
};
var He = class He {
  constructor(e) {
    this.element = e, this.next = He.Undefined, this.prev = He.Undefined;
  }
};
He.Undefined = new He(undefined);
var zr = globalThis.performance && typeof globalThis.performance.now == "function";
var kt = class i {
  static create(e) {
    return new i(e);
  }
  constructor(e) {
    this._now = zr && e === false ? Date.now : globalThis.performance.now.bind(globalThis.performance), this._startTime = this._now(), this._stopTime = -1;
  }
  stop() {
    this._stopTime = this._now();
  }
  reset() {
    this._startTime = this._now(), this._stopTime = -1;
  }
  elapsed() {
    return this._stopTime !== -1 ? this._stopTime - this._startTime : this._now() - this._startTime;
  }
};
var qr = false;
var Dn = false;
var jr = false;
var ee;
((se) => {
  se.None = () => B.None;
  function e(v) {
    if (jr) {
      let { onDidAddListener: p } = v, g = nt.create(), b = 0;
      v.onDidAddListener = () => {
        ++b === 2 && (console.warn("snapshotted emitter LIKELY used public and SHOULD HAVE BEEN created with DisposableStore. snapshotted here"), g.print()), p?.();
      };
    }
  }
  function t(v, p) {
    return h(v, () => {}, 0, undefined, true, undefined, p);
  }
  se.defer = t;
  function n(v) {
    return (p, g = null, b) => {
      let m = false, _;
      return _ = v((T) => {
        if (!m)
          return _ ? _.dispose() : m = true, p.call(g, T);
      }, null, b), m && _.dispose(), _;
    };
  }
  se.once = n;
  function s(v, p, g) {
    return c((b, m = null, _) => v((T) => b.call(m, p(T)), null, _), g);
  }
  se.map = s;
  function o(v, p, g) {
    return c((b, m = null, _) => v((T) => {
      p(T), b.call(m, T);
    }, null, _), g);
  }
  se.forEach = o;
  function r(v, p, g) {
    return c((b, m = null, _) => v((T) => p(T) && b.call(m, T), null, _), g);
  }
  se.filter = r;
  function a(v) {
    return v;
  }
  se.signal = a;
  function l(...v) {
    return (p, g = null, b) => {
      let m = It(...v.map((_) => _((T) => p.call(g, T))));
      return d(m, b);
    };
  }
  se.any = l;
  function u(v, p, g, b) {
    let m = g;
    return s(v, (_) => (m = p(m, _), m), b);
  }
  se.reduce = u;
  function c(v, p) {
    let g, b = { onWillAddFirstListener() {
      g = v(m.fire, m);
    }, onDidRemoveLastListener() {
      g?.dispose();
    } };
    p || e(b);
    let m = new D(b);
    return p?.add(m), m.event;
  }
  function d(v, p) {
    return p instanceof Array ? p.push(v) : p && p.add(v), v;
  }
  function h(v, p, g = 100, b = false, m = false, _, T) {
    let x, R, $, P = 0, de, Re = { leakWarningThreshold: _, onWillAddFirstListener() {
      x = v((ie) => {
        P++, R = p(R, ie), b && !$ && (oe.fire(R), R = undefined), de = () => {
          let N = R;
          R = undefined, $ = undefined, (!b || P > 1) && oe.fire(N), P = 0;
        }, typeof g == "number" ? (clearTimeout($), $ = setTimeout(de, g)) : $ === undefined && ($ = 0, queueMicrotask(de));
      });
    }, onWillRemoveListener() {
      m && P > 0 && de?.();
    }, onDidRemoveLastListener() {
      de = undefined, x.dispose();
    } };
    T || e(Re);
    let oe = new D(Re);
    return T?.add(oe), oe.event;
  }
  se.debounce = h;
  function f(v, p = 0, g) {
    return se.debounce(v, (b, m) => b ? (b.push(m), b) : [m], p, undefined, true, undefined, g);
  }
  se.accumulate = f;
  function I(v, p = (b, m) => b === m, g) {
    let b = true, m;
    return r(v, (_) => {
      let T = b || !p(_, m);
      return b = false, m = _, T;
    }, g);
  }
  se.latch = I;
  function L(v, p, g) {
    return [se.filter(v, p, g), se.filter(v, (b) => !p(b), g)];
  }
  se.split = L;
  function M(v, p = false, g = [], b) {
    let m = g.slice(), _ = v((R) => {
      m ? m.push(R) : x.fire(R);
    });
    b && b.add(_);
    let T = () => {
      m?.forEach((R) => x.fire(R)), m = null;
    }, x = new D({ onWillAddFirstListener() {
      _ || (_ = v((R) => x.fire(R)), b && b.add(_));
    }, onDidAddFirstListener() {
      m && (p ? setTimeout(T) : T());
    }, onDidRemoveLastListener() {
      _ && _.dispose(), _ = null;
    } });
    return b && b.add(x), x.event;
  }
  se.buffer = M;
  function q(v, p) {
    return (b, m, _) => {
      let T = p(new W);
      return v(function(x) {
        let R = T.evaluate(x);
        R !== S && b.call(m, R);
      }, undefined, _);
    };
  }
  se.chain = q;
  let S = Symbol("HaltChainable");

  class W {
    constructor() {
      this.steps = [];
    }
    map(p) {
      return this.steps.push(p), this;
    }
    forEach(p) {
      return this.steps.push((g) => (p(g), g)), this;
    }
    filter(p) {
      return this.steps.push((g) => p(g) ? g : S), this;
    }
    reduce(p, g) {
      let b = g;
      return this.steps.push((m) => (b = p(b, m), b)), this;
    }
    latch(p = (g, b) => g === b) {
      let g = true, b;
      return this.steps.push((m) => {
        let _ = g || !p(m, b);
        return g = false, b = m, _ ? m : S;
      }), this;
    }
    evaluate(p) {
      for (let g of this.steps)
        if (p = g(p), p === S)
          break;
      return p;
    }
  }
  function E(v, p, g = (b) => b) {
    let b = (...x) => T.fire(g(...x)), m = () => v.on(p, b), _ = () => v.removeListener(p, b), T = new D({ onWillAddFirstListener: m, onDidRemoveLastListener: _ });
    return T.event;
  }
  se.fromNodeEventEmitter = E;
  function y(v, p, g = (b) => b) {
    let b = (...x) => T.fire(g(...x)), m = () => v.addEventListener(p, b), _ = () => v.removeEventListener(p, b), T = new D({ onWillAddFirstListener: m, onDidRemoveLastListener: _ });
    return T.event;
  }
  se.fromDOMEventEmitter = y;
  function w(v) {
    return new Promise((p) => n(v)(p));
  }
  se.toPromise = w;
  function G(v) {
    let p = new D;
    return v.then((g) => {
      p.fire(g);
    }, () => {
      p.fire(undefined);
    }).finally(() => {
      p.dispose();
    }), p.event;
  }
  se.fromPromise = G;
  function ue(v, p) {
    return v((g) => p.fire(g));
  }
  se.forward = ue;
  function Se(v, p, g) {
    return p(g), v((b) => p(b));
  }
  se.runAndSubscribe = Se;

  class ce {
    constructor(p, g) {
      this._observable = p;
      this._counter = 0;
      this._hasChanged = false;
      let b = { onWillAddFirstListener: () => {
        p.addObserver(this);
      }, onDidRemoveLastListener: () => {
        p.removeObserver(this);
      } };
      g || e(b), this.emitter = new D(b), g && g.add(this.emitter);
    }
    beginUpdate(p) {
      this._counter++;
    }
    handlePossibleChange(p) {}
    handleChange(p, g) {
      this._hasChanged = true;
    }
    endUpdate(p) {
      this._counter--, this._counter === 0 && (this._observable.reportChanges(), this._hasChanged && (this._hasChanged = false, this.emitter.fire(this._observable.get())));
    }
  }
  function we(v, p) {
    return new ce(v, p).emitter.event;
  }
  se.fromObservable = we;
  function A(v) {
    return (p, g, b) => {
      let m = 0, _ = false, T = { beginUpdate() {
        m++;
      }, endUpdate() {
        m--, m === 0 && (v.reportChanges(), _ && (_ = false, p.call(g)));
      }, handlePossibleChange() {}, handleChange() {
        _ = true;
      } };
      v.addObserver(T), v.reportChanges();
      let x = { dispose() {
        v.removeObserver(T);
      } };
      return b instanceof fe ? b.add(x) : Array.isArray(b) && b.push(x), x;
    };
  }
  se.fromObservableLight = A;
})(ee ||= {});
var We = class We {
  constructor(e) {
    this.listenerCount = 0;
    this.invocationCount = 0;
    this.elapsedOverall = 0;
    this.durations = [];
    this.name = `${e}_${We._idPool++}`, We.all.add(this);
  }
  start(e) {
    this._stopWatch = new kt, this.listenerCount = e;
  }
  stop() {
    if (this._stopWatch) {
      let e = this._stopWatch.elapsed();
      this.durations.push(e), this.elapsedOverall += e, this.invocationCount += 1, this._stopWatch = undefined;
    }
  }
};
We.all = new Set, We._idPool = 0;
var Ei = We;
var Mn = -1;
var Bt = class Bt {
  constructor(e, t, n = (Bt._idPool++).toString(16).padStart(3, "0")) {
    this._errorHandler = e;
    this.threshold = t;
    this.name = n;
    this._warnCountdown = 0;
  }
  dispose() {
    this._stacks?.clear();
  }
  check(e, t) {
    let n = this.threshold;
    if (n <= 0 || t < n)
      return;
    this._stacks || (this._stacks = new Map);
    let s = this._stacks.get(e.value) || 0;
    if (this._stacks.set(e.value, s + 1), this._warnCountdown -= 1, this._warnCountdown <= 0) {
      this._warnCountdown = n * 0.5;
      let [o, r] = this.getMostFrequentStack(), a = `[${this.name}] potential listener LEAK detected, having ${t} listeners already. MOST frequent listener (${r}):`;
      console.warn(a), console.warn(o);
      let l = new Ii(a, o);
      this._errorHandler(l);
    }
    return () => {
      let o = this._stacks.get(e.value) || 0;
      this._stacks.set(e.value, o - 1);
    };
  }
  getMostFrequentStack() {
    if (!this._stacks)
      return;
    let e, t = 0;
    for (let [n, s] of this._stacks)
      (!e || t < s) && (e = [n, s], t = s);
    return e;
  }
};
Bt._idPool = 1;
var yi = Bt;
var nt = class i {
  constructor(e) {
    this.value = e;
  }
  static create() {
    let e = new Error;
    return new i(e.stack ?? "");
  }
  print() {
    console.warn(this.value.split(`
`).slice(2).join(`
`));
  }
};
var Ii = class extends Error {
  constructor(e, t) {
    super(e), this.name = "ListenerLeakError", this.stack = t;
  }
};
var Li = class extends Error {
  constructor(e, t) {
    super(e), this.name = "ListenerRefusalError", this.stack = t;
  }
};
var Xr = 0;
var Ge = class {
  constructor(e) {
    this.value = e;
    this.id = Xr++;
  }
};
var Yr = 2;
var Qr = (i, e) => {
  if (i instanceof Ge)
    e(i);
  else
    for (let t = 0;t < i.length; t++) {
      let n = i[t];
      n && e(n);
    }
};
var Pt;
if (qr) {
  let i = [];
  setInterval(() => {
    i.length !== 0 && (console.warn("[LEAKING LISTENERS] GC'ed these listeners that were NOT yet disposed:"), console.warn(i.join(`
`)), i.length = 0);
  }, 3000), Pt = new FinalizationRegistry((e) => {
    typeof e == "string" && i.push(e);
  });
}
var D = class {
  constructor(e) {
    this._size = 0;
    this._options = e, this._leakageMon = Mn > 0 || this._options?.leakWarningThreshold ? new yi(e?.onListenerError ?? Pe, this._options?.leakWarningThreshold ?? Mn) : undefined, this._perfMon = this._options?._profName ? new Ei(this._options._profName) : undefined, this._deliveryQueue = this._options?.deliveryQueue;
  }
  dispose() {
    if (!this._disposed) {
      if (this._disposed = true, this._deliveryQueue?.current === this && this._deliveryQueue.reset(), this._listeners) {
        if (Dn) {
          let e = this._listeners;
          queueMicrotask(() => {
            Qr(e, (t) => t.stack?.print());
          });
        }
        this._listeners = undefined, this._size = 0;
      }
      this._options?.onDidRemoveLastListener?.(), this._leakageMon?.dispose();
    }
  }
  get event() {
    return this._event ??= (e, t, n) => {
      if (this._leakageMon && this._size > this._leakageMon.threshold ** 2) {
        let l = `[${this._leakageMon.name}] REFUSES to accept new listeners because it exceeded its threshold by far (${this._size} vs ${this._leakageMon.threshold})`;
        console.warn(l);
        let u = this._leakageMon.getMostFrequentStack() ?? ["UNKNOWN stack", -1], c = new Li(`${l}. HINT: Stack shows most frequent listener (${u[1]}-times)`, u[0]);
        return (this._options?.onListenerError || Pe)(c), B.None;
      }
      if (this._disposed)
        return B.None;
      t && (e = e.bind(t));
      let s = new Ge(e), o, r;
      this._leakageMon && this._size >= Math.ceil(this._leakageMon.threshold * 0.2) && (s.stack = nt.create(), o = this._leakageMon.check(s.stack, this._size + 1)), Dn && (s.stack = r ?? nt.create()), this._listeners ? this._listeners instanceof Ge ? (this._deliveryQueue ??= new wi, this._listeners = [this._listeners, s]) : this._listeners.push(s) : (this._options?.onWillAddFirstListener?.(this), this._listeners = s, this._options?.onDidAddFirstListener?.(this)), this._size++;
      let a = O(() => {
        Pt?.unregister(a), o?.(), this._removeListener(s);
      });
      if (n instanceof fe ? n.add(a) : Array.isArray(n) && n.push(a), Pt) {
        let l = new Error().stack.split(`
`).slice(2, 3).join(`
`).trim(), u = /(file:|vscode-file:\/\/vscode-app)?(\/[^:]*:\d+:\d+)/.exec(l);
        Pt.register(a, u?.[2] ?? l, a);
      }
      return a;
    }, this._event;
  }
  _removeListener(e) {
    if (this._options?.onWillRemoveListener?.(this), !this._listeners)
      return;
    if (this._size === 1) {
      this._listeners = undefined, this._options?.onDidRemoveLastListener?.(this), this._size = 0;
      return;
    }
    let t = this._listeners, n = t.indexOf(e);
    if (n === -1)
      throw console.log("disposed?", this._disposed), console.log("size?", this._size), console.log("arr?", JSON.stringify(this._listeners)), new Error("Attempted to dispose unknown listener");
    this._size--, t[n] = undefined;
    let s = this._deliveryQueue.current === this;
    if (this._size * Yr <= t.length) {
      let o = 0;
      for (let r = 0;r < t.length; r++)
        t[r] ? t[o++] = t[r] : s && (this._deliveryQueue.end--, o < this._deliveryQueue.i && this._deliveryQueue.i--);
      t.length = o;
    }
  }
  _deliver(e, t) {
    if (!e)
      return;
    let n = this._options?.onListenerError || Pe;
    if (!n) {
      e.value(t);
      return;
    }
    try {
      e.value(t);
    } catch (s) {
      n(s);
    }
  }
  _deliverQueue(e) {
    let t = e.current._listeners;
    for (;e.i < e.end; )
      this._deliver(t[e.i++], e.value);
    e.reset();
  }
  fire(e) {
    if (this._deliveryQueue?.current && (this._deliverQueue(this._deliveryQueue), this._perfMon?.stop()), this._perfMon?.start(this._size), this._listeners)
      if (this._listeners instanceof Ge)
        this._deliver(this._listeners, e);
      else {
        let t = this._deliveryQueue;
        t.enqueue(this, e, this._listeners.length), this._deliverQueue(t);
      }
    this._perfMon?.stop();
  }
  hasListeners() {
    return this._size > 0;
  }
};
var wi = class {
  constructor() {
    this.i = -1;
    this.end = 0;
  }
  enqueue(e, t, n) {
    this.i = 0, this.end = n, this.current = e, this.value = t;
  }
  reset() {
    this.i = this.end, this.current = undefined, this.value = undefined;
  }
};
var An = { texturePage: 0, texturePosition: { x: 0, y: 0 }, texturePositionClipSpace: { x: 0, y: 0 }, offset: { x: 0, y: 0 }, size: { x: 0, y: 0 }, sizeClipSpace: { x: 0, y: 0 } };
var rt = 2;
var st;
var ae = class i {
  constructor(e, t, n) {
    this._document = e;
    this._config = t;
    this._unicodeService = n;
    this._didWarmUp = false;
    this._cacheMap = new tt;
    this._cacheMapCombined = new tt;
    this._pages = [];
    this._activePages = [];
    this._workBoundingBox = { top: 0, left: 0, bottom: 0, right: 0 };
    this._workAttributeData = new he;
    this._textureSize = 512;
    this._onAddTextureAtlasCanvas = new D;
    this.onAddTextureAtlasCanvas = this._onAddTextureAtlasCanvas.event;
    this._onRemoveTextureAtlasCanvas = new D;
    this.onRemoveTextureAtlasCanvas = this._onRemoveTextureAtlasCanvas.event;
    this._requestClearModel = false;
    this._createNewPage(), this._tmpCanvas = Sn(e, this._config.deviceCellWidth * 4 + rt * 2, this._config.deviceCellHeight + rt * 2), this._tmpCtx = F(this._tmpCanvas.getContext("2d", { alpha: this._config.allowTransparency, willReadFrequently: true }));
  }
  get pages() {
    return this._pages;
  }
  dispose() {
    this._tmpCanvas.remove();
    for (let e of this.pages)
      e.canvas.remove();
    this._onAddTextureAtlasCanvas.dispose();
  }
  warmUp() {
    this._didWarmUp || (this._doWarmUp(), this._didWarmUp = true);
  }
  _doWarmUp() {
    let e = new wn;
    for (let t = 33;t < 126; t++)
      e.enqueue(() => {
        if (!this._cacheMap.get(t, 0, 0, 0)) {
          let n = this._drawToCache(t, 0, 0, 0, false, undefined);
          this._cacheMap.set(t, 0, 0, 0, n);
        }
      });
  }
  beginFrame() {
    return this._requestClearModel;
  }
  clearTexture() {
    if (!(this._pages[0].currentRow.x === 0 && this._pages[0].currentRow.y === 0)) {
      for (let e of this._pages)
        e.clear();
      this._cacheMap.clear(), this._cacheMapCombined.clear(), this._didWarmUp = false;
    }
  }
  _createNewPage() {
    if (i.maxAtlasPages && this._pages.length >= Math.max(4, i.maxAtlasPages)) {
      let t = this._pages.filter((u) => u.canvas.width * 2 <= (i.maxTextureSize || 4096)).sort((u, c) => c.canvas.width !== u.canvas.width ? c.canvas.width - u.canvas.width : c.percentageUsed - u.percentageUsed), n = -1, s = 0;
      for (let u = 0;u < t.length; u++)
        if (t[u].canvas.width !== s)
          n = u, s = t[u].canvas.width;
        else if (u - n === 3)
          break;
      let o = t.slice(n, n + 4), r = o.map((u) => u.glyphs[0].texturePage).sort((u, c) => u > c ? 1 : -1), a = this.pages.length - o.length, l = this._mergePages(o, a);
      l.version++;
      for (let u = r.length - 1;u >= 0; u--)
        this._deletePage(r[u]);
      this.pages.push(l), this._requestClearModel = true, this._onAddTextureAtlasCanvas.fire(l.canvas);
    }
    let e = new ot(this._document, this._textureSize);
    return this._pages.push(e), this._activePages.push(e), this._onAddTextureAtlasCanvas.fire(e.canvas), e;
  }
  _mergePages(e, t) {
    let n = e[0].canvas.width * 2, s = new ot(this._document, n, e);
    for (let [o, r] of e.entries()) {
      let a = o * r.canvas.width % n, l = Math.floor(o / 2) * r.canvas.height;
      s.ctx.drawImage(r.canvas, a, l);
      for (let c of r.glyphs)
        c.texturePage = t, c.sizeClipSpace.x = c.size.x / n, c.sizeClipSpace.y = c.size.y / n, c.texturePosition.x += a, c.texturePosition.y += l, c.texturePositionClipSpace.x = c.texturePosition.x / n, c.texturePositionClipSpace.y = c.texturePosition.y / n;
      this._onRemoveTextureAtlasCanvas.fire(r.canvas);
      let u = this._activePages.indexOf(r);
      u !== -1 && this._activePages.splice(u, 1);
    }
    return s;
  }
  _deletePage(e) {
    this._pages.splice(e, 1);
    for (let t = e;t < this._pages.length; t++) {
      let n = this._pages[t];
      for (let s of n.glyphs)
        s.texturePage--;
      n.version++;
    }
  }
  getRasterizedGlyphCombinedChar(e, t, n, s, o, r) {
    return this._getFromCacheMap(this._cacheMapCombined, e, t, n, s, o, r);
  }
  getRasterizedGlyph(e, t, n, s, o, r) {
    return this._getFromCacheMap(this._cacheMap, e, t, n, s, o, r);
  }
  _getFromCacheMap(e, t, n, s, o, r, a) {
    return st = e.get(t, n, s, o), st || (st = this._drawToCache(t, n, s, o, r, a), e.set(t, n, s, o, st)), st;
  }
  _getColorFromAnsiIndex(e) {
    if (e >= this._config.colors.ansi.length)
      throw new Error("No color found for idx " + e);
    return this._config.colors.ansi[e];
  }
  _getBackgroundColor(e, t, n, s) {
    if (this._config.allowTransparency)
      return Z;
    let o;
    switch (e) {
      case 16777216:
      case 33554432:
        o = this._getColorFromAnsiIndex(t);
        break;
      case 50331648:
        let r = he.toColorRGB(t);
        o = X.toColor(r[0], r[1], r[2]);
        break;
      case 0:
      default:
        n ? o = Ue.opaque(this._config.colors.foreground) : o = this._config.colors.background;
        break;
    }
    return this._config.allowTransparency || (o = Ue.opaque(o)), o;
  }
  _getForegroundColor(e, t, n, s, o, r, a, l, u, c) {
    let d = this._getMinimumContrastColor(e, t, n, s, o, r, a, u, l, c);
    if (d)
      return d;
    let h;
    switch (o) {
      case 16777216:
      case 33554432:
        this._config.drawBoldTextInBrightColors && u && r < 8 && (r += 8), h = this._getColorFromAnsiIndex(r);
        break;
      case 50331648:
        let f = he.toColorRGB(r);
        h = X.toColor(f[0], f[1], f[2]);
        break;
      case 0:
      default:
        a ? h = this._config.colors.background : h = this._config.colors.foreground;
    }
    return this._config.allowTransparency && (h = Ue.opaque(h)), l && (h = Ue.multiplyOpacity(h, gn)), h;
  }
  _resolveBackgroundRgba(e, t, n) {
    switch (e) {
      case 16777216:
      case 33554432:
        return this._getColorFromAnsiIndex(t).rgba;
      case 50331648:
        return t << 8;
      case 0:
      default:
        return n ? this._config.colors.foreground.rgba : this._config.colors.background.rgba;
    }
  }
  _resolveForegroundRgba(e, t, n, s) {
    switch (e) {
      case 16777216:
      case 33554432:
        return this._config.drawBoldTextInBrightColors && s && t < 8 && (t += 8), this._getColorFromAnsiIndex(t).rgba;
      case 50331648:
        return t << 8;
      case 0:
      default:
        return n ? this._config.colors.background.rgba : this._config.colors.foreground.rgba;
    }
  }
  _getMinimumContrastColor(e, t, n, s, o, r, a, l, u, c) {
    if (this._config.minimumContrastRatio === 1 || c)
      return;
    let d = this._getContrastCache(u), h = d.getColor(e, s);
    if (h !== undefined)
      return h || undefined;
    let f = this._resolveBackgroundRgba(t, n, a), I = this._resolveForegroundRgba(o, r, a, l), L = Te.ensureContrastRatio(f, I, this._config.minimumContrastRatio / (u ? 2 : 1));
    if (!L) {
      d.setColor(e, s, null);
      return;
    }
    let M = X.toColor(L >> 24 & 255, L >> 16 & 255, L >> 8 & 255);
    return d.setColor(e, s, M), M;
  }
  _getContrastCache(e) {
    return e ? this._config.colors.halfContrastCache : this._config.colors.contrastCache;
  }
  _drawToCache(e, t, n, s, o, r) {
    let a = typeof e == "number" ? String.fromCharCode(e) : e;
    r && this._tmpCanvas.parentElement !== r && (this._tmpCanvas.style.display = "none", r.append(this._tmpCanvas));
    let l = Math.min(this._config.deviceCellWidth * Math.max(a.length, 2) + rt * 2, this._config.deviceMaxTextureSize);
    this._tmpCanvas.width < l && (this._tmpCanvas.width = l);
    let u = Math.min(this._config.deviceCellHeight + rt * 4, this._textureSize);
    if (this._tmpCanvas.height < u && (this._tmpCanvas.height = u), this._tmpCtx.save(), this._workAttributeData.fg = n, this._workAttributeData.bg = t, this._workAttributeData.extended.ext = s, !!this._workAttributeData.isInvisible())
      return An;
    let d = !!this._workAttributeData.isBold(), h = !!this._workAttributeData.isInverse(), f = !!this._workAttributeData.isDim(), I = !!this._workAttributeData.isItalic(), L = !!this._workAttributeData.isUnderline(), M = !!this._workAttributeData.isStrikethrough(), q = !!this._workAttributeData.isOverline(), S = this._workAttributeData.getFgColor(), W = this._workAttributeData.getFgColorMode(), E = this._workAttributeData.getBgColor(), y = this._workAttributeData.getBgColorMode();
    if (h) {
      let x = S;
      S = E, E = x;
      let R = W;
      W = y, y = R;
    }
    let w = this._getBackgroundColor(y, E, h, f);
    this._tmpCtx.globalCompositeOperation = "copy", this._tmpCtx.fillStyle = w.css, this._tmpCtx.fillRect(0, 0, this._tmpCanvas.width, this._tmpCanvas.height), this._tmpCtx.globalCompositeOperation = "source-over";
    let G = d ? this._config.fontWeightBold : this._config.fontWeight, ue = I ? "italic" : "";
    this._tmpCtx.font = `${ue} ${G} ${this._config.fontSize * this._config.devicePixelRatio}px ${this._config.fontFamily}`, this._tmpCtx.textBaseline = St;
    let Se = a.length === 1 && Rt(a.charCodeAt(0)), ce = a.length === 1 && fn(a.charCodeAt(0)), we = this._getForegroundColor(t, y, E, n, W, S, h, f, d, Dt(a.charCodeAt(0)));
    this._tmpCtx.fillStyle = we.css;
    let A = ce ? 0 : rt * 2, se = false;
    this._config.customGlyphs !== false && (se = yn(this._tmpCtx, a, A, A, this._config.deviceCellWidth, this._config.deviceCellHeight, this._config.fontSize, this._config.devicePixelRatio));
    let v = !Se, p;
    if (typeof e == "number" ? p = this._unicodeService.wcwidth(e) : p = this._unicodeService.getStringCellWidth(e), L) {
      this._tmpCtx.save();
      let x = Math.max(1, Math.floor(this._config.fontSize * this._config.devicePixelRatio / 15)), R = x % 2 === 1 ? 0.5 : 0;
      if (this._tmpCtx.lineWidth = x, this._workAttributeData.isUnderlineColorDefault())
        this._tmpCtx.strokeStyle = this._tmpCtx.fillStyle;
      else if (this._workAttributeData.isUnderlineColorRGB())
        v = false, this._tmpCtx.strokeStyle = `rgb(${he.toColorRGB(this._workAttributeData.getUnderlineColor()).join(",")})`;
      else {
        v = false;
        let ie = this._workAttributeData.getUnderlineColor();
        this._config.drawBoldTextInBrightColors && this._workAttributeData.isBold() && ie < 8 && (ie += 8), this._tmpCtx.strokeStyle = this._getColorFromAnsiIndex(ie).css;
      }
      this._tmpCtx.beginPath();
      let $ = A, P = Math.ceil(A + this._config.deviceCharHeight) - R - (o ? x * 2 : 0), de = P + x, Re = P + x * 2, oe = this._workAttributeData.getUnderlineVariantOffset();
      for (let ie = 0;ie < p; ie++) {
        this._tmpCtx.save();
        let N = $ + ie * this._config.deviceCellWidth, ne = $ + (ie + 1) * this._config.deviceCellWidth, di = N + this._config.deviceCellWidth / 2;
        switch (this._workAttributeData.extended.underlineStyle) {
          case 2:
            this._tmpCtx.moveTo(N, P), this._tmpCtx.lineTo(ne, P), this._tmpCtx.moveTo(N, Re), this._tmpCtx.lineTo(ne, Re);
            break;
          case 3:
            let ft = x <= 1 ? Re : Math.ceil(A + this._config.deviceCharHeight - x / 2) - R, mt = x <= 1 ? P : Math.ceil(A + this._config.deviceCharHeight + x / 2) - R, qi = new Path2D;
            qi.rect(N, P, this._config.deviceCellWidth, Re - P), this._tmpCtx.clip(qi), this._tmpCtx.moveTo(N - this._config.deviceCellWidth / 2, de), this._tmpCtx.bezierCurveTo(N - this._config.deviceCellWidth / 2, mt, N, mt, N, de), this._tmpCtx.bezierCurveTo(N, ft, di, ft, di, de), this._tmpCtx.bezierCurveTo(di, mt, ne, mt, ne, de), this._tmpCtx.bezierCurveTo(ne, ft, ne + this._config.deviceCellWidth / 2, ft, ne + this._config.deviceCellWidth / 2, de);
            break;
          case 4:
            let _t = oe === 0 ? 0 : oe >= x ? x * 2 - oe : x - oe;
            !(oe >= x) === false || _t === 0 ? (this._tmpCtx.setLineDash([Math.round(x), Math.round(x)]), this._tmpCtx.moveTo(N + _t, P), this._tmpCtx.lineTo(ne, P)) : (this._tmpCtx.setLineDash([Math.round(x), Math.round(x)]), this._tmpCtx.moveTo(N, P), this._tmpCtx.lineTo(N + _t, P), this._tmpCtx.moveTo(N + _t + x, P), this._tmpCtx.lineTo(ne, P)), oe = bn(ne - N, x, oe);
            break;
          case 5:
            let Er = 0.6, yr = 0.3, hi = ne - N, ji = Math.floor(Er * hi), Xi = Math.floor(yr * hi), Ir = hi - ji - Xi;
            this._tmpCtx.setLineDash([ji, Xi, Ir]), this._tmpCtx.moveTo(N, P), this._tmpCtx.lineTo(ne, P);
            break;
          case 1:
          default:
            this._tmpCtx.moveTo(N, P), this._tmpCtx.lineTo(ne, P);
            break;
        }
        this._tmpCtx.stroke(), this._tmpCtx.restore();
      }
      if (this._tmpCtx.restore(), !se && this._config.fontSize >= 12 && !this._config.allowTransparency && a !== " ") {
        this._tmpCtx.save(), this._tmpCtx.textBaseline = "alphabetic";
        let ie = this._tmpCtx.measureText(a);
        if (this._tmpCtx.restore(), "actualBoundingBoxDescent" in ie && ie.actualBoundingBoxDescent > 0) {
          this._tmpCtx.save();
          let N = new Path2D;
          N.rect($, P - Math.ceil(x / 2), this._config.deviceCellWidth * p, Re - P + Math.ceil(x / 2)), this._tmpCtx.clip(N), this._tmpCtx.lineWidth = this._config.devicePixelRatio * 3, this._tmpCtx.strokeStyle = w.css, this._tmpCtx.strokeText(a, A, A + this._config.deviceCharHeight), this._tmpCtx.restore();
        }
      }
    }
    if (q) {
      let x = Math.max(1, Math.floor(this._config.fontSize * this._config.devicePixelRatio / 15)), R = x % 2 === 1 ? 0.5 : 0;
      this._tmpCtx.lineWidth = x, this._tmpCtx.strokeStyle = this._tmpCtx.fillStyle, this._tmpCtx.beginPath(), this._tmpCtx.moveTo(A, A + R), this._tmpCtx.lineTo(A + this._config.deviceCharWidth * p, A + R), this._tmpCtx.stroke();
    }
    if (se || this._tmpCtx.fillText(a, A, A + this._config.deviceCharHeight), a === "_" && !this._config.allowTransparency) {
      let x = Di(this._tmpCtx.getImageData(A, A, this._config.deviceCellWidth, this._config.deviceCellHeight), w, we, v);
      if (x)
        for (let R = 1;R <= 5 && (this._tmpCtx.save(), this._tmpCtx.fillStyle = w.css, this._tmpCtx.fillRect(0, 0, this._tmpCanvas.width, this._tmpCanvas.height), this._tmpCtx.restore(), this._tmpCtx.fillText(a, A, A + this._config.deviceCharHeight - R), x = Di(this._tmpCtx.getImageData(A, A, this._config.deviceCellWidth, this._config.deviceCellHeight), w, we, v), !!x); R++)
          ;
    }
    if (M) {
      let x = Math.max(1, Math.floor(this._config.fontSize * this._config.devicePixelRatio / 10)), R = this._tmpCtx.lineWidth % 2 === 1 ? 0.5 : 0;
      this._tmpCtx.lineWidth = x, this._tmpCtx.strokeStyle = this._tmpCtx.fillStyle, this._tmpCtx.beginPath(), this._tmpCtx.moveTo(A, A + Math.floor(this._config.deviceCharHeight / 2) - R), this._tmpCtx.lineTo(A + this._config.deviceCharWidth * p, A + Math.floor(this._config.deviceCharHeight / 2) - R), this._tmpCtx.stroke();
    }
    this._tmpCtx.restore();
    let g = this._tmpCtx.getImageData(0, 0, this._tmpCanvas.width, this._tmpCanvas.height), b;
    if (this._config.allowTransparency ? b = Jr(g) : b = Di(g, w, we, v), b)
      return An;
    let m = this._findGlyphBoundingBox(g, this._workBoundingBox, l, ce, se, A), _, T;
    for (;; ) {
      if (this._activePages.length === 0) {
        let x = this._createNewPage();
        _ = x, T = x.currentRow, T.height = m.size.y;
        break;
      }
      _ = this._activePages[this._activePages.length - 1], T = _.currentRow;
      for (let x of this._activePages)
        m.size.y <= x.currentRow.height && (_ = x, T = x.currentRow);
      for (let x = this._activePages.length - 1;x >= 0; x--)
        for (let R of this._activePages[x].fixedRows)
          R.height <= T.height && m.size.y <= R.height && (_ = this._activePages[x], T = R);
      if (m.size.x > this._textureSize) {
        this._overflowSizePage || (this._overflowSizePage = new ot(this._document, this._config.deviceMaxTextureSize), this.pages.push(this._overflowSizePage), this._requestClearModel = true, this._onAddTextureAtlasCanvas.fire(this._overflowSizePage.canvas)), _ = this._overflowSizePage, T = this._overflowSizePage.currentRow, T.x + m.size.x >= _.canvas.width && (T.x = 0, T.y += T.height, T.height = 0);
        break;
      }
      if (T.y + m.size.y >= _.canvas.height || T.height > m.size.y + 2) {
        let x = false;
        if (_.currentRow.y + _.currentRow.height + m.size.y >= _.canvas.height) {
          let R;
          for (let $ of this._activePages)
            if ($.currentRow.y + $.currentRow.height + m.size.y < $.canvas.height) {
              R = $;
              break;
            }
          if (R)
            _ = R;
          else if (i.maxAtlasPages && this._pages.length >= i.maxAtlasPages && T.y + m.size.y <= _.canvas.height && T.height >= m.size.y && T.x + m.size.x <= _.canvas.width)
            x = true;
          else {
            let $ = this._createNewPage();
            _ = $, T = $.currentRow, T.height = m.size.y, x = true;
          }
        }
        x || (_.currentRow.height > 0 && _.fixedRows.push(_.currentRow), T = { x: 0, y: _.currentRow.y + _.currentRow.height, height: m.size.y }, _.fixedRows.push(T), _.currentRow = { x: 0, y: T.y + T.height, height: 0 });
      }
      if (T.x + m.size.x <= _.canvas.width)
        break;
      T === _.currentRow ? (T.x = 0, T.y += T.height, T.height = 0) : _.fixedRows.splice(_.fixedRows.indexOf(T), 1);
    }
    return m.texturePage = this._pages.indexOf(_), m.texturePosition.x = T.x, m.texturePosition.y = T.y, m.texturePositionClipSpace.x = T.x / _.canvas.width, m.texturePositionClipSpace.y = T.y / _.canvas.height, m.sizeClipSpace.x /= _.canvas.width, m.sizeClipSpace.y /= _.canvas.height, T.height = Math.max(T.height, m.size.y), T.x += m.size.x, _.ctx.putImageData(g, m.texturePosition.x - this._workBoundingBox.left, m.texturePosition.y - this._workBoundingBox.top, this._workBoundingBox.left, this._workBoundingBox.top, m.size.x, m.size.y), _.addGlyph(m), _.version++, m;
  }
  _findGlyphBoundingBox(e, t, n, s, o, r) {
    t.top = 0;
    let a = s ? this._config.deviceCellHeight : this._tmpCanvas.height, l = s ? this._config.deviceCellWidth : n, u = false;
    for (let c = 0;c < a; c++) {
      for (let d = 0;d < l; d++) {
        let h = c * this._tmpCanvas.width * 4 + d * 4 + 3;
        if (e.data[h] !== 0) {
          t.top = c, u = true;
          break;
        }
      }
      if (u)
        break;
    }
    t.left = 0, u = false;
    for (let c = 0;c < r + l; c++) {
      for (let d = 0;d < a; d++) {
        let h = d * this._tmpCanvas.width * 4 + c * 4 + 3;
        if (e.data[h] !== 0) {
          t.left = c, u = true;
          break;
        }
      }
      if (u)
        break;
    }
    t.right = l, u = false;
    for (let c = r + l - 1;c >= r; c--) {
      for (let d = 0;d < a; d++) {
        let h = d * this._tmpCanvas.width * 4 + c * 4 + 3;
        if (e.data[h] !== 0) {
          t.right = c, u = true;
          break;
        }
      }
      if (u)
        break;
    }
    t.bottom = a, u = false;
    for (let c = a - 1;c >= 0; c--) {
      for (let d = 0;d < l; d++) {
        let h = c * this._tmpCanvas.width * 4 + d * 4 + 3;
        if (e.data[h] !== 0) {
          t.bottom = c, u = true;
          break;
        }
      }
      if (u)
        break;
    }
    return { texturePage: 0, texturePosition: { x: 0, y: 0 }, texturePositionClipSpace: { x: 0, y: 0 }, size: { x: t.right - t.left + 1, y: t.bottom - t.top + 1 }, sizeClipSpace: { x: t.right - t.left + 1, y: t.bottom - t.top + 1 }, offset: { x: -t.left + r + (s || o ? Math.floor((this._config.deviceCellWidth - this._config.deviceCharWidth) / 2) : 0), y: -t.top + r + (s || o ? this._config.lineHeight === 1 ? 0 : Math.round((this._config.deviceCellHeight - this._config.deviceCharHeight) / 2) : 0) } };
  }
};
var ot = class {
  constructor(e, t, n) {
    this._usedPixels = 0;
    this._glyphs = [];
    this.version = 0;
    this.currentRow = { x: 0, y: 0, height: 0 };
    this.fixedRows = [];
    if (n)
      for (let s of n)
        this._glyphs.push(...s.glyphs), this._usedPixels += s._usedPixels;
    this.canvas = Sn(e, t, t), this.ctx = F(this.canvas.getContext("2d", { alpha: true }));
  }
  get percentageUsed() {
    return this._usedPixels / (this.canvas.width * this.canvas.height);
  }
  get glyphs() {
    return this._glyphs;
  }
  addGlyph(e) {
    this._glyphs.push(e), this._usedPixels += e.size.x * e.size.y;
  }
  clear() {
    this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height), this.currentRow.x = 0, this.currentRow.y = 0, this.currentRow.height = 0, this.fixedRows.length = 0, this.version++;
  }
};
function Di(i, e, t, n) {
  let s = e.rgba >>> 24, o = e.rgba >>> 16 & 255, r = e.rgba >>> 8 & 255, a = t.rgba >>> 24, l = t.rgba >>> 16 & 255, u = t.rgba >>> 8 & 255, c = Math.floor((Math.abs(s - a) + Math.abs(o - l) + Math.abs(r - u)) / 12), d = true;
  for (let h = 0;h < i.data.length; h += 4)
    i.data[h] === s && i.data[h + 1] === o && i.data[h + 2] === r || n && Math.abs(i.data[h] - s) + Math.abs(i.data[h + 1] - o) + Math.abs(i.data[h + 2] - r) < c ? i.data[h + 3] = 0 : d = false;
  return d;
}
function Jr(i) {
  for (let e = 0;e < i.data.length; e += 4)
    if (i.data[e + 3] > 0)
      return false;
  return true;
}
function Sn(i, e, t) {
  let n = i.createElement("canvas");
  return n.width = e, n.height = t, n;
}
function On(i, e, t, n, s, o, r, a) {
  let l = { foreground: o.foreground, background: o.background, cursor: Z, cursorAccent: Z, selectionForeground: Z, selectionBackgroundTransparent: Z, selectionBackgroundOpaque: Z, selectionInactiveBackgroundTransparent: Z, selectionInactiveBackgroundOpaque: Z, overviewRulerBorder: Z, scrollbarSliderBackground: Z, scrollbarSliderHoverBackground: Z, scrollbarSliderActiveBackground: Z, ansi: o.ansi.slice(), contrastCache: o.contrastCache, halfContrastCache: o.halfContrastCache };
  return { customGlyphs: s.customGlyphs, devicePixelRatio: r, deviceMaxTextureSize: a, letterSpacing: s.letterSpacing, lineHeight: s.lineHeight, deviceCellWidth: i, deviceCellHeight: e, deviceCharWidth: t, deviceCharHeight: n, fontFamily: s.fontFamily, fontSize: s.fontSize, fontWeight: s.fontWeight, fontWeightBold: s.fontWeightBold, allowTransparency: s.allowTransparency, drawBoldTextInBrightColors: s.drawBoldTextInBrightColors, minimumContrastRatio: s.minimumContrastRatio, colors: l };
}
function Mi(i, e) {
  for (let t = 0;t < i.colors.ansi.length; t++)
    if (i.colors.ansi[t].rgba !== e.colors.ansi[t].rgba)
      return false;
  return i.devicePixelRatio === e.devicePixelRatio && i.customGlyphs === e.customGlyphs && i.lineHeight === e.lineHeight && i.letterSpacing === e.letterSpacing && i.fontFamily === e.fontFamily && i.fontSize === e.fontSize && i.fontWeight === e.fontWeight && i.fontWeightBold === e.fontWeightBold && i.allowTransparency === e.allowTransparency && i.deviceCharWidth === e.deviceCharWidth && i.deviceCharHeight === e.deviceCharHeight && i.drawBoldTextInBrightColors === e.drawBoldTextInBrightColors && i.minimumContrastRatio === e.minimumContrastRatio && i.colors.foreground.rgba === e.colors.foreground.rgba && i.colors.background.rgba === e.colors.background.rgba;
}
function Fn(i) {
  return (i & 50331648) === 16777216 || (i & 50331648) === 33554432;
}
var le = [];
function Nt(i, e, t, n, s, o, r, a, l) {
  let u = On(n, s, o, r, e, t, a, l);
  for (let h = 0;h < le.length; h++) {
    let f = le[h], I = f.ownedBy.indexOf(i);
    if (I >= 0) {
      if (Mi(f.config, u))
        return f.atlas;
      f.ownedBy.length === 1 ? (f.atlas.dispose(), le.splice(h, 1)) : f.ownedBy.splice(I, 1);
      break;
    }
  }
  for (let h = 0;h < le.length; h++) {
    let f = le[h];
    if (Mi(f.config, u))
      return f.ownedBy.push(i), f.atlas;
  }
  let c = i._core, d = { atlas: new ae(document, u, c.unicodeService), config: u, ownedBy: [i] };
  return le.push(d), d.atlas;
}
function Ai(i) {
  for (let e = 0;e < le.length; e++) {
    let t = le[e].ownedBy.indexOf(i);
    if (t !== -1) {
      le[e].ownedBy.length === 1 ? (le[e].atlas.dispose(), le.splice(e, 1)) : le[e].ownedBy.splice(t, 1);
      break;
    }
  }
}
var Ut = 600;
var Ht = class {
  constructor(e, t) {
    this._renderCallback = e;
    this._coreBrowserService = t;
    this.isCursorVisible = true, this._coreBrowserService.isFocused && this._restartInterval();
  }
  get isPaused() {
    return !(this._blinkStartTimeout || this._blinkInterval);
  }
  dispose() {
    this._blinkInterval && (this._coreBrowserService.window.clearInterval(this._blinkInterval), this._blinkInterval = undefined), this._blinkStartTimeout && (this._coreBrowserService.window.clearTimeout(this._blinkStartTimeout), this._blinkStartTimeout = undefined), this._animationFrame && (this._coreBrowserService.window.cancelAnimationFrame(this._animationFrame), this._animationFrame = undefined);
  }
  restartBlinkAnimation() {
    this.isPaused || (this._animationTimeRestarted = Date.now(), this.isCursorVisible = true, this._animationFrame || (this._animationFrame = this._coreBrowserService.window.requestAnimationFrame(() => {
      this._renderCallback(), this._animationFrame = undefined;
    })));
  }
  _restartInterval(e = Ut) {
    this._blinkInterval && (this._coreBrowserService.window.clearInterval(this._blinkInterval), this._blinkInterval = undefined), this._blinkStartTimeout = this._coreBrowserService.window.setTimeout(() => {
      if (this._animationTimeRestarted) {
        let t = Ut - (Date.now() - this._animationTimeRestarted);
        if (this._animationTimeRestarted = undefined, t > 0) {
          this._restartInterval(t);
          return;
        }
      }
      this.isCursorVisible = false, this._animationFrame = this._coreBrowserService.window.requestAnimationFrame(() => {
        this._renderCallback(), this._animationFrame = undefined;
      }), this._blinkInterval = this._coreBrowserService.window.setInterval(() => {
        if (this._animationTimeRestarted) {
          let t = Ut - (Date.now() - this._animationTimeRestarted);
          this._animationTimeRestarted = undefined, this._restartInterval(t);
          return;
        }
        this.isCursorVisible = !this.isCursorVisible, this._animationFrame = this._coreBrowserService.window.requestAnimationFrame(() => {
          this._renderCallback(), this._animationFrame = undefined;
        });
      }, Ut);
    }, e);
  }
  pause() {
    this.isCursorVisible = true, this._blinkInterval && (this._coreBrowserService.window.clearInterval(this._blinkInterval), this._blinkInterval = undefined), this._blinkStartTimeout && (this._coreBrowserService.window.clearTimeout(this._blinkStartTimeout), this._blinkStartTimeout = undefined), this._animationFrame && (this._coreBrowserService.window.cancelAnimationFrame(this._animationFrame), this._animationFrame = undefined);
  }
  resume() {
    this.pause(), this._animationTimeRestarted = undefined, this._restartInterval(), this.restartBlinkAnimation();
  }
};
function Si(i, e, t) {
  let n = new e.ResizeObserver((s) => {
    let o = s.find((l) => l.target === i);
    if (!o)
      return;
    if (!("devicePixelContentBoxSize" in o)) {
      n?.disconnect(), n = undefined;
      return;
    }
    let r = o.devicePixelContentBoxSize[0].inlineSize, a = o.devicePixelContentBoxSize[0].blockSize;
    r > 0 && a > 0 && t(r, a);
  });
  try {
    n.observe(i, { box: ["device-pixel-content-box"] });
  } catch {
    n.disconnect(), n = undefined;
  }
  return O(() => n?.disconnect());
}
function kn(i) {
  return i > 65535 ? (i -= 65536, String.fromCharCode((i >> 10) + 55296) + String.fromCharCode(i % 1024 + 56320)) : String.fromCharCode(i);
}
var at = class i extends he {
  constructor() {
    super(...arguments);
    this.content = 0;
    this.fg = 0;
    this.bg = 0;
    this.extended = new it;
    this.combinedData = "";
  }
  static fromCharData(t) {
    let n = new i;
    return n.setFromCharData(t), n;
  }
  isCombined() {
    return this.content & 2097152;
  }
  getWidth() {
    return this.content >> 22;
  }
  getChars() {
    return this.content & 2097152 ? this.combinedData : this.content & 2097151 ? kn(this.content & 2097151) : "";
  }
  getCode() {
    return this.isCombined() ? this.combinedData.charCodeAt(this.combinedData.length - 1) : this.content & 2097151;
  }
  setFromCharData(t) {
    this.fg = t[0], this.bg = 0;
    let n = false;
    if (t[1].length > 2)
      n = true;
    else if (t[1].length === 2) {
      let s = t[1].charCodeAt(0);
      if (55296 <= s && s <= 56319) {
        let o = t[1].charCodeAt(1);
        56320 <= o && o <= 57343 ? this.content = (s - 55296) * 1024 + o - 56320 + 65536 | t[2] << 22 : n = true;
      } else
        n = true;
    } else
      this.content = t[1].charCodeAt(0) | t[2] << 22;
    n && (this.combinedData = t[1], this.content = 2097152 | t[2] << 22);
  }
  getAsCharData() {
    return [this.fg, this.getChars(), this.getWidth(), this.getCode()];
  }
};
var Gt = new Float32Array([2, 0, 0, 0, 0, -2, 0, 0, 0, 0, 1, 0, -1, 1, 0, 1]);
function $t(i, e, t) {
  let n = F(i.createProgram());
  if (i.attachShader(n, F(Pn(i, i.VERTEX_SHADER, e))), i.attachShader(n, F(Pn(i, i.FRAGMENT_SHADER, t))), i.linkProgram(n), i.getProgramParameter(n, i.LINK_STATUS))
    return n;
  console.error(i.getProgramInfoLog(n)), i.deleteProgram(n);
}
function Pn(i, e, t) {
  let n = F(i.createShader(e));
  if (i.shaderSource(n, t), i.compileShader(n), i.getShaderParameter(n, i.COMPILE_STATUS))
    return n;
  console.error(i.getShaderInfoLog(n)), i.deleteShader(n);
}
function Bn(i, e) {
  let t = Math.min(i.length * 2, e), n = new Float32Array(t);
  for (let s = 0;s < i.length; s++)
    n[s] = i[s];
  return n;
}
var Wt = class {
  constructor(e) {
    this.texture = e, this.version = -1;
  }
};
var is = `#version 300 es
layout (location = 0) in vec2 a_unitquad;
layout (location = 1) in vec2 a_cellpos;
layout (location = 2) in vec2 a_offset;
layout (location = 3) in vec2 a_size;
layout (location = 4) in float a_texpage;
layout (location = 5) in vec2 a_texcoord;
layout (location = 6) in vec2 a_texsize;

uniform mat4 u_projection;
uniform vec2 u_resolution;

out vec2 v_texcoord;
flat out int v_texpage;

void main() {
  vec2 zeroToOne = (a_offset / u_resolution) + a_cellpos + (a_unitquad * a_size);
  gl_Position = u_projection * vec4(zeroToOne, 0.0, 1.0);
  v_texpage = int(a_texpage);
  v_texcoord = a_texcoord + a_unitquad * a_texsize;
}`;
function ns(i) {
  let e = "";
  for (let t = 1;t < i; t++)
    e += ` else if (v_texpage == ${t}) { outColor = texture(u_texture[${t}], v_texcoord); }`;
  return `#version 300 es
precision lowp float;

in vec2 v_texcoord;
flat in int v_texpage;

uniform sampler2D u_texture[${i}];

out vec4 outColor;

void main() {
  if (v_texpage == 0) {
    outColor = texture(u_texture[0], v_texcoord);
  } ${e}
}`;
}
var De = 11;
var Ve = De * Float32Array.BYTES_PER_ELEMENT;
var rs = 2;
var H = 0;
var k;
var Fi = 0;
var lt = 0;
var Kt = class extends B {
  constructor(t, n, s, o) {
    super();
    this._terminal = t;
    this._gl = n;
    this._dimensions = s;
    this._optionsService = o;
    this._activeBuffer = 0;
    this._vertices = { count: 0, attributes: new Float32Array(0), attributesBuffers: [new Float32Array(0), new Float32Array(0)] };
    let r = this._gl;
    ae.maxAtlasPages === undefined && (ae.maxAtlasPages = Math.min(32, F(r.getParameter(r.MAX_TEXTURE_IMAGE_UNITS))), ae.maxTextureSize = F(r.getParameter(r.MAX_TEXTURE_SIZE))), this._program = F($t(r, is, ns(ae.maxAtlasPages))), this._register(O(() => r.deleteProgram(this._program))), this._projectionLocation = F(r.getUniformLocation(this._program, "u_projection")), this._resolutionLocation = F(r.getUniformLocation(this._program, "u_resolution")), this._textureLocation = F(r.getUniformLocation(this._program, "u_texture")), this._vertexArrayObject = r.createVertexArray(), r.bindVertexArray(this._vertexArrayObject);
    let a = new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]), l = r.createBuffer();
    this._register(O(() => r.deleteBuffer(l))), r.bindBuffer(r.ARRAY_BUFFER, l), r.bufferData(r.ARRAY_BUFFER, a, r.STATIC_DRAW), r.enableVertexAttribArray(0), r.vertexAttribPointer(0, 2, this._gl.FLOAT, false, 0, 0);
    let u = new Uint8Array([0, 1, 2, 3]), c = r.createBuffer();
    this._register(O(() => r.deleteBuffer(c))), r.bindBuffer(r.ELEMENT_ARRAY_BUFFER, c), r.bufferData(r.ELEMENT_ARRAY_BUFFER, u, r.STATIC_DRAW), this._attributesBuffer = F(r.createBuffer()), this._register(O(() => r.deleteBuffer(this._attributesBuffer))), r.bindBuffer(r.ARRAY_BUFFER, this._attributesBuffer), r.enableVertexAttribArray(2), r.vertexAttribPointer(2, 2, r.FLOAT, false, Ve, 0), r.vertexAttribDivisor(2, 1), r.enableVertexAttribArray(3), r.vertexAttribPointer(3, 2, r.FLOAT, false, Ve, 2 * Float32Array.BYTES_PER_ELEMENT), r.vertexAttribDivisor(3, 1), r.enableVertexAttribArray(4), r.vertexAttribPointer(4, 1, r.FLOAT, false, Ve, 4 * Float32Array.BYTES_PER_ELEMENT), r.vertexAttribDivisor(4, 1), r.enableVertexAttribArray(5), r.vertexAttribPointer(5, 2, r.FLOAT, false, Ve, 5 * Float32Array.BYTES_PER_ELEMENT), r.vertexAttribDivisor(5, 1), r.enableVertexAttribArray(6), r.vertexAttribPointer(6, 2, r.FLOAT, false, Ve, 7 * Float32Array.BYTES_PER_ELEMENT), r.vertexAttribDivisor(6, 1), r.enableVertexAttribArray(1), r.vertexAttribPointer(1, 2, r.FLOAT, false, Ve, 9 * Float32Array.BYTES_PER_ELEMENT), r.vertexAttribDivisor(1, 1), r.useProgram(this._program);
    let d = new Int32Array(ae.maxAtlasPages);
    for (let h = 0;h < ae.maxAtlasPages; h++)
      d[h] = h;
    r.uniform1iv(this._textureLocation, d), r.uniformMatrix4fv(this._projectionLocation, false, Gt), this._atlasTextures = [];
    for (let h = 0;h < ae.maxAtlasPages; h++) {
      let f = new Wt(F(r.createTexture()));
      this._register(O(() => r.deleteTexture(f.texture))), r.activeTexture(r.TEXTURE0 + h), r.bindTexture(r.TEXTURE_2D, f.texture), r.texParameteri(r.TEXTURE_2D, r.TEXTURE_WRAP_S, r.CLAMP_TO_EDGE), r.texParameteri(r.TEXTURE_2D, r.TEXTURE_WRAP_T, r.CLAMP_TO_EDGE), r.texImage2D(r.TEXTURE_2D, 0, r.RGBA, 1, 1, 0, r.RGBA, r.UNSIGNED_BYTE, new Uint8Array([255, 0, 0, 255])), this._atlasTextures[h] = f;
    }
    r.enable(r.BLEND), r.blendFunc(r.SRC_ALPHA, r.ONE_MINUS_SRC_ALPHA), this.handleResize();
  }
  beginFrame() {
    return this._atlas ? this._atlas.beginFrame() : true;
  }
  updateCell(t, n, s, o, r, a, l, u, c) {
    this._updateCell(this._vertices.attributes, t, n, s, o, r, a, l, u, c);
  }
  _updateCell(t, n, s, o, r, a, l, u, c, d) {
    if (H = (s * this._terminal.cols + n) * De, o === 0 || o === undefined) {
      t.fill(0, H, H + De - 1 - rs);
      return;
    }
    this._atlas && (u && u.length > 1 ? k = this._atlas.getRasterizedGlyphCombinedChar(u, r, a, l, false, this._terminal.element) : k = this._atlas.getRasterizedGlyph(o, r, a, l, false, this._terminal.element), Fi = Math.floor((this._dimensions.device.cell.width - this._dimensions.device.char.width) / 2), r !== d && k.offset.x > Fi ? (lt = k.offset.x - Fi, t[H] = -(k.offset.x - lt) + this._dimensions.device.char.left, t[H + 1] = -k.offset.y + this._dimensions.device.char.top, t[H + 2] = (k.size.x - lt) / this._dimensions.device.canvas.width, t[H + 3] = k.size.y / this._dimensions.device.canvas.height, t[H + 4] = k.texturePage, t[H + 5] = k.texturePositionClipSpace.x + lt / this._atlas.pages[k.texturePage].canvas.width, t[H + 6] = k.texturePositionClipSpace.y, t[H + 7] = k.sizeClipSpace.x - lt / this._atlas.pages[k.texturePage].canvas.width, t[H + 8] = k.sizeClipSpace.y) : (t[H] = -k.offset.x + this._dimensions.device.char.left, t[H + 1] = -k.offset.y + this._dimensions.device.char.top, t[H + 2] = k.size.x / this._dimensions.device.canvas.width, t[H + 3] = k.size.y / this._dimensions.device.canvas.height, t[H + 4] = k.texturePage, t[H + 5] = k.texturePositionClipSpace.x, t[H + 6] = k.texturePositionClipSpace.y, t[H + 7] = k.sizeClipSpace.x, t[H + 8] = k.sizeClipSpace.y), this._optionsService.rawOptions.rescaleOverlappingGlyphs && mn(o, c, k.size.x, this._dimensions.device.cell.width) && (t[H + 2] = (this._dimensions.device.cell.width - 1) / this._dimensions.device.canvas.width));
  }
  clear() {
    let t = this._terminal, n = t.cols * t.rows * De;
    this._vertices.count !== n ? this._vertices.attributes = new Float32Array(n) : this._vertices.attributes.fill(0);
    let s = 0;
    for (;s < this._vertices.attributesBuffers.length; s++)
      this._vertices.count !== n ? this._vertices.attributesBuffers[s] = new Float32Array(n) : this._vertices.attributesBuffers[s].fill(0);
    this._vertices.count = n, s = 0;
    for (let o = 0;o < t.rows; o++)
      for (let r = 0;r < t.cols; r++)
        this._vertices.attributes[s + 9] = r / t.cols, this._vertices.attributes[s + 10] = o / t.rows, s += De;
  }
  handleResize() {
    let t = this._gl;
    t.useProgram(this._program), t.viewport(0, 0, t.canvas.width, t.canvas.height), t.uniform2f(this._resolutionLocation, t.canvas.width, t.canvas.height), this.clear();
  }
  render(t) {
    if (!this._atlas)
      return;
    let n = this._gl;
    n.useProgram(this._program), n.bindVertexArray(this._vertexArrayObject), this._activeBuffer = (this._activeBuffer + 1) % 2;
    let s = this._vertices.attributesBuffers[this._activeBuffer], o = 0;
    for (let r = 0;r < t.lineLengths.length; r++) {
      let a = r * this._terminal.cols * De, l = this._vertices.attributes.subarray(a, a + t.lineLengths[r] * De);
      s.set(l, o), o += l.length;
    }
    n.bindBuffer(n.ARRAY_BUFFER, this._attributesBuffer), n.bufferData(n.ARRAY_BUFFER, s.subarray(0, o), n.STREAM_DRAW);
    for (let r = 0;r < this._atlas.pages.length; r++)
      this._atlas.pages[r].version !== this._atlasTextures[r].version && this._bindAtlasPageTexture(n, this._atlas, r);
    n.drawElementsInstanced(n.TRIANGLE_STRIP, 4, n.UNSIGNED_BYTE, 0, o / De);
  }
  setAtlas(t) {
    this._atlas = t;
    for (let n of this._atlasTextures)
      n.version = -1;
  }
  _bindAtlasPageTexture(t, n, s) {
    t.activeTexture(t.TEXTURE0 + s), t.bindTexture(t.TEXTURE_2D, this._atlasTextures[s].texture), t.texParameteri(t.TEXTURE_2D, t.TEXTURE_WRAP_S, t.CLAMP_TO_EDGE), t.texParameteri(t.TEXTURE_2D, t.TEXTURE_WRAP_T, t.CLAMP_TO_EDGE), t.texImage2D(t.TEXTURE_2D, 0, t.RGBA, t.RGBA, t.UNSIGNED_BYTE, n.pages[s].canvas), t.generateMipmap(t.TEXTURE_2D), this._atlasTextures[s].version = n.pages[s].version;
  }
  setDimensions(t) {
    this._dimensions = t;
  }
};
var ki = class {
  constructor() {
    this.clear();
  }
  clear() {
    this.hasSelection = false, this.columnSelectMode = false, this.viewportStartRow = 0, this.viewportEndRow = 0, this.viewportCappedStartRow = 0, this.viewportCappedEndRow = 0, this.startCol = 0, this.endCol = 0, this.selectionStart = undefined, this.selectionEnd = undefined;
  }
  update(e, t, n, s = false) {
    if (this.selectionStart = t, this.selectionEnd = n, !t || !n || t[0] === n[0] && t[1] === n[1]) {
      this.clear();
      return;
    }
    let o = e.buffers.active.ydisp, r = t[1] - o, a = n[1] - o, l = Math.max(r, 0), u = Math.min(a, e.rows - 1);
    if (l >= e.rows || u < 0) {
      this.clear();
      return;
    }
    this.hasSelection = true, this.columnSelectMode = s, this.viewportStartRow = r, this.viewportEndRow = a, this.viewportCappedStartRow = l, this.viewportCappedEndRow = u, this.startCol = t[0], this.endCol = n[0];
  }
  isCellSelected(e, t, n) {
    return this.hasSelection ? (n -= e.buffer.active.viewportY, this.columnSelectMode ? this.startCol <= this.endCol ? t >= this.startCol && n >= this.viewportCappedStartRow && t < this.endCol && n <= this.viewportCappedEndRow : t < this.startCol && n >= this.viewportCappedStartRow && t >= this.endCol && n <= this.viewportCappedEndRow : n > this.viewportStartRow && n < this.viewportEndRow || this.viewportStartRow === this.viewportEndRow && n === this.viewportStartRow && t >= this.startCol && t < this.endCol || this.viewportStartRow < this.viewportEndRow && n === this.viewportEndRow && t < this.endCol || this.viewportStartRow < this.viewportEndRow && n === this.viewportStartRow && t >= this.startCol) : false;
  }
};
function Nn() {
  return new ki;
}
var Ce = 4;
var ze = 1;
var qe = 2;
var Ct = 3;
var Un = 2147483648;
var Vt = class {
  constructor() {
    this.cells = new Uint32Array(0), this.lineLengths = new Uint32Array(0), this.selection = Nn();
  }
  resize(e, t) {
    let n = e * t * Ce;
    n !== this.cells.length && (this.cells = new Uint32Array(n), this.lineLengths = new Uint32Array(t));
  }
  clear() {
    this.cells.fill(0, 0), this.lineLengths.fill(0, 0);
  }
};
var ss = `#version 300 es
layout (location = 0) in vec2 a_position;
layout (location = 1) in vec2 a_size;
layout (location = 2) in vec4 a_color;
layout (location = 3) in vec2 a_unitquad;

uniform mat4 u_projection;

out vec4 v_color;

void main() {
  vec2 zeroToOne = a_position + (a_unitquad * a_size);
  gl_Position = u_projection * vec4(zeroToOne, 0.0, 1.0);
  v_color = a_color;
}`;
var os = `#version 300 es
precision lowp float;

in vec4 v_color;

out vec4 outColor;

void main() {
  outColor = v_color;
}`;
var Ee = 8;
var Pi = Ee * Float32Array.BYTES_PER_ELEMENT;
var as = 20 * Ee;
var zt = class {
  constructor() {
    this.attributes = new Float32Array(as), this.count = 0;
  }
};
var xe = 0;
var Hn = 0;
var Wn = 0;
var Gn = 0;
var $n = 0;
var Kn = 0;
var Vn = 0;
var qt = class extends B {
  constructor(t, n, s, o) {
    super();
    this._terminal = t;
    this._gl = n;
    this._dimensions = s;
    this._themeService = o;
    this._vertices = new zt;
    this._verticesCursor = new zt;
    let r = this._gl;
    this._program = F($t(r, ss, os)), this._register(O(() => r.deleteProgram(this._program))), this._projectionLocation = F(r.getUniformLocation(this._program, "u_projection")), this._vertexArrayObject = r.createVertexArray(), r.bindVertexArray(this._vertexArrayObject);
    let a = new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]), l = r.createBuffer();
    this._register(O(() => r.deleteBuffer(l))), r.bindBuffer(r.ARRAY_BUFFER, l), r.bufferData(r.ARRAY_BUFFER, a, r.STATIC_DRAW), r.enableVertexAttribArray(3), r.vertexAttribPointer(3, 2, this._gl.FLOAT, false, 0, 0);
    let u = new Uint8Array([0, 1, 2, 3]), c = r.createBuffer();
    this._register(O(() => r.deleteBuffer(c))), r.bindBuffer(r.ELEMENT_ARRAY_BUFFER, c), r.bufferData(r.ELEMENT_ARRAY_BUFFER, u, r.STATIC_DRAW), this._attributesBuffer = F(r.createBuffer()), this._register(O(() => r.deleteBuffer(this._attributesBuffer))), r.bindBuffer(r.ARRAY_BUFFER, this._attributesBuffer), r.enableVertexAttribArray(0), r.vertexAttribPointer(0, 2, r.FLOAT, false, Pi, 0), r.vertexAttribDivisor(0, 1), r.enableVertexAttribArray(1), r.vertexAttribPointer(1, 2, r.FLOAT, false, Pi, 2 * Float32Array.BYTES_PER_ELEMENT), r.vertexAttribDivisor(1, 1), r.enableVertexAttribArray(2), r.vertexAttribPointer(2, 4, r.FLOAT, false, Pi, 4 * Float32Array.BYTES_PER_ELEMENT), r.vertexAttribDivisor(2, 1), this._updateCachedColors(o.colors), this._register(this._themeService.onChangeColors((d) => {
      this._updateCachedColors(d), this._updateViewportRectangle();
    }));
  }
  renderBackgrounds() {
    this._renderVertices(this._vertices);
  }
  renderCursor() {
    this._renderVertices(this._verticesCursor);
  }
  _renderVertices(t) {
    let n = this._gl;
    n.useProgram(this._program), n.bindVertexArray(this._vertexArrayObject), n.uniformMatrix4fv(this._projectionLocation, false, Gt), n.bindBuffer(n.ARRAY_BUFFER, this._attributesBuffer), n.bufferData(n.ARRAY_BUFFER, t.attributes, n.DYNAMIC_DRAW), n.drawElementsInstanced(this._gl.TRIANGLE_STRIP, 4, n.UNSIGNED_BYTE, 0, t.count);
  }
  handleResize() {
    this._updateViewportRectangle();
  }
  setDimensions(t) {
    this._dimensions = t;
  }
  _updateCachedColors(t) {
    this._bgFloat = this._colorToFloat32Array(t.background), this._cursorFloat = this._colorToFloat32Array(t.cursor);
  }
  _updateViewportRectangle() {
    this._addRectangleFloat(this._vertices.attributes, 0, 0, 0, this._terminal.cols * this._dimensions.device.cell.width, this._terminal.rows * this._dimensions.device.cell.height, this._bgFloat);
  }
  updateBackgrounds(t) {
    let n = this._terminal, s = this._vertices, o = 1, r, a, l, u, c, d, h, f, I, L, M;
    for (r = 0;r < n.rows; r++) {
      for (l = -1, u = 0, c = 0, d = false, a = 0;a < n.cols; a++)
        h = (r * n.cols + a) * Ce, f = t.cells[h + ze], I = t.cells[h + qe], L = !!(I & 67108864), (f !== u || I !== c && (d || L)) && ((u !== 0 || d && c !== 0) && (M = o++ * Ee, this._updateRectangle(s, M, c, u, l, a, r)), l = a, u = f, c = I, d = L);
      (u !== 0 || d && c !== 0) && (M = o++ * Ee, this._updateRectangle(s, M, c, u, l, n.cols, r));
    }
    s.count = o;
  }
  updateCursor(t) {
    let n = this._verticesCursor, s = t.cursor;
    if (!s || s.style === "block") {
      n.count = 0;
      return;
    }
    let o, r = 0;
    (s.style === "bar" || s.style === "outline") && (o = r++ * Ee, this._addRectangleFloat(n.attributes, o, s.x * this._dimensions.device.cell.width, s.y * this._dimensions.device.cell.height, s.style === "bar" ? s.dpr * s.cursorWidth : s.dpr, this._dimensions.device.cell.height, this._cursorFloat)), (s.style === "underline" || s.style === "outline") && (o = r++ * Ee, this._addRectangleFloat(n.attributes, o, s.x * this._dimensions.device.cell.width, (s.y + 1) * this._dimensions.device.cell.height - s.dpr, s.width * this._dimensions.device.cell.width, s.dpr, this._cursorFloat)), s.style === "outline" && (o = r++ * Ee, this._addRectangleFloat(n.attributes, o, s.x * this._dimensions.device.cell.width, s.y * this._dimensions.device.cell.height, s.width * this._dimensions.device.cell.width, s.dpr, this._cursorFloat), o = r++ * Ee, this._addRectangleFloat(n.attributes, o, (s.x + s.width) * this._dimensions.device.cell.width - s.dpr, s.y * this._dimensions.device.cell.height, s.dpr, this._dimensions.device.cell.height, this._cursorFloat)), n.count = r;
  }
  _updateRectangle(t, n, s, o, r, a, l) {
    if (s & 67108864)
      switch (s & 50331648) {
        case 16777216:
        case 33554432:
          xe = this._themeService.colors.ansi[s & 255].rgba;
          break;
        case 50331648:
          xe = (s & 16777215) << 8;
          break;
        case 0:
        default:
          xe = this._themeService.colors.foreground.rgba;
      }
    else
      switch (o & 50331648) {
        case 16777216:
        case 33554432:
          xe = this._themeService.colors.ansi[o & 255].rgba;
          break;
        case 50331648:
          xe = (o & 16777215) << 8;
          break;
        case 0:
        default:
          xe = this._themeService.colors.background.rgba;
      }
    t.attributes.length < n + 4 && (t.attributes = Bn(t.attributes, this._terminal.rows * this._terminal.cols * Ee)), Hn = r * this._dimensions.device.cell.width, Wn = l * this._dimensions.device.cell.height, Gn = (xe >> 24 & 255) / 255, $n = (xe >> 16 & 255) / 255, Kn = (xe >> 8 & 255) / 255, Vn = 1, this._addRectangle(t.attributes, n, Hn, Wn, (a - r) * this._dimensions.device.cell.width, this._dimensions.device.cell.height, Gn, $n, Kn, Vn);
  }
  _addRectangle(t, n, s, o, r, a, l, u, c, d) {
    t[n] = s / this._dimensions.device.canvas.width, t[n + 1] = o / this._dimensions.device.canvas.height, t[n + 2] = r / this._dimensions.device.canvas.width, t[n + 3] = a / this._dimensions.device.canvas.height, t[n + 4] = l, t[n + 5] = u, t[n + 6] = c, t[n + 7] = d;
  }
  _addRectangleFloat(t, n, s, o, r, a, l) {
    t[n] = s / this._dimensions.device.canvas.width, t[n + 1] = o / this._dimensions.device.canvas.height, t[n + 2] = r / this._dimensions.device.canvas.width, t[n + 3] = a / this._dimensions.device.canvas.height, t[n + 4] = l[0], t[n + 5] = l[1], t[n + 6] = l[2], t[n + 7] = l[3];
  }
  _colorToFloat32Array(t) {
    return new Float32Array([(t.rgba >> 24 & 255) / 255, (t.rgba >> 16 & 255) / 255, (t.rgba >> 8 & 255) / 255, (t.rgba & 255) / 255]);
  }
};
var jt = class extends B {
  constructor(t, n, s, o, r, a, l, u) {
    super();
    this._container = n;
    this._alpha = r;
    this._coreBrowserService = a;
    this._optionsService = l;
    this._themeService = u;
    this._deviceCharWidth = 0;
    this._deviceCharHeight = 0;
    this._deviceCellWidth = 0;
    this._deviceCellHeight = 0;
    this._deviceCharLeft = 0;
    this._deviceCharTop = 0;
    this._canvas = this._coreBrowserService.mainDocument.createElement("canvas"), this._canvas.classList.add(`xterm-${s}-layer`), this._canvas.style.zIndex = o.toString(), this._initCanvas(), this._container.appendChild(this._canvas), this._register(this._themeService.onChangeColors((c) => {
      this._refreshCharAtlas(t, c), this.reset(t);
    })), this._register(O(() => {
      this._canvas.remove();
    }));
  }
  _initCanvas() {
    this._ctx = F(this._canvas.getContext("2d", { alpha: this._alpha })), this._alpha || this._clearAll();
  }
  handleBlur(t) {}
  handleFocus(t) {}
  handleCursorMove(t) {}
  handleGridChanged(t, n, s) {}
  handleSelectionChanged(t, n, s, o = false) {}
  _setTransparency(t, n) {
    if (n === this._alpha)
      return;
    let s = this._canvas;
    this._alpha = n, this._canvas = this._canvas.cloneNode(), this._initCanvas(), this._container.replaceChild(this._canvas, s), this._refreshCharAtlas(t, this._themeService.colors), this.handleGridChanged(t, 0, t.rows - 1);
  }
  _refreshCharAtlas(t, n) {
    this._deviceCharWidth <= 0 && this._deviceCharHeight <= 0 || (this._charAtlas = Nt(t, this._optionsService.rawOptions, n, this._deviceCellWidth, this._deviceCellHeight, this._deviceCharWidth, this._deviceCharHeight, this._coreBrowserService.dpr, 2048), this._charAtlas.warmUp());
  }
  resize(t, n) {
    this._deviceCellWidth = n.device.cell.width, this._deviceCellHeight = n.device.cell.height, this._deviceCharWidth = n.device.char.width, this._deviceCharHeight = n.device.char.height, this._deviceCharLeft = n.device.char.left, this._deviceCharTop = n.device.char.top, this._canvas.width = n.device.canvas.width, this._canvas.height = n.device.canvas.height, this._canvas.style.width = `${n.css.canvas.width}px`, this._canvas.style.height = `${n.css.canvas.height}px`, this._alpha || this._clearAll(), this._refreshCharAtlas(t, this._themeService.colors);
  }
  _fillBottomLineAtCells(t, n, s = 1) {
    this._ctx.fillRect(t * this._deviceCellWidth, (n + 1) * this._deviceCellHeight - this._coreBrowserService.dpr - 1, s * this._deviceCellWidth, this._coreBrowserService.dpr);
  }
  _clearAll() {
    this._alpha ? this._ctx.clearRect(0, 0, this._canvas.width, this._canvas.height) : (this._ctx.fillStyle = this._themeService.colors.background.css, this._ctx.fillRect(0, 0, this._canvas.width, this._canvas.height));
  }
  _clearCells(t, n, s, o) {
    this._alpha ? this._ctx.clearRect(t * this._deviceCellWidth, n * this._deviceCellHeight, s * this._deviceCellWidth, o * this._deviceCellHeight) : (this._ctx.fillStyle = this._themeService.colors.background.css, this._ctx.fillRect(t * this._deviceCellWidth, n * this._deviceCellHeight, s * this._deviceCellWidth, o * this._deviceCellHeight));
  }
  _fillCharTrueColor(t, n, s, o) {
    this._ctx.font = this._getFont(t, false, false), this._ctx.textBaseline = St, this._clipCell(s, o, n.getWidth()), this._ctx.fillText(n.getChars(), s * this._deviceCellWidth + this._deviceCharLeft, o * this._deviceCellHeight + this._deviceCharTop + this._deviceCharHeight);
  }
  _clipCell(t, n, s) {
    this._ctx.beginPath(), this._ctx.rect(t * this._deviceCellWidth, n * this._deviceCellHeight, s * this._deviceCellWidth, this._deviceCellHeight), this._ctx.clip();
  }
  _getFont(t, n, s) {
    let o = n ? t.options.fontWeightBold : t.options.fontWeight;
    return `${s ? "italic" : ""} ${o} ${t.options.fontSize * this._coreBrowserService.dpr}px ${t.options.fontFamily}`;
  }
};
var Xt = class extends jt {
  constructor(e, t, n, s, o, r, a) {
    super(n, e, "link", t, true, o, r, a), this._register(s.onShowLinkUnderline((l) => this._handleShowLinkUnderline(l))), this._register(s.onHideLinkUnderline((l) => this._handleHideLinkUnderline(l)));
  }
  resize(e, t) {
    super.resize(e, t), this._state = undefined;
  }
  reset(e) {
    this._clearCurrentLink();
  }
  _clearCurrentLink() {
    if (this._state) {
      this._clearCells(this._state.x1, this._state.y1, this._state.cols - this._state.x1, 1);
      let e = this._state.y2 - this._state.y1 - 1;
      e > 0 && this._clearCells(0, this._state.y1 + 1, this._state.cols, e), this._clearCells(0, this._state.y2, this._state.x2, 1), this._state = undefined;
    }
  }
  _handleShowLinkUnderline(e) {
    if (e.fg === 257 ? this._ctx.fillStyle = this._themeService.colors.background.css : e.fg !== undefined && Fn(e.fg) ? this._ctx.fillStyle = this._themeService.colors.ansi[e.fg].css : this._ctx.fillStyle = this._themeService.colors.foreground.css, e.y1 === e.y2)
      this._fillBottomLineAtCells(e.x1, e.y1, e.x2 - e.x1);
    else {
      this._fillBottomLineAtCells(e.x1, e.y1, e.cols - e.x1);
      for (let t = e.y1 + 1;t < e.y2; t++)
        this._fillBottomLineAtCells(0, t, e.cols);
      this._fillBottomLineAtCells(0, e.y2, e.x2);
    }
    this._state = e;
  }
  _handleHideLinkUnderline(e) {
    this._clearCurrentLink();
  }
};
var te = typeof window == "object" ? window : globalThis;
var Zt = class Zt {
  constructor() {
    this.mapWindowIdToZoomLevel = new Map;
    this._onDidChangeZoomLevel = new D;
    this.onDidChangeZoomLevel = this._onDidChangeZoomLevel.event;
    this.mapWindowIdToZoomFactor = new Map;
    this._onDidChangeFullscreen = new D;
    this.onDidChangeFullscreen = this._onDidChangeFullscreen.event;
    this.mapWindowIdToFullScreen = new Map;
  }
  getZoomLevel(e) {
    return this.mapWindowIdToZoomLevel.get(this.getWindowId(e)) ?? 0;
  }
  setZoomLevel(e, t) {
    if (this.getZoomLevel(t) === e)
      return;
    let n = this.getWindowId(t);
    this.mapWindowIdToZoomLevel.set(n, e), this._onDidChangeZoomLevel.fire(n);
  }
  getZoomFactor(e) {
    return this.mapWindowIdToZoomFactor.get(this.getWindowId(e)) ?? 1;
  }
  setZoomFactor(e, t) {
    this.mapWindowIdToZoomFactor.set(this.getWindowId(t), e);
  }
  setFullscreen(e, t) {
    if (this.isFullscreen(t) === e)
      return;
    let n = this.getWindowId(t);
    this.mapWindowIdToFullScreen.set(n, e), this._onDidChangeFullscreen.fire(n);
  }
  isFullscreen(e) {
    return !!this.mapWindowIdToFullScreen.get(this.getWindowId(e));
  }
  getWindowId(e) {
    return e.vscodeWindowId;
  }
};
Zt.INSTANCE = new Zt;
var Qt = Zt;
function us(i, e, t) {
  typeof e == "string" && (e = i.matchMedia(e)), e.addEventListener("change", t);
}
var Wa = Qt.INSTANCE.onDidChangeZoomLevel;
var Ga = Qt.INSTANCE.onDidChangeFullscreen;
var je = typeof navigator == "object" ? navigator.userAgent : "";
var Cn = je.indexOf("Firefox") >= 0;
var ut = je.indexOf("AppleWebKit") >= 0;
var zn = je.indexOf("Chrome") >= 0;
var Bi = !zn && je.indexOf("Safari") >= 0;
var $a = je.indexOf("Electron/") >= 0;
var Ka = je.indexOf("Android") >= 0;
var Yt = false;
if (typeof te.matchMedia == "function") {
  let i = te.matchMedia("(display-mode: standalone) or (display-mode: window-controls-overlay)"), e = te.matchMedia("(display-mode: fullscreen)");
  Yt = i.matches, us(te, i, ({ matches: t }) => {
    Yt && e.matches || (Yt = t);
  });
}
function qn() {
  return Yt;
}
var Xe = "en";
var Ui = false;
var ni = false;
var ti = false;
var cs = false;
var Xn = false;
var Yn = false;
var ds = false;
var hs = false;
var ps = false;
var fs = false;
var ei;
var ii = Xe;
var jn = Xe;
var ms;
var ye;
var Ie = globalThis;
var re;
typeof Ie.vscode < "u" && typeof Ie.vscode.process < "u" ? re = Ie.vscode.process : typeof process < "u" && typeof process?.versions?.node == "string" && (re = process);
var Qn = typeof re?.versions?.electron == "string";
var _s = Qn && re?.type === "renderer";
if (typeof re == "object") {
  Ui = re.platform === "win32", ni = re.platform === "darwin", ti = re.platform === "linux", cs = ti && !!re.env.SNAP && !!re.env.SNAP_REVISION, ds = Qn, ps = !!re.env.CI || !!re.env.BUILD_ARTIFACTSTAGINGDIRECTORY, ei = Xe, ii = Xe;
  let i = re.env.VSCODE_NLS_CONFIG;
  if (i)
    try {
      let e = JSON.parse(i);
      ei = e.userLocale, jn = e.osLocale, ii = e.resolvedLanguage || Xe, ms = e.languagePack?.translationsConfigFile;
    } catch {}
  Xn = true;
} else
  typeof navigator == "object" && !_s ? (ye = navigator.userAgent, Ui = ye.indexOf("Windows") >= 0, ni = ye.indexOf("Macintosh") >= 0, hs = (ye.indexOf("Macintosh") >= 0 || ye.indexOf("iPad") >= 0 || ye.indexOf("iPhone") >= 0) && !!navigator.maxTouchPoints && navigator.maxTouchPoints > 0, ti = ye.indexOf("Linux") >= 0, fs = ye?.indexOf("Mobi") >= 0, Yn = true, ii = globalThis._VSCODE_NLS_LANGUAGE || Xe, ei = navigator.language.toLowerCase(), jn = ei) : console.error("Unable to resolve platform.");
var Ni = 0;
ni ? Ni = 1 : Ui ? Ni = 3 : ti && (Ni = 2);
var ri = Xn;
var bs = Yn && typeof Ie.importScripts == "function";
var Va = bs ? Ie.origin : undefined;
var _e = ye;
var Me = ii;
var vs;
((n) => {
  function i() {
    return Me;
  }
  n.value = i;
  function e() {
    return Me.length === 2 ? Me === "en" : Me.length >= 3 ? Me[0] === "e" && Me[1] === "n" && Me[2] === "-" : false;
  }
  n.isDefaultVariant = e;
  function t() {
    return Me === "en";
  }
  n.isDefault = t;
})(vs ||= {});
var Ts = typeof Ie.postMessage == "function" && !Ie.importScripts;
var Zn = (() => {
  if (Ts) {
    let i = [];
    Ie.addEventListener("message", (t) => {
      if (t.data && t.data.vscodeScheduleAsyncWork)
        for (let n = 0, s = i.length;n < s; n++) {
          let o = i[n];
          if (o.id === t.data.vscodeScheduleAsyncWork) {
            i.splice(n, 1), o.callback();
            return;
          }
        }
    });
    let e = 0;
    return (t) => {
      let n = ++e;
      i.push({ id: n, callback: t }), Ie.postMessage({ vscodeScheduleAsyncWork: n }, "*");
    };
  }
  return (i) => setTimeout(i);
})();
var gs = !!(_e && _e.indexOf("Chrome") >= 0);
var Ca = !!(_e && _e.indexOf("Firefox") >= 0);
var za = !!(!gs && _e && _e.indexOf("Safari") >= 0);
var qa = !!(_e && _e.indexOf("Edg/") >= 0);
var ja = !!(_e && _e.indexOf("Android") >= 0);
var Ae = typeof navigator == "object" ? navigator : {};
var xs = { clipboard: { writeText: ri || document.queryCommandSupported && document.queryCommandSupported("copy") || !!(Ae && Ae.clipboard && Ae.clipboard.writeText), readText: ri || !!(Ae && Ae.clipboard && Ae.clipboard.readText) }, keyboard: ri || qn() ? 0 : Ae.keyboard || Bi ? 1 : 2, touch: "ontouchstart" in te || Ae.maxTouchPoints > 0, pointerEvents: te.PointerEvent && (("ontouchstart" in te) || navigator.maxTouchPoints > 0) };
var dt = class {
  constructor() {
    this._keyCodeToStr = [], this._strToKeyCode = Object.create(null);
  }
  define(e, t) {
    this._keyCodeToStr[e] = t, this._strToKeyCode[t.toLowerCase()] = e;
  }
  keyCodeToStr(e) {
    return this._keyCodeToStr[e];
  }
  strToKeyCode(e) {
    return this._strToKeyCode[e.toLowerCase()] || 0;
  }
};
var Hi = new dt;
var Jn = new dt;
var er = new dt;
var Es = new Array(230);
var tr;
((r) => {
  function i(a) {
    return Hi.keyCodeToStr(a);
  }
  r.toString = i;
  function e(a) {
    return Hi.strToKeyCode(a);
  }
  r.fromString = e;
  function t(a) {
    return Jn.keyCodeToStr(a);
  }
  r.toUserSettingsUS = t;
  function n(a) {
    return er.keyCodeToStr(a);
  }
  r.toUserSettingsGeneral = n;
  function s(a) {
    return Jn.strToKeyCode(a) || er.strToKeyCode(a);
  }
  r.fromUserSettings = s;
  function o(a) {
    if (a >= 98 && a <= 113)
      return null;
    switch (a) {
      case 16:
        return "Up";
      case 18:
        return "Down";
      case 15:
        return "Left";
      case 17:
        return "Right";
    }
    return Hi.keyCodeToStr(a);
  }
  r.toElectronAccelerator = o;
})(tr ||= {});
var nr = Object.freeze(function(i, e) {
  let t = setTimeout(i.bind(e), 0);
  return { dispose() {
    clearTimeout(t);
  } };
});
var Is;
((n) => {
  function i(s) {
    return s === n.None || s === n.Cancelled || s instanceof Wi ? true : !s || typeof s != "object" ? false : typeof s.isCancellationRequested == "boolean" && typeof s.onCancellationRequested == "function";
  }
  n.isCancellationToken = i, n.None = Object.freeze({ isCancellationRequested: false, onCancellationRequested: ee.None }), n.Cancelled = Object.freeze({ isCancellationRequested: true, onCancellationRequested: nr });
})(Is ||= {});
var Wi = class {
  constructor() {
    this._isCancelled = false;
    this._emitter = null;
  }
  cancel() {
    this._isCancelled || (this._isCancelled = true, this._emitter && (this._emitter.fire(undefined), this.dispose()));
  }
  get isCancellationRequested() {
    return this._isCancelled;
  }
  get onCancellationRequested() {
    return this._isCancelled ? nr : (this._emitter || (this._emitter = new D), this._emitter.event);
  }
  dispose() {
    this._emitter && (this._emitter.dispose(), this._emitter = null);
  }
};
var Ls = Symbol("MicrotaskDelay");
var ws;
var oi;
(function() {
  typeof globalThis.requestIdleCallback != "function" || typeof globalThis.cancelIdleCallback != "function" ? oi = (i, e) => {
    Zn(() => {
      if (t)
        return;
      let n = Date.now() + 15;
      e(Object.freeze({ didTimeout: true, timeRemaining() {
        return Math.max(0, n - Date.now());
      } }));
    });
    let t = false;
    return { dispose() {
      t || (t = true);
    } };
  } : oi = (i, e, t) => {
    let n = i.requestIdleCallback(e, typeof t == "number" ? { timeout: t } : undefined), s = false;
    return { dispose() {
      s || (s = true, i.cancelIdleCallback(n));
    } };
  }, ws = (i) => oi(globalThis, i);
})();
var Rs;
((t) => {
  async function i(n) {
    let s, o = await Promise.all(n.map((r) => r.then((a) => a, (a) => {
      s || (s = a);
    })));
    if (typeof s < "u")
      throw s;
    return o;
  }
  t.settled = i;
  function e(n) {
    return new Promise(async (s, o) => {
      try {
        await n(s, o);
      } catch (r) {
        o(r);
      }
    });
  }
  t.withAsyncBody = e;
})(Rs ||= {});
var Q = class Q {
  static fromArray(e) {
    return new Q((t) => {
      t.emitMany(e);
    });
  }
  static fromPromise(e) {
    return new Q(async (t) => {
      t.emitMany(await e);
    });
  }
  static fromPromises(e) {
    return new Q(async (t) => {
      await Promise.all(e.map(async (n) => t.emitOne(await n)));
    });
  }
  static merge(e) {
    return new Q(async (t) => {
      await Promise.all(e.map(async (n) => {
        for await (let s of n)
          t.emitOne(s);
      }));
    });
  }
  constructor(e, t) {
    this._state = 0, this._results = [], this._error = null, this._onReturn = t, this._onStateChanged = new D, queueMicrotask(async () => {
      let n = { emitOne: (s) => this.emitOne(s), emitMany: (s) => this.emitMany(s), reject: (s) => this.reject(s) };
      try {
        await Promise.resolve(e(n)), this.resolve();
      } catch (s) {
        this.reject(s);
      } finally {
        n.emitOne = undefined, n.emitMany = undefined, n.reject = undefined;
      }
    });
  }
  [Symbol.asyncIterator]() {
    let e = 0;
    return { next: async () => {
      do {
        if (this._state === 2)
          throw this._error;
        if (e < this._results.length)
          return { done: false, value: this._results[e++] };
        if (this._state === 1)
          return { done: true, value: undefined };
        await ee.toPromise(this._onStateChanged.event);
      } while (true);
    }, return: async () => (this._onReturn?.(), { done: true, value: undefined }) };
  }
  static map(e, t) {
    return new Q(async (n) => {
      for await (let s of e)
        n.emitOne(t(s));
    });
  }
  map(e) {
    return Q.map(this, e);
  }
  static filter(e, t) {
    return new Q(async (n) => {
      for await (let s of e)
        t(s) && n.emitOne(s);
    });
  }
  filter(e) {
    return Q.filter(this, e);
  }
  static coalesce(e) {
    return Q.filter(e, (t) => !!t);
  }
  coalesce() {
    return Q.coalesce(this);
  }
  static async toPromise(e) {
    let t = [];
    for await (let n of e)
      t.push(n);
    return t;
  }
  toPromise() {
    return Q.toPromise(this);
  }
  emitOne(e) {
    this._state === 0 && (this._results.push(e), this._onStateChanged.fire());
  }
  emitMany(e) {
    this._state === 0 && (this._results = this._results.concat(e), this._onStateChanged.fire());
  }
  resolve() {
    this._state === 0 && (this._state = 1, this._onStateChanged.fire());
  }
  reject(e) {
    this._state === 0 && (this._state = 2, this._error = e, this._onStateChanged.fire());
  }
};
Q.EMPTY = Q.fromArray([]);
function sr(i) {
  return 55296 <= i && i <= 56319;
}
function Gi(i) {
  return 56320 <= i && i <= 57343;
}
function or(i, e) {
  return (i - 55296 << 10) + (e - 56320) + 65536;
}
function ur(i) {
  return Ki(i, 0);
}
function Ki(i, e) {
  switch (typeof i) {
    case "object":
      return i === null ? Le(349, e) : Array.isArray(i) ? As(i, e) : Ss(i, e);
    case "string":
      return cr(i, e);
    case "boolean":
      return Ms(i, e);
    case "number":
      return Le(i, e);
    case "undefined":
      return Le(937, e);
    default:
      return Le(617, e);
  }
}
function Le(i, e) {
  return (e << 5) - e + i | 0;
}
function Ms(i, e) {
  return Le(i ? 433 : 863, e);
}
function cr(i, e) {
  e = Le(149417, e);
  for (let t = 0, n = i.length;t < n; t++)
    e = Le(i.charCodeAt(t), e);
  return e;
}
function As(i, e) {
  return e = Le(104579, e), i.reduce((t, n) => Ki(n, t), e);
}
function Ss(i, e) {
  return e = Le(181387, e), Object.keys(i).sort().reduce((t, n) => (t = cr(n, t), Ki(i[n], t)), e);
}
function $i(i, e, t = 32) {
  let n = t - e, s = ~((1 << n) - 1);
  return (i << e | (s & i) >>> n) >>> 0;
}
function ar(i, e = 0, t = i.byteLength, n = 0) {
  for (let s = 0;s < t; s++)
    i[e + s] = n;
}
function Os(i, e, t = "0") {
  for (;i.length < e; )
    i = t + i;
  return i;
}
function ht(i, e = 32) {
  return i instanceof ArrayBuffer ? Array.from(new Uint8Array(i)).map((t) => t.toString(16).padStart(2, "0")).join("") : Os((i >>> 0).toString(16), e / 4);
}
var ai = class ai {
  constructor() {
    this._h0 = 1732584193;
    this._h1 = 4023233417;
    this._h2 = 2562383102;
    this._h3 = 271733878;
    this._h4 = 3285377520;
    this._buff = new Uint8Array(67), this._buffDV = new DataView(this._buff.buffer), this._buffLen = 0, this._totalLen = 0, this._leftoverHighSurrogate = 0, this._finished = false;
  }
  update(e) {
    let t = e.length;
    if (t === 0)
      return;
    let n = this._buff, s = this._buffLen, o = this._leftoverHighSurrogate, r, a;
    for (o !== 0 ? (r = o, a = -1, o = 0) : (r = e.charCodeAt(0), a = 0);; ) {
      let l = r;
      if (sr(r))
        if (a + 1 < t) {
          let u = e.charCodeAt(a + 1);
          Gi(u) ? (a++, l = or(r, u)) : l = 65533;
        } else {
          o = r;
          break;
        }
      else
        Gi(r) && (l = 65533);
      if (s = this._push(n, s, l), a++, a < t)
        r = e.charCodeAt(a);
      else
        break;
    }
    this._buffLen = s, this._leftoverHighSurrogate = o;
  }
  _push(e, t, n) {
    return n < 128 ? e[t++] = n : n < 2048 ? (e[t++] = 192 | (n & 1984) >>> 6, e[t++] = 128 | (n & 63) >>> 0) : n < 65536 ? (e[t++] = 224 | (n & 61440) >>> 12, e[t++] = 128 | (n & 4032) >>> 6, e[t++] = 128 | (n & 63) >>> 0) : (e[t++] = 240 | (n & 1835008) >>> 18, e[t++] = 128 | (n & 258048) >>> 12, e[t++] = 128 | (n & 4032) >>> 6, e[t++] = 128 | (n & 63) >>> 0), t >= 64 && (this._step(), t -= 64, this._totalLen += 64, e[0] = e[64], e[1] = e[65], e[2] = e[66]), t;
  }
  digest() {
    return this._finished || (this._finished = true, this._leftoverHighSurrogate && (this._leftoverHighSurrogate = 0, this._buffLen = this._push(this._buff, this._buffLen, 65533)), this._totalLen += this._buffLen, this._wrapUp()), ht(this._h0) + ht(this._h1) + ht(this._h2) + ht(this._h3) + ht(this._h4);
  }
  _wrapUp() {
    this._buff[this._buffLen++] = 128, ar(this._buff, this._buffLen), this._buffLen > 56 && (this._step(), ar(this._buff));
    let e = 8 * this._totalLen;
    this._buffDV.setUint32(56, Math.floor(e / 4294967296), false), this._buffDV.setUint32(60, e % 4294967296, false), this._step();
  }
  _step() {
    let e = ai._bigBlock32, t = this._buffDV;
    for (let d = 0;d < 64; d += 4)
      e.setUint32(d, t.getUint32(d, false), false);
    for (let d = 64;d < 320; d += 4)
      e.setUint32(d, $i(e.getUint32(d - 12, false) ^ e.getUint32(d - 32, false) ^ e.getUint32(d - 56, false) ^ e.getUint32(d - 64, false), 1), false);
    let n = this._h0, s = this._h1, o = this._h2, r = this._h3, a = this._h4, l, u, c;
    for (let d = 0;d < 80; d++)
      d < 20 ? (l = s & o | ~s & r, u = 1518500249) : d < 40 ? (l = s ^ o ^ r, u = 1859775393) : d < 60 ? (l = s & o | s & r | o & r, u = 2400959708) : (l = s ^ o ^ r, u = 3395469782), c = $i(n, 5) + l + a + u + e.getUint32(d * 4, false) & 4294967295, a = r, r = o, o = $i(s, 30), s = n, n = c;
    this._h0 = this._h0 + n & 4294967295, this._h1 = this._h1 + s & 4294967295, this._h2 = this._h2 + o & 4294967295, this._h3 = this._h3 + r & 4294967295, this._h4 = this._h4 + a & 4294967295;
  }
};
ai._bigBlock32 = new DataView(new ArrayBuffer(320));
var { registerWindow: fu, getWindow: Fs, getDocument: mu, getWindows: _u, getWindowsCount: bu, getWindowId: dr, getWindowById: vu, hasWindow: Tu, onDidRegisterWindow: gu, onWillUnregisterWindow: xu, onDidUnregisterWindow: Eu } = function() {
  let i = new Map;
  let e = { window: te, disposables: new fe };
  i.set(te.vscodeWindowId, e);
  let t = new D, n = new D, s = new D;
  function o(r, a) {
    return (typeof r == "number" ? i.get(r) : undefined) ?? (a ? e : undefined);
  }
  return { onDidRegisterWindow: t.event, onWillUnregisterWindow: s.event, onDidUnregisterWindow: n.event, registerWindow(r) {
    if (i.has(r.vscodeWindowId))
      return B.None;
    let a = new fe, l = { window: r, disposables: a.add(new fe) };
    return i.set(r.vscodeWindowId, l), a.add(O(() => {
      i.delete(r.vscodeWindowId), n.fire(r);
    })), a.add(li(r, Ps.BEFORE_UNLOAD, () => {
      s.fire(r);
    })), t.fire(l), a;
  }, getWindows() {
    return i.values();
  }, getWindowsCount() {
    return i.size;
  }, getWindowId(r) {
    return r.vscodeWindowId;
  }, hasWindow(r) {
    return i.has(r);
  }, getWindowById: o, getWindow(r) {
    let a = r;
    if (a?.ownerDocument?.defaultView)
      return a.ownerDocument.defaultView.window;
    let l = r;
    return l?.view ? l.view.window : te;
  }, getDocument(r) {
    return Fs(r).document;
  } };
}();
var Vi = class {
  constructor(e, t, n, s) {
    this._node = e, this._type = t, this._handler = n, this._options = s || false, this._node.addEventListener(this._type, this._handler, this._options);
  }
  dispose() {
    this._handler && (this._node.removeEventListener(this._type, this._handler, this._options), this._node = null, this._handler = null);
  }
};
function li(i, e, t, n) {
  return new Vi(i, e, t, n);
}
var ks;
var hr;
var pt = class {
  constructor(e, t = 0) {
    this._runner = e, this.priority = t, this._canceled = false;
  }
  dispose() {
    this._canceled = true;
  }
  execute() {
    if (!this._canceled)
      try {
        this._runner();
      } catch (e) {
        Pe(e);
      }
  }
  static sort(e, t) {
    return t.priority - e.priority;
  }
};
(function() {
  let i = new Map, e = new Map, t = new Map, n = new Map, s = (o) => {
    t.set(o, false);
    let r = i.get(o) ?? [];
    for (e.set(o, r), i.set(o, []), n.set(o, true);r.length > 0; )
      r.sort(pt.sort), r.shift().execute();
    n.set(o, false);
  };
  hr = (o, r, a = 0) => {
    let l = dr(o), u = new pt(r, a), c = i.get(l);
    return c || (c = [], i.set(l, c)), c.push(u), t.get(l) || (t.set(l, true), o.requestAnimationFrame(() => s(l))), u;
  }, ks = (o, r, a) => {
    let l = dr(o);
    if (n.get(l)) {
      let u = new pt(r, a), c = e.get(l);
      return c || (c = [], e.set(l, c)), c.push(u), u;
    } else
      return hr(o, r, a);
  };
})();
var ke = class ke {
  constructor(e, t) {
    this.width = e;
    this.height = t;
  }
  with(e = this.width, t = this.height) {
    return e !== this.width || t !== this.height ? new ke(e, t) : this;
  }
  static is(e) {
    return typeof e == "object" && typeof e.height == "number" && typeof e.width == "number";
  }
  static lift(e) {
    return e instanceof ke ? e : new ke(e.width, e.height);
  }
  static equals(e, t) {
    return e === t ? true : !e || !t ? false : e.width === t.width && e.height === t.height;
  }
};
ke.None = new ke(0, 0);
var yu = new class {
  constructor() {
    this.mutationObservers = new Map;
  }
  observe(i, e, t) {
    let n = this.mutationObservers.get(i);
    n || (n = new Map, this.mutationObservers.set(i, n));
    let s = ur(t), o = n.get(s);
    if (o)
      o.users += 1;
    else {
      let r = new D, a = new MutationObserver((u) => r.fire(u));
      a.observe(i, t);
      let l = o = { users: 1, observer: a, onDidMutate: r.event };
      e.add(O(() => {
        l.users -= 1, l.users === 0 && (r.dispose(), a.disconnect(), n?.delete(s), n?.size === 0 && this.mutationObservers.delete(i));
      })), n.set(s, o);
    }
    return o.onDidMutate;
  }
};
var Ps = { CLICK: "click", AUXCLICK: "auxclick", DBLCLICK: "dblclick", MOUSE_UP: "mouseup", MOUSE_DOWN: "mousedown", MOUSE_OVER: "mouseover", MOUSE_MOVE: "mousemove", MOUSE_OUT: "mouseout", MOUSE_ENTER: "mouseenter", MOUSE_LEAVE: "mouseleave", MOUSE_WHEEL: "wheel", POINTER_UP: "pointerup", POINTER_DOWN: "pointerdown", POINTER_MOVE: "pointermove", POINTER_LEAVE: "pointerleave", CONTEXT_MENU: "contextmenu", WHEEL: "wheel", KEY_DOWN: "keydown", KEY_PRESS: "keypress", KEY_UP: "keyup", LOAD: "load", BEFORE_UNLOAD: "beforeunload", UNLOAD: "unload", PAGE_SHOW: "pageshow", PAGE_HIDE: "pagehide", PASTE: "paste", ABORT: "abort", ERROR: "error", RESIZE: "resize", SCROLL: "scroll", FULLSCREEN_CHANGE: "fullscreenchange", WK_FULLSCREEN_CHANGE: "webkitfullscreenchange", SELECT: "select", CHANGE: "change", SUBMIT: "submit", RESET: "reset", FOCUS: "focus", FOCUS_IN: "focusin", FOCUS_OUT: "focusout", BLUR: "blur", INPUT: "input", STORAGE: "storage", DRAG_START: "dragstart", DRAG: "drag", DRAG_ENTER: "dragenter", DRAG_LEAVE: "dragleave", DRAG_OVER: "dragover", DROP: "drop", DRAG_END: "dragend", ANIMATION_START: ut ? "webkitAnimationStart" : "animationstart", ANIMATION_END: ut ? "webkitAnimationEnd" : "animationend", ANIMATION_ITERATION: ut ? "webkitAnimationIteration" : "animationiteration" };
var Bs = /([\w\-]+)?(#([\w\-]+))?((\.([\w\-]+))*)/;
function fr(i, e, t, ...n) {
  let s = Bs.exec(e);
  if (!s)
    throw new Error("Bad use of emmet");
  let o = s[1] || "div", r;
  return i !== "http://www.w3.org/1999/xhtml" ? r = document.createElementNS(i, o) : r = document.createElement(o), s[3] && (r.id = s[3]), s[4] && (r.className = s[4].replace(/\./g, " ").trim()), t && Object.entries(t).forEach(([a, l]) => {
    typeof l > "u" || (/^on\w+$/.test(a) ? r[a] = l : a === "selected" ? l && r.setAttribute(a, "true") : r.setAttribute(a, l));
  }), r.append(...n), r;
}
function Ns(i, e, ...t) {
  return fr("http://www.w3.org/1999/xhtml", i, e, ...t);
}
Ns.SVG = function(i, e, ...t) {
  return fr("http://www.w3.org/2000/svg", i, e, ...t);
};
var ui = class extends B {
  constructor(t, n, s, o, r, a, l, u, c) {
    super();
    this._terminal = t;
    this._characterJoinerService = n;
    this._charSizeService = s;
    this._coreBrowserService = o;
    this._coreService = r;
    this._decorationService = a;
    this._optionsService = l;
    this._themeService = u;
    this._cursorBlinkStateManager = new be;
    this._charAtlasDisposable = this._register(new be);
    this._observerDisposable = this._register(new be);
    this._model = new Vt;
    this._workCell = new at;
    this._workCell2 = new at;
    this._rectangleRenderer = this._register(new be);
    this._glyphRenderer = this._register(new be);
    this._onChangeTextureAtlas = this._register(new D);
    this.onChangeTextureAtlas = this._onChangeTextureAtlas.event;
    this._onAddTextureAtlasCanvas = this._register(new D);
    this.onAddTextureAtlasCanvas = this._onAddTextureAtlasCanvas.event;
    this._onRemoveTextureAtlasCanvas = this._register(new D);
    this.onRemoveTextureAtlasCanvas = this._onRemoveTextureAtlasCanvas.event;
    this._onRequestRedraw = this._register(new D);
    this.onRequestRedraw = this._onRequestRedraw.event;
    this._onContextLoss = this._register(new D);
    this.onContextLoss = this._onContextLoss.event;
    this._canvas = this._coreBrowserService.mainDocument.createElement("canvas");
    let d = { antialias: false, depth: false, preserveDrawingBuffer: c };
    if (this._gl = this._canvas.getContext("webgl2", d), !this._gl)
      throw new Error("WebGL2 not supported " + this._gl);
    this._register(this._themeService.onChangeColors(() => this._handleColorChange())), this._cellColorResolver = new At(this._terminal, this._optionsService, this._model.selection, this._decorationService, this._coreBrowserService, this._themeService), this._core = this._terminal._core, this._renderLayers = [new Xt(this._core.screenElement, 2, this._terminal, this._core.linkifier, this._coreBrowserService, l, this._themeService)], this.dimensions = _n(), this._devicePixelRatio = this._coreBrowserService.dpr, this._updateDimensions(), this._updateCursorBlink(), this._register(l.onOptionChange(() => this._handleOptionsChanged())), this._deviceMaxTextureSize = this._gl.getParameter(this._gl.MAX_TEXTURE_SIZE), this._register(li(this._canvas, "webglcontextlost", (h) => {
      console.log("webglcontextlost event received"), h.preventDefault(), this._contextRestorationTimeout = setTimeout(() => {
        this._contextRestorationTimeout = undefined, console.warn("webgl context not restored; firing onContextLoss"), this._onContextLoss.fire(h);
      }, 3000);
    })), this._register(li(this._canvas, "webglcontextrestored", (h) => {
      console.warn("webglcontextrestored event received"), clearTimeout(this._contextRestorationTimeout), this._contextRestorationTimeout = undefined, Ai(this._terminal), this._initializeWebGLState(), this._requestRedrawViewport();
    })), this._observerDisposable.value = Si(this._canvas, this._coreBrowserService.window, (h, f) => this._setCanvasDevicePixelDimensions(h, f)), this._register(this._coreBrowserService.onWindowChange((h) => {
      this._observerDisposable.value = Si(this._canvas, h, (f, I) => this._setCanvasDevicePixelDimensions(f, I));
    })), this._core.screenElement.appendChild(this._canvas), [this._rectangleRenderer.value, this._glyphRenderer.value] = this._initializeWebGLState(), this._isAttached = this._core.screenElement.isConnected, this._register(O(() => {
      for (let h of this._renderLayers)
        h.dispose();
      this._canvas.parentElement?.removeChild(this._canvas), Ai(this._terminal);
    }));
  }
  get textureAtlas() {
    return this._charAtlas?.pages[0].canvas;
  }
  _handleColorChange() {
    this._refreshCharAtlas(), this._clearModel(true);
  }
  handleDevicePixelRatioChange() {
    this._devicePixelRatio !== this._coreBrowserService.dpr && (this._devicePixelRatio = this._coreBrowserService.dpr, this.handleResize(this._terminal.cols, this._terminal.rows));
  }
  handleResize(t, n) {
    this._updateDimensions(), this._model.resize(this._terminal.cols, this._terminal.rows);
    for (let s of this._renderLayers)
      s.resize(this._terminal, this.dimensions);
    this._canvas.width = this.dimensions.device.canvas.width, this._canvas.height = this.dimensions.device.canvas.height, this._canvas.style.width = `${this.dimensions.css.canvas.width}px`, this._canvas.style.height = `${this.dimensions.css.canvas.height}px`, this._core.screenElement.style.width = `${this.dimensions.css.canvas.width}px`, this._core.screenElement.style.height = `${this.dimensions.css.canvas.height}px`, this._rectangleRenderer.value?.setDimensions(this.dimensions), this._rectangleRenderer.value?.handleResize(), this._glyphRenderer.value?.setDimensions(this.dimensions), this._glyphRenderer.value?.handleResize(), this._refreshCharAtlas(), this._clearModel(false);
  }
  handleCharSizeChanged() {
    this.handleResize(this._terminal.cols, this._terminal.rows);
  }
  handleBlur() {
    for (let t of this._renderLayers)
      t.handleBlur(this._terminal);
    this._cursorBlinkStateManager.value?.pause(), this._requestRedrawViewport();
  }
  handleFocus() {
    for (let t of this._renderLayers)
      t.handleFocus(this._terminal);
    this._cursorBlinkStateManager.value?.resume(), this._requestRedrawViewport();
  }
  handleSelectionChanged(t, n, s) {
    for (let o of this._renderLayers)
      o.handleSelectionChanged(this._terminal, t, n, s);
    this._model.selection.update(this._core, t, n, s), this._requestRedrawViewport();
  }
  handleCursorMove() {
    for (let t of this._renderLayers)
      t.handleCursorMove(this._terminal);
    this._cursorBlinkStateManager.value?.restartBlinkAnimation();
  }
  _handleOptionsChanged() {
    this._updateDimensions(), this._refreshCharAtlas(), this._updateCursorBlink();
  }
  _initializeWebGLState() {
    return this._rectangleRenderer.value = new qt(this._terminal, this._gl, this.dimensions, this._themeService), this._glyphRenderer.value = new Kt(this._terminal, this._gl, this.dimensions, this._optionsService), this.handleCharSizeChanged(), [this._rectangleRenderer.value, this._glyphRenderer.value];
  }
  _refreshCharAtlas() {
    if (this.dimensions.device.char.width <= 0 && this.dimensions.device.char.height <= 0) {
      this._isAttached = false;
      return;
    }
    let t = Nt(this._terminal, this._optionsService.rawOptions, this._themeService.colors, this.dimensions.device.cell.width, this.dimensions.device.cell.height, this.dimensions.device.char.width, this.dimensions.device.char.height, this._coreBrowserService.dpr, this._deviceMaxTextureSize);
    this._charAtlas !== t && (this._onChangeTextureAtlas.fire(t.pages[0].canvas), this._charAtlasDisposable.value = It(ee.forward(t.onAddTextureAtlasCanvas, this._onAddTextureAtlasCanvas), ee.forward(t.onRemoveTextureAtlasCanvas, this._onRemoveTextureAtlasCanvas))), this._charAtlas = t, this._charAtlas.warmUp(), this._glyphRenderer.value?.setAtlas(this._charAtlas);
  }
  _clearModel(t) {
    this._model.clear(), t && this._glyphRenderer.value?.clear();
  }
  clearTextureAtlas() {
    this._charAtlas?.clearTexture(), this._clearModel(true), this._requestRedrawViewport();
  }
  clear() {
    this._clearModel(true);
    for (let t of this._renderLayers)
      t.reset(this._terminal);
    this._cursorBlinkStateManager.value?.restartBlinkAnimation(), this._updateCursorBlink();
  }
  renderRows(t, n) {
    if (!this._isAttached)
      if (this._core.screenElement?.isConnected && this._charSizeService.width && this._charSizeService.height)
        this._updateDimensions(), this._refreshCharAtlas(), this._isAttached = true;
      else
        return;
    for (let s of this._renderLayers)
      s.handleGridChanged(this._terminal, t, n);
    !this._glyphRenderer.value || !this._rectangleRenderer.value || (this._glyphRenderer.value.beginFrame() ? (this._clearModel(true), this._updateModel(0, this._terminal.rows - 1)) : this._updateModel(t, n), this._rectangleRenderer.value.renderBackgrounds(), this._glyphRenderer.value.render(this._model), (!this._cursorBlinkStateManager.value || this._cursorBlinkStateManager.value.isCursorVisible) && this._rectangleRenderer.value.renderCursor());
  }
  _updateCursorBlink() {
    this._coreService.decPrivateModes.cursorBlink ?? this._terminal.options.cursorBlink ? this._cursorBlinkStateManager.value = new Ht(() => {
      this._requestRedrawCursor();
    }, this._coreBrowserService) : this._cursorBlinkStateManager.clear(), this._requestRedrawCursor();
  }
  _updateModel(t, n) {
    let s = this._core, o = this._workCell, r, a, l, u, c, d, h = 0, f = true, I, L, M, q, S, W, E, y, w;
    t = mr(t, s.rows - 1, 0), n = mr(n, s.rows - 1, 0);
    let G = this._coreService.decPrivateModes.cursorStyle ?? s.options.cursorStyle ?? "block", ue = this._terminal.buffer.active.baseY + this._terminal.buffer.active.cursorY, Se = ue - s.buffer.ydisp, ce = Math.min(this._terminal.buffer.active.cursorX, s.cols - 1), we = -1, A = this._coreService.isCursorInitialized && !this._coreService.isCursorHidden && (!this._cursorBlinkStateManager.value || this._cursorBlinkStateManager.value.isCursorVisible);
    this._model.cursor = undefined;
    let se = false;
    for (a = t;a <= n; a++)
      for (l = a + s.buffer.ydisp, u = s.buffer.lines.get(l), this._model.lineLengths[a] = 0, M = ue === l, h = 0, c = this._characterJoinerService.getJoinedCharacters(l), y = 0;y < s.cols; y++) {
        if (r = this._cellColorResolver.result.bg, u.loadCell(y, o), y === 0 && (r = this._cellColorResolver.result.bg), d = false, f = y >= h, I = y, c.length > 0 && y === c[0][0] && f) {
          L = c.shift();
          let v = this._model.selection.isCellSelected(this._terminal, L[0], l);
          for (E = L[0] + 1;E < L[1]; E++)
            f &&= v === this._model.selection.isCellSelected(this._terminal, E, l);
          f &&= !M || ce < L[0] || ce >= L[1], f ? (d = true, o = new Ci(o, u.translateToString(true, L[0], L[1]), L[1] - L[0]), I = L[1] - 1) : h = L[1];
        }
        if (q = o.getChars(), S = o.getCode(), E = (a * s.cols + y) * Ce, this._cellColorResolver.resolve(o, y, l, this.dimensions.device.cell.width), A && l === ue && (y === ce && (this._model.cursor = { x: ce, y: Se, width: o.getWidth(), style: this._coreBrowserService.isFocused ? G : s.options.cursorInactiveStyle, cursorWidth: s.options.cursorWidth, dpr: this._devicePixelRatio }, we = ce + o.getWidth() - 1), y >= ce && y <= we && (this._coreBrowserService.isFocused && G === "block" || this._coreBrowserService.isFocused === false && s.options.cursorInactiveStyle === "block") && (this._cellColorResolver.result.fg = 50331648 | this._themeService.colors.cursorAccent.rgba >> 8 & 16777215, this._cellColorResolver.result.bg = 50331648 | this._themeService.colors.cursor.rgba >> 8 & 16777215)), S !== 0 && (this._model.lineLengths[a] = y + 1), !(this._model.cells[E] === S && this._model.cells[E + ze] === this._cellColorResolver.result.bg && this._model.cells[E + qe] === this._cellColorResolver.result.fg && this._model.cells[E + Ct] === this._cellColorResolver.result.ext) && (se = true, q.length > 1 && (S |= Un), this._model.cells[E] = S, this._model.cells[E + ze] = this._cellColorResolver.result.bg, this._model.cells[E + qe] = this._cellColorResolver.result.fg, this._model.cells[E + Ct] = this._cellColorResolver.result.ext, W = o.getWidth(), this._glyphRenderer.value.updateCell(y, a, S, this._cellColorResolver.result.bg, this._cellColorResolver.result.fg, this._cellColorResolver.result.ext, q, W, r), d)) {
          for (o = this._workCell, y++;y <= I; y++)
            w = (a * s.cols + y) * Ce, this._glyphRenderer.value.updateCell(y, a, 0, 0, 0, 0, pn, 0, 0), this._model.cells[w] = 0, this._model.cells[w + ze] = this._cellColorResolver.result.bg, this._model.cells[w + qe] = this._cellColorResolver.result.fg, this._model.cells[w + Ct] = this._cellColorResolver.result.ext;
          y--;
        }
      }
    se && this._rectangleRenderer.value.updateBackgrounds(this._model), this._rectangleRenderer.value.updateCursor(this._model);
  }
  _updateDimensions() {
    !this._charSizeService.width || !this._charSizeService.height || (this.dimensions.device.char.width = Math.floor(this._charSizeService.width * this._devicePixelRatio), this.dimensions.device.char.height = Math.ceil(this._charSizeService.height * this._devicePixelRatio), this.dimensions.device.cell.height = Math.floor(this.dimensions.device.char.height * this._optionsService.rawOptions.lineHeight), this.dimensions.device.char.top = this._optionsService.rawOptions.lineHeight === 1 ? 0 : Math.round((this.dimensions.device.cell.height - this.dimensions.device.char.height) / 2), this.dimensions.device.cell.width = this.dimensions.device.char.width + Math.round(this._optionsService.rawOptions.letterSpacing), this.dimensions.device.char.left = Math.floor(this._optionsService.rawOptions.letterSpacing / 2), this.dimensions.device.canvas.height = this._terminal.rows * this.dimensions.device.cell.height, this.dimensions.device.canvas.width = this._terminal.cols * this.dimensions.device.cell.width, this.dimensions.css.canvas.height = Math.round(this.dimensions.device.canvas.height / this._devicePixelRatio), this.dimensions.css.canvas.width = Math.round(this.dimensions.device.canvas.width / this._devicePixelRatio), this.dimensions.css.cell.height = this.dimensions.device.cell.height / this._devicePixelRatio, this.dimensions.css.cell.width = this.dimensions.device.cell.width / this._devicePixelRatio);
  }
  _setCanvasDevicePixelDimensions(t, n) {
    this._canvas.width === t && this._canvas.height === n || (this._canvas.width = t, this._canvas.height = n, this._requestRedrawViewport());
  }
  _requestRedrawViewport() {
    this._onRequestRedraw.fire({ start: 0, end: this._terminal.rows - 1 });
  }
  _requestRedrawCursor() {
    let t = this._terminal.buffer.active.cursorY;
    this._onRequestRedraw.fire({ start: t, end: t });
  }
};
var Ci = class extends he {
  constructor(t, n, s) {
    super();
    this.content = 0;
    this.combinedData = "";
    this.fg = t.fg, this.bg = t.bg, this.combinedData = n, this._width = s;
  }
  isCombined() {
    return 2097152;
  }
  getWidth() {
    return this._width;
  }
  getChars() {
    return this.combinedData;
  }
  getCode() {
    return 2097151;
  }
  setFromCharData(t) {
    throw new Error("not implemented");
  }
  getAsCharData() {
    return [this.fg, this.getChars(), this.getWidth(), this.getCode()];
  }
};
function mr(i, e, t = 0) {
  return Math.max(Math.min(i, e), t);
}
var _r = "di$target";
var br = "di$dependencies";
var zi = new Map;
function pe(i) {
  if (zi.has(i))
    return zi.get(i);
  let e = function(t, n, s) {
    if (arguments.length !== 3)
      throw new Error("@IServiceName-decorator can only be used to decorate a parameter");
    Us(e, t, s);
  };
  return e._id = i, zi.set(i, e), e;
}
function Us(i, e, t) {
  e[_r] === e ? e[br].push({ id: i, index: t }) : (e[br] = [{ id: i, index: t }], e[_r] = e);
}
var Vu = pe("BufferService");
var Cu = pe("CoreMouseService");
var zu = pe("CoreService");
var qu = pe("CharsetService");
var ju = pe("InstantiationService");
var Xu = pe("LogService");
var vr = pe("OptionsService");
var Yu = pe("OscLinkService");
var Qu = pe("UnicodeService");
var Zu = pe("DecorationService");
var Hs = { trace: 0, debug: 1, info: 2, warn: 3, error: 4, off: 5 };
var Ws = "xterm.js: ";
var ci = class extends B {
  constructor(t) {
    super();
    this._optionsService = t;
    this._logLevel = 5;
    this._updateLogLevel(), this._register(this._optionsService.onSpecificOptionChange("logLevel", () => this._updateLogLevel())), Tr = this;
  }
  get logLevel() {
    return this._logLevel;
  }
  _updateLogLevel() {
    this._logLevel = Hs[this._optionsService.rawOptions.logLevel];
  }
  _evalLazyOptionalParams(t) {
    for (let n = 0;n < t.length; n++)
      typeof t[n] == "function" && (t[n] = t[n]());
  }
  _log(t, n, s) {
    this._evalLazyOptionalParams(s), t.call(console, (this._optionsService.options.logger ? "" : Ws) + n, ...s);
  }
  trace(t, ...n) {
    this._logLevel <= 0 && this._log(this._optionsService.options.logger?.trace.bind(this._optionsService.options.logger) ?? console.log, t, n);
  }
  debug(t, ...n) {
    this._logLevel <= 1 && this._log(this._optionsService.options.logger?.debug.bind(this._optionsService.options.logger) ?? console.log, t, n);
  }
  info(t, ...n) {
    this._logLevel <= 2 && this._log(this._optionsService.options.logger?.info.bind(this._optionsService.options.logger) ?? console.info, t, n);
  }
  warn(t, ...n) {
    this._logLevel <= 3 && this._log(this._optionsService.options.logger?.warn.bind(this._optionsService.options.logger) ?? console.warn, t, n);
  }
  error(t, ...n) {
    this._logLevel <= 4 && this._log(this._optionsService.options.logger?.error.bind(this._optionsService.options.logger) ?? console.error, t, n);
  }
};
ci = Yi([Qi(0, vr)], ci);
var Tr;
function gr(i) {
  Tr = i;
}
var xr = class extends B {
  constructor(t) {
    if (vi && hn() < 16) {
      let n = { antialias: false, depth: false, preserveDrawingBuffer: true };
      if (!document.createElement("canvas").getContext("webgl2", n))
        throw new Error("Webgl2 is only supported on Safari 16 and above");
    }
    super();
    this._preserveDrawingBuffer = t;
    this._onChangeTextureAtlas = this._register(new D);
    this.onChangeTextureAtlas = this._onChangeTextureAtlas.event;
    this._onAddTextureAtlasCanvas = this._register(new D);
    this.onAddTextureAtlasCanvas = this._onAddTextureAtlasCanvas.event;
    this._onRemoveTextureAtlasCanvas = this._register(new D);
    this.onRemoveTextureAtlasCanvas = this._onRemoveTextureAtlasCanvas.event;
    this._onContextLoss = this._register(new D);
    this.onContextLoss = this._onContextLoss.event;
  }
  activate(t) {
    let n = t._core;
    if (!t.element) {
      this._register(n.onWillOpen(() => this.activate(t)));
      return;
    }
    this._terminal = t;
    let { coreService: s, optionsService: o } = n, r = n, { _renderService: a, _characterJoinerService: l, _charSizeService: u, _coreBrowserService: c, _decorationService: d, _logService: h, _themeService: f } = r;
    gr(h), this._renderer = this._register(new ui(t, l, u, c, s, d, o, f, this._preserveDrawingBuffer)), this._register(ee.forward(this._renderer.onContextLoss, this._onContextLoss)), this._register(ee.forward(this._renderer.onChangeTextureAtlas, this._onChangeTextureAtlas)), this._register(ee.forward(this._renderer.onAddTextureAtlasCanvas, this._onAddTextureAtlasCanvas)), this._register(ee.forward(this._renderer.onRemoveTextureAtlasCanvas, this._onRemoveTextureAtlasCanvas)), a.setRenderer(this._renderer), this._register(O(() => {
      if (this._terminal._core._store._isDisposed)
        return;
      let I = this._terminal._core._renderService;
      I.setRenderer(this._terminal._core._createRenderer()), I.handleResize(t.cols, t.rows);
    }));
  }
  get textureAtlas() {
    return this._renderer?.textureAtlas;
  }
  clearTextureAtlas() {
    this._renderer?.clearTextureAtlas();
  }
};

// node_modules/@xterm/xterm/lib/xterm.mjs
var zs = Object.defineProperty;
var Rl = Object.getOwnPropertyDescriptor;
var Ll = (s, t) => {
  for (var e in t)
    zs(s, e, { get: t[e], enumerable: true });
};
var M = (s, t, e, i) => {
  for (var r = i > 1 ? undefined : i ? Rl(t, e) : t, n = s.length - 1, o;n >= 0; n--)
    (o = s[n]) && (r = (i ? o(t, e, r) : o(r)) || r);
  return i && r && zs(t, e, r), r;
};
var S = (s, t) => (e, i) => t(e, i, s);
var Gs = "Terminal input";
var mi2 = { get: () => Gs, set: (s) => Gs = s };
var $s = "Too much output to announce, navigate to rows manually to read";
var _i2 = { get: () => $s, set: (s) => $s = s };
function Al(s) {
  return s.replace(/\r?\n/g, "\r");
}
function kl(s, t) {
  return t ? "\x1B[200~" + s + "\x1B[201~" : s;
}
function Vs(s, t) {
  s.clipboardData && s.clipboardData.setData("text/plain", t.selectionText), s.preventDefault();
}
function qs(s, t, e, i) {
  if (s.stopPropagation(), s.clipboardData) {
    let r = s.clipboardData.getData("text/plain");
    Cn2(r, t, e, i);
  }
}
function Cn2(s, t, e, i) {
  s = Al(s), s = kl(s, e.decPrivateModes.bracketedPasteMode && i.rawOptions.ignoreBracketedPasteMode !== true), e.triggerDataEvent(s, true), t.value = "";
}
function Mn2(s, t, e) {
  let i = e.getBoundingClientRect(), r = s.clientX - i.left - 10, n = s.clientY - i.top - 10;
  t.style.width = "20px", t.style.height = "20px", t.style.left = `${r}px`, t.style.top = `${n}px`, t.style.zIndex = "1000", t.focus();
}
function Pn2(s, t, e, i, r) {
  Mn2(s, t, e), r && i.rightClickSelect(s), t.value = i.selectionText, t.select();
}
function Ce2(s) {
  return s > 65535 ? (s -= 65536, String.fromCharCode((s >> 10) + 55296) + String.fromCharCode(s % 1024 + 56320)) : String.fromCharCode(s);
}
function It2(s, t = 0, e = s.length) {
  let i = "";
  for (let r = t;r < e; ++r) {
    let n = s[r];
    n > 65535 ? (n -= 65536, i += String.fromCharCode((n >> 10) + 55296) + String.fromCharCode(n % 1024 + 56320)) : i += String.fromCharCode(n);
  }
  return i;
}
var er2 = class {
  constructor() {
    this._interim = 0;
  }
  clear() {
    this._interim = 0;
  }
  decode(t, e) {
    let i = t.length;
    if (!i)
      return 0;
    let r = 0, n = 0;
    if (this._interim) {
      let o = t.charCodeAt(n++);
      56320 <= o && o <= 57343 ? e[r++] = (this._interim - 55296) * 1024 + o - 56320 + 65536 : (e[r++] = this._interim, e[r++] = o), this._interim = 0;
    }
    for (let o = n;o < i; ++o) {
      let l = t.charCodeAt(o);
      if (55296 <= l && l <= 56319) {
        if (++o >= i)
          return this._interim = l, r;
        let a = t.charCodeAt(o);
        56320 <= a && a <= 57343 ? e[r++] = (l - 55296) * 1024 + a - 56320 + 65536 : (e[r++] = l, e[r++] = a);
        continue;
      }
      l !== 65279 && (e[r++] = l);
    }
    return r;
  }
};
var tr2 = class {
  constructor() {
    this.interim = new Uint8Array(3);
  }
  clear() {
    this.interim.fill(0);
  }
  decode(t, e) {
    let i = t.length;
    if (!i)
      return 0;
    let r = 0, n, o, l, a, u = 0, h = 0;
    if (this.interim[0]) {
      let _ = false, p = this.interim[0];
      p &= (p & 224) === 192 ? 31 : (p & 240) === 224 ? 15 : 7;
      let m = 0, f;
      for (;(f = this.interim[++m] & 63) && m < 4; )
        p <<= 6, p |= f;
      let A = (this.interim[0] & 224) === 192 ? 2 : (this.interim[0] & 240) === 224 ? 3 : 4, R = A - m;
      for (;h < R; ) {
        if (h >= i)
          return 0;
        if (f = t[h++], (f & 192) !== 128) {
          h--, _ = true;
          break;
        } else
          this.interim[m++] = f, p <<= 6, p |= f & 63;
      }
      _ || (A === 2 ? p < 128 ? h-- : e[r++] = p : A === 3 ? p < 2048 || p >= 55296 && p <= 57343 || p === 65279 || (e[r++] = p) : p < 65536 || p > 1114111 || (e[r++] = p)), this.interim.fill(0);
    }
    let c = i - 4, d = h;
    for (;d < i; ) {
      for (;d < c && !((n = t[d]) & 128) && !((o = t[d + 1]) & 128) && !((l = t[d + 2]) & 128) && !((a = t[d + 3]) & 128); )
        e[r++] = n, e[r++] = o, e[r++] = l, e[r++] = a, d += 4;
      if (n = t[d++], n < 128)
        e[r++] = n;
      else if ((n & 224) === 192) {
        if (d >= i)
          return this.interim[0] = n, r;
        if (o = t[d++], (o & 192) !== 128) {
          d--;
          continue;
        }
        if (u = (n & 31) << 6 | o & 63, u < 128) {
          d--;
          continue;
        }
        e[r++] = u;
      } else if ((n & 240) === 224) {
        if (d >= i)
          return this.interim[0] = n, r;
        if (o = t[d++], (o & 192) !== 128) {
          d--;
          continue;
        }
        if (d >= i)
          return this.interim[0] = n, this.interim[1] = o, r;
        if (l = t[d++], (l & 192) !== 128) {
          d--;
          continue;
        }
        if (u = (n & 15) << 12 | (o & 63) << 6 | l & 63, u < 2048 || u >= 55296 && u <= 57343 || u === 65279)
          continue;
        e[r++] = u;
      } else if ((n & 248) === 240) {
        if (d >= i)
          return this.interim[0] = n, r;
        if (o = t[d++], (o & 192) !== 128) {
          d--;
          continue;
        }
        if (d >= i)
          return this.interim[0] = n, this.interim[1] = o, r;
        if (l = t[d++], (l & 192) !== 128) {
          d--;
          continue;
        }
        if (d >= i)
          return this.interim[0] = n, this.interim[1] = o, this.interim[2] = l, r;
        if (a = t[d++], (a & 192) !== 128) {
          d--;
          continue;
        }
        if (u = (n & 7) << 18 | (o & 63) << 12 | (l & 63) << 6 | a & 63, u < 65536 || u > 1114111)
          continue;
        e[r++] = u;
      }
    }
    return r;
  }
};
var ir = "";
var we = " ";
var De2 = class s {
  constructor() {
    this.fg = 0;
    this.bg = 0;
    this.extended = new rt2;
  }
  static toColorRGB(t) {
    return [t >>> 16 & 255, t >>> 8 & 255, t & 255];
  }
  static fromColorRGB(t) {
    return (t[0] & 255) << 16 | (t[1] & 255) << 8 | t[2] & 255;
  }
  clone() {
    let t = new s;
    return t.fg = this.fg, t.bg = this.bg, t.extended = this.extended.clone(), t;
  }
  isInverse() {
    return this.fg & 67108864;
  }
  isBold() {
    return this.fg & 134217728;
  }
  isUnderline() {
    return this.hasExtendedAttrs() && this.extended.underlineStyle !== 0 ? 1 : this.fg & 268435456;
  }
  isBlink() {
    return this.fg & 536870912;
  }
  isInvisible() {
    return this.fg & 1073741824;
  }
  isItalic() {
    return this.bg & 67108864;
  }
  isDim() {
    return this.bg & 134217728;
  }
  isStrikethrough() {
    return this.fg & 2147483648;
  }
  isProtected() {
    return this.bg & 536870912;
  }
  isOverline() {
    return this.bg & 1073741824;
  }
  getFgColorMode() {
    return this.fg & 50331648;
  }
  getBgColorMode() {
    return this.bg & 50331648;
  }
  isFgRGB() {
    return (this.fg & 50331648) === 50331648;
  }
  isBgRGB() {
    return (this.bg & 50331648) === 50331648;
  }
  isFgPalette() {
    return (this.fg & 50331648) === 16777216 || (this.fg & 50331648) === 33554432;
  }
  isBgPalette() {
    return (this.bg & 50331648) === 16777216 || (this.bg & 50331648) === 33554432;
  }
  isFgDefault() {
    return (this.fg & 50331648) === 0;
  }
  isBgDefault() {
    return (this.bg & 50331648) === 0;
  }
  isAttributeDefault() {
    return this.fg === 0 && this.bg === 0;
  }
  getFgColor() {
    switch (this.fg & 50331648) {
      case 16777216:
      case 33554432:
        return this.fg & 255;
      case 50331648:
        return this.fg & 16777215;
      default:
        return -1;
    }
  }
  getBgColor() {
    switch (this.bg & 50331648) {
      case 16777216:
      case 33554432:
        return this.bg & 255;
      case 50331648:
        return this.bg & 16777215;
      default:
        return -1;
    }
  }
  hasExtendedAttrs() {
    return this.bg & 268435456;
  }
  updateExtended() {
    this.extended.isEmpty() ? this.bg &= -268435457 : this.bg |= 268435456;
  }
  getUnderlineColor() {
    if (this.bg & 268435456 && ~this.extended.underlineColor)
      switch (this.extended.underlineColor & 50331648) {
        case 16777216:
        case 33554432:
          return this.extended.underlineColor & 255;
        case 50331648:
          return this.extended.underlineColor & 16777215;
        default:
          return this.getFgColor();
      }
    return this.getFgColor();
  }
  getUnderlineColorMode() {
    return this.bg & 268435456 && ~this.extended.underlineColor ? this.extended.underlineColor & 50331648 : this.getFgColorMode();
  }
  isUnderlineColorRGB() {
    return this.bg & 268435456 && ~this.extended.underlineColor ? (this.extended.underlineColor & 50331648) === 50331648 : this.isFgRGB();
  }
  isUnderlineColorPalette() {
    return this.bg & 268435456 && ~this.extended.underlineColor ? (this.extended.underlineColor & 50331648) === 16777216 || (this.extended.underlineColor & 50331648) === 33554432 : this.isFgPalette();
  }
  isUnderlineColorDefault() {
    return this.bg & 268435456 && ~this.extended.underlineColor ? (this.extended.underlineColor & 50331648) === 0 : this.isFgDefault();
  }
  getUnderlineStyle() {
    return this.fg & 268435456 ? this.bg & 268435456 ? this.extended.underlineStyle : 1 : 0;
  }
  getUnderlineVariantOffset() {
    return this.extended.underlineVariantOffset;
  }
};
var rt2 = class s {
  constructor(t = 0, e = 0) {
    this._ext = 0;
    this._urlId = 0;
    this._ext = t, this._urlId = e;
  }
  get ext() {
    return this._urlId ? this._ext & -469762049 | this.underlineStyle << 26 : this._ext;
  }
  set ext(t) {
    this._ext = t;
  }
  get underlineStyle() {
    return this._urlId ? 5 : (this._ext & 469762048) >> 26;
  }
  set underlineStyle(t) {
    this._ext &= -469762049, this._ext |= t << 26 & 469762048;
  }
  get underlineColor() {
    return this._ext & 67108863;
  }
  set underlineColor(t) {
    this._ext &= -67108864, this._ext |= t & 67108863;
  }
  get urlId() {
    return this._urlId;
  }
  set urlId(t) {
    this._urlId = t;
  }
  get underlineVariantOffset() {
    let t = (this._ext & 3758096384) >> 29;
    return t < 0 ? t ^ 4294967288 : t;
  }
  set underlineVariantOffset(t) {
    this._ext &= 536870911, this._ext |= t << 29 & 3758096384;
  }
  clone() {
    return new s(this._ext, this._urlId);
  }
  isEmpty() {
    return this.underlineStyle === 0 && this._urlId === 0;
  }
};
var q = class s extends De2 {
  constructor() {
    super(...arguments);
    this.content = 0;
    this.fg = 0;
    this.bg = 0;
    this.extended = new rt2;
    this.combinedData = "";
  }
  static fromCharData(e) {
    let i = new s;
    return i.setFromCharData(e), i;
  }
  isCombined() {
    return this.content & 2097152;
  }
  getWidth() {
    return this.content >> 22;
  }
  getChars() {
    return this.content & 2097152 ? this.combinedData : this.content & 2097151 ? Ce2(this.content & 2097151) : "";
  }
  getCode() {
    return this.isCombined() ? this.combinedData.charCodeAt(this.combinedData.length - 1) : this.content & 2097151;
  }
  setFromCharData(e) {
    this.fg = e[0], this.bg = 0;
    let i = false;
    if (e[1].length > 2)
      i = true;
    else if (e[1].length === 2) {
      let r = e[1].charCodeAt(0);
      if (55296 <= r && r <= 56319) {
        let n = e[1].charCodeAt(1);
        56320 <= n && n <= 57343 ? this.content = (r - 55296) * 1024 + n - 56320 + 65536 | e[2] << 22 : i = true;
      } else
        i = true;
    } else
      this.content = e[1].charCodeAt(0) | e[2] << 22;
    i && (this.combinedData = e[1], this.content = 2097152 | e[2] << 22);
  }
  getAsCharData() {
    return [this.fg, this.getChars(), this.getWidth(), this.getCode()];
  }
};
var js = "di$target";
var Hn2 = "di$dependencies";
var Fn2 = new Map;
function Xs(s) {
  return s[Hn2] || [];
}
function ie(s) {
  if (Fn2.has(s))
    return Fn2.get(s);
  let t = function(e, i, r) {
    if (arguments.length !== 3)
      throw new Error("@IServiceName-decorator can only be used to decorate a parameter");
    Pl(t, e, r);
  };
  return t._id = s, Fn2.set(s, t), t;
}
function Pl(s, t, e) {
  t[js] === t ? t[Hn2].push({ id: s, index: e }) : (t[Hn2] = [{ id: s, index: e }], t[js] = t);
}
var F2 = ie("BufferService");
var rr = ie("CoreMouseService");
var ge2 = ie("CoreService");
var Zs = ie("CharsetService");
var xt2 = ie("InstantiationService");
var nr2 = ie("LogService");
var H2 = ie("OptionsService");
var sr2 = ie("OscLinkService");
var Js = ie("UnicodeService");
var Be2 = ie("DecorationService");
var wt2 = class {
  constructor(t, e, i) {
    this._bufferService = t;
    this._optionsService = e;
    this._oscLinkService = i;
  }
  provideLinks(t, e) {
    let i = this._bufferService.buffer.lines.get(t - 1);
    if (!i) {
      e(undefined);
      return;
    }
    let r = [], n = this._optionsService.rawOptions.linkHandler, o = new q, l = i.getTrimmedLength(), a = -1, u = -1, h = false;
    for (let c = 0;c < l; c++)
      if (!(u === -1 && !i.hasContent(c))) {
        if (i.loadCell(c, o), o.hasExtendedAttrs() && o.extended.urlId)
          if (u === -1) {
            u = c, a = o.extended.urlId;
            continue;
          } else
            h = o.extended.urlId !== a;
        else
          u !== -1 && (h = true);
        if (h || u !== -1 && c === l - 1) {
          let d = this._oscLinkService.getLinkData(a)?.uri;
          if (d) {
            let _ = { start: { x: u + 1, y: t }, end: { x: c + (!h && c === l - 1 ? 1 : 0), y: t } }, p = false;
            if (!n?.allowNonHttpProtocols)
              try {
                let m = new URL(d);
                ["http:", "https:"].includes(m.protocol) || (p = true);
              } catch {
                p = true;
              }
            p || r.push({ text: d, range: _, activate: (m, f) => n ? n.activate(m, f, _) : Ol(m, f), hover: (m, f) => n?.hover?.(m, f, _), leave: (m, f) => n?.leave?.(m, f, _) });
          }
          h = false, o.hasExtendedAttrs() && o.extended.urlId ? (u = c, a = o.extended.urlId) : (u = -1, a = -1);
        }
      }
    e(r);
  }
};
wt2 = M([S(0, F2), S(1, H2), S(2, sr2)], wt2);
function Ol(s, t) {
  if (confirm(`Do you want to navigate to ${t}?

WARNING: This link could potentially be dangerous`)) {
    let i = window.open();
    if (i) {
      try {
        i.opener = null;
      } catch {}
      i.location.href = t;
    } else
      console.warn("Opening link blocked as opener could not be cleared");
  }
}
var nt2 = ie("CharSizeService");
var ae2 = ie("CoreBrowserService");
var Dt2 = ie("MouseService");
var ce = ie("RenderService");
var Qs = ie("SelectionService");
var or2 = ie("CharacterJoinerService");
var Re = ie("ThemeService");
var lr = ie("LinkProviderService");
var Wn2 = class {
  constructor() {
    this.listeners = [], this.unexpectedErrorHandler = function(t) {
      setTimeout(() => {
        throw t.stack ? ar2.isErrorNoTelemetry(t) ? new ar2(t.message + `

` + t.stack) : new Error(t.message + `

` + t.stack) : t;
      }, 0);
    };
  }
  addListener(t) {
    return this.listeners.push(t), () => {
      this._removeListener(t);
    };
  }
  emit(t) {
    this.listeners.forEach((e) => {
      e(t);
    });
  }
  _removeListener(t) {
    this.listeners.splice(this.listeners.indexOf(t), 1);
  }
  setUnexpectedErrorHandler(t) {
    this.unexpectedErrorHandler = t;
  }
  getUnexpectedErrorHandler() {
    return this.unexpectedErrorHandler;
  }
  onUnexpectedError(t) {
    this.unexpectedErrorHandler(t), this.emit(t);
  }
  onUnexpectedExternalError(t) {
    this.unexpectedErrorHandler(t);
  }
};
var Bl = new Wn2;
function Lt2(s) {
  Nl(s) || Bl.onUnexpectedError(s);
}
var Un2 = "Canceled";
function Nl(s) {
  return s instanceof bi2 ? true : s instanceof Error && s.name === Un2 && s.message === Un2;
}
var bi2 = class extends Error {
  constructor() {
    super(Un2), this.name = this.message;
  }
};
function eo(s) {
  return s ? new Error(`Illegal argument: ${s}`) : new Error("Illegal argument");
}
var ar2 = class s extends Error {
  constructor(t) {
    super(t), this.name = "CodeExpectedError";
  }
  static fromError(t) {
    if (t instanceof s)
      return t;
    let e = new s;
    return e.message = t.message, e.stack = t.stack, e;
  }
  static isErrorNoTelemetry(t) {
    return t.name === "CodeExpectedError";
  }
};
var Rt2 = class s extends Error {
  constructor(t) {
    super(t || "An unexpected bug occurred."), Object.setPrototypeOf(this, s.prototype);
  }
};
function Fl(s, t, e = 0, i = s.length) {
  let r = e, n = i;
  for (;r < n; ) {
    let o = Math.floor((r + n) / 2);
    t(s[o]) ? r = o + 1 : n = o;
  }
  return r - 1;
}
var cr2 = class cr {
  constructor(t) {
    this._array = t;
    this._findLastMonotonousLastIdx = 0;
  }
  findLastMonotonous(t) {
    if (cr.assertInvariants) {
      if (this._prevFindLastPredicate) {
        for (let i of this._array)
          if (this._prevFindLastPredicate(i) && !t(i))
            throw new Error("MonotonousArray: current predicate must be weaker than (or equal to) the previous predicate.");
      }
      this._prevFindLastPredicate = t;
    }
    let e = Fl(this._array, t, this._findLastMonotonousLastIdx);
    return this._findLastMonotonousLastIdx = e + 1, e === -1 ? undefined : this._array[e];
  }
};
cr2.assertInvariants = false;
function Se(s, t = 0) {
  return s[s.length - (1 + t)];
}
var ro;
((l) => {
  function s(a) {
    return a < 0;
  }
  l.isLessThan = s;
  function t(a) {
    return a <= 0;
  }
  l.isLessThanOrEqual = t;
  function e(a) {
    return a > 0;
  }
  l.isGreaterThan = e;
  function i(a) {
    return a === 0;
  }
  l.isNeitherLessOrGreaterThan = i, l.greaterThan = 1, l.lessThan = -1, l.neitherLessOrGreaterThan = 0;
})(ro ||= {});
function no(s, t) {
  return (e, i) => t(s(e), s(i));
}
var so = (s, t) => s - t;
var At2 = class At {
  constructor(t) {
    this.iterate = t;
  }
  forEach(t) {
    this.iterate((e) => (t(e), true));
  }
  toArray() {
    let t = [];
    return this.iterate((e) => (t.push(e), true)), t;
  }
  filter(t) {
    return new At((e) => this.iterate((i) => t(i) ? e(i) : true));
  }
  map(t) {
    return new At((e) => this.iterate((i) => e(t(i))));
  }
  some(t) {
    let e = false;
    return this.iterate((i) => (e = t(i), !e)), e;
  }
  findFirst(t) {
    let e;
    return this.iterate((i) => t(i) ? (e = i, false) : true), e;
  }
  findLast(t) {
    let e;
    return this.iterate((i) => (t(i) && (e = i), true)), e;
  }
  findLastMaxBy(t) {
    let e, i = true;
    return this.iterate((r) => ((i || ro.isGreaterThan(t(r, e))) && (i = false, e = r), true)), e;
  }
};
At2.empty = new At2((t) => {});
function co(s, t) {
  let e = Object.create(null);
  for (let i of s) {
    let r = t(i), n = e[r];
    n || (n = e[r] = []), n.push(i);
  }
  return e;
}
var lo2;
var ao2;
var oo2 = class {
  constructor(t, e) {
    this.toKey = e;
    this._map = new Map;
    this[lo2] = "SetWithKey";
    for (let i of t)
      this.add(i);
  }
  get size() {
    return this._map.size;
  }
  add(t) {
    let e = this.toKey(t);
    return this._map.set(e, t), this;
  }
  delete(t) {
    return this._map.delete(this.toKey(t));
  }
  has(t) {
    return this._map.has(this.toKey(t));
  }
  *entries() {
    for (let t of this._map.values())
      yield [t, t];
  }
  keys() {
    return this.values();
  }
  *values() {
    for (let t of this._map.values())
      yield t;
  }
  clear() {
    this._map.clear();
  }
  forEach(t, e) {
    this._map.forEach((i) => t.call(e, i, i, this));
  }
  [(ao2 = Symbol.iterator, lo2 = Symbol.toStringTag, ao2)]() {
    return this.values();
  }
};
var ur2 = class {
  constructor() {
    this.map = new Map;
  }
  add(t, e) {
    let i = this.map.get(t);
    i || (i = new Set, this.map.set(t, i)), i.add(e);
  }
  delete(t, e) {
    let i = this.map.get(t);
    i && (i.delete(e), i.size === 0 && this.map.delete(t));
  }
  forEach(t, e) {
    let i = this.map.get(t);
    i && i.forEach(e);
  }
  get(t) {
    let e = this.map.get(t);
    return e || new Set;
  }
};
function Kn2(s, t) {
  let e = this, i = false, r;
  return function() {
    if (i)
      return r;
    if (i = true, t)
      try {
        r = s.apply(e, arguments);
      } finally {
        t();
      }
    else
      r = s.apply(e, arguments);
    return r;
  };
}
var zn2;
((O) => {
  function s(I) {
    return I && typeof I == "object" && typeof I[Symbol.iterator] == "function";
  }
  O.is = s;
  let t = Object.freeze([]);
  function e() {
    return t;
  }
  O.empty = e;
  function* i(I) {
    yield I;
  }
  O.single = i;
  function r(I) {
    return s(I) ? I : i(I);
  }
  O.wrap = r;
  function n(I) {
    return I || t;
  }
  O.from = n;
  function* o(I) {
    for (let k = I.length - 1;k >= 0; k--)
      yield I[k];
  }
  O.reverse = o;
  function l(I) {
    return !I || I[Symbol.iterator]().next().done === true;
  }
  O.isEmpty = l;
  function a(I) {
    return I[Symbol.iterator]().next().value;
  }
  O.first = a;
  function u(I, k) {
    let P = 0;
    for (let oe of I)
      if (k(oe, P++))
        return true;
    return false;
  }
  O.some = u;
  function h(I, k) {
    for (let P of I)
      if (k(P))
        return P;
  }
  O.find = h;
  function* c(I, k) {
    for (let P of I)
      k(P) && (yield P);
  }
  O.filter = c;
  function* d(I, k) {
    let P = 0;
    for (let oe of I)
      yield k(oe, P++);
  }
  O.map = d;
  function* _(I, k) {
    let P = 0;
    for (let oe of I)
      yield* k(oe, P++);
  }
  O.flatMap = _;
  function* p(...I) {
    for (let k of I)
      yield* k;
  }
  O.concat = p;
  function m(I, k, P) {
    let oe = P;
    for (let Me of I)
      oe = k(oe, Me);
    return oe;
  }
  O.reduce = m;
  function* f(I, k, P = I.length) {
    for (k < 0 && (k += I.length), P < 0 ? P += I.length : P > I.length && (P = I.length);k < P; k++)
      yield I[k];
  }
  O.slice = f;
  function A(I, k = Number.POSITIVE_INFINITY) {
    let P = [];
    if (k === 0)
      return [P, I];
    let oe = I[Symbol.iterator]();
    for (let Me = 0;Me < k; Me++) {
      let Pe = oe.next();
      if (Pe.done)
        return [P, O.empty()];
      P.push(Pe.value);
    }
    return [P, { [Symbol.iterator]() {
      return oe;
    } }];
  }
  O.consume = A;
  async function R(I) {
    let k = [];
    for await (let P of I)
      k.push(P);
    return Promise.resolve(k);
  }
  O.asyncToArray = R;
})(zn2 ||= {});
var Wl = false;
var dt2 = null;
var hr2 = class hr {
  constructor() {
    this.livingDisposables = new Map;
  }
  getDisposableData(t) {
    let e = this.livingDisposables.get(t);
    return e || (e = { parent: null, source: null, isSingleton: false, value: t, idx: hr.idx++ }, this.livingDisposables.set(t, e)), e;
  }
  trackDisposable(t) {
    let e = this.getDisposableData(t);
    e.source || (e.source = new Error().stack);
  }
  setParent(t, e) {
    let i = this.getDisposableData(t);
    i.parent = e;
  }
  markAsDisposed(t) {
    this.livingDisposables.delete(t);
  }
  markAsSingleton(t) {
    this.getDisposableData(t).isSingleton = true;
  }
  getRootParent(t, e) {
    let i = e.get(t);
    if (i)
      return i;
    let r = t.parent ? this.getRootParent(this.getDisposableData(t.parent), e) : t;
    return e.set(t, r), r;
  }
  getTrackedDisposables() {
    let t = new Map;
    return [...this.livingDisposables.entries()].filter(([, i]) => i.source !== null && !this.getRootParent(i, t).isSingleton).flatMap(([i]) => i);
  }
  computeLeakingDisposables(t = 10, e) {
    let i;
    if (e)
      i = e;
    else {
      let a = new Map, u = [...this.livingDisposables.values()].filter((c) => c.source !== null && !this.getRootParent(c, a).isSingleton);
      if (u.length === 0)
        return;
      let h = new Set(u.map((c) => c.value));
      if (i = u.filter((c) => !(c.parent && h.has(c.parent))), i.length === 0)
        throw new Error("There are cyclic diposable chains!");
    }
    if (!i)
      return;
    function r(a) {
      function u(c, d) {
        for (;c.length > 0 && d.some((_) => typeof _ == "string" ? _ === c[0] : c[0].match(_)); )
          c.shift();
      }
      let h = a.source.split(`
`).map((c) => c.trim().replace("at ", "")).filter((c) => c !== "");
      return u(h, ["Error", /^trackDisposable \(.*\)$/, /^DisposableTracker.trackDisposable \(.*\)$/]), h.reverse();
    }
    let n = new ur2;
    for (let a of i) {
      let u = r(a);
      for (let h = 0;h <= u.length; h++)
        n.add(u.slice(0, h).join(`
`), a);
    }
    i.sort(no((a) => a.idx, so));
    let o = "", l = 0;
    for (let a of i.slice(0, t)) {
      l++;
      let u = r(a), h = [];
      for (let c = 0;c < u.length; c++) {
        let d = u[c];
        d = `(shared with ${n.get(u.slice(0, c + 1).join(`
`)).size}/${i.length} leaks) at ${d}`;
        let p = n.get(u.slice(0, c).join(`
`)), m = co([...p].map((f) => r(f)[c]), (f) => f);
        delete m[u[c]];
        for (let [f, A] of Object.entries(m))
          h.unshift(`    - stacktraces of ${A.length} other leaks continue with ${f}`);
        h.unshift(d);
      }
      o += `


==================== Leaking disposable ${l}/${i.length}: ${a.value.constructor.name} ====================
${h.join(`
`)}
============================================================

`;
    }
    return i.length > t && (o += `


... and ${i.length - t} more leaking disposables

`), { leaks: i, details: o };
  }
};
hr2.idx = 0;
function Ul(s) {
  dt2 = s;
}
if (Wl) {
  let s = "__is_disposable_tracked__";
  Ul(new class {
    trackDisposable(t) {
      let e = new Error("Potentially leaked disposable").stack;
      setTimeout(() => {
        t[s] || console.log(e);
      }, 3000);
    }
    setParent(t, e) {
      if (t && t !== D2.None)
        try {
          t[s] = true;
        } catch {}
    }
    markAsDisposed(t) {
      if (t && t !== D2.None)
        try {
          t[s] = true;
        } catch {}
    }
    markAsSingleton(t) {}
  });
}
function fr2(s) {
  return dt2?.trackDisposable(s), s;
}
function pr(s) {
  dt2?.markAsDisposed(s);
}
function vi2(s, t) {
  dt2?.setParent(s, t);
}
function Kl(s, t) {
  if (dt2)
    for (let e of s)
      dt2.setParent(e, t);
}
function Gn2(s) {
  return dt2?.markAsSingleton(s), s;
}
function Ne2(s) {
  if (zn2.is(s)) {
    let t = [];
    for (let e of s)
      if (e)
        try {
          e.dispose();
        } catch (i) {
          t.push(i);
        }
    if (t.length === 1)
      throw t[0];
    if (t.length > 1)
      throw new AggregateError(t, "Encountered errors while disposing of store");
    return Array.isArray(s) ? [] : s;
  } else if (s)
    return s.dispose(), s;
}
function ho(...s) {
  let t = C2(() => Ne2(s));
  return Kl(s, t), t;
}
function C2(s) {
  let t = fr2({ dispose: Kn2(() => {
    pr(t), s();
  }) });
  return t;
}
var dr2 = class dr {
  constructor() {
    this._toDispose = new Set;
    this._isDisposed = false;
    fr2(this);
  }
  dispose() {
    this._isDisposed || (pr(this), this._isDisposed = true, this.clear());
  }
  get isDisposed() {
    return this._isDisposed;
  }
  clear() {
    if (this._toDispose.size !== 0)
      try {
        Ne2(this._toDispose);
      } finally {
        this._toDispose.clear();
      }
  }
  add(t) {
    if (!t)
      return t;
    if (t === this)
      throw new Error("Cannot register a disposable on itself!");
    return vi2(t, this), this._isDisposed ? dr.DISABLE_DISPOSED_WARNING || console.warn(new Error("Trying to add a disposable to a DisposableStore that has already been disposed of. The added object will be leaked!").stack) : this._toDispose.add(t), t;
  }
  delete(t) {
    if (t) {
      if (t === this)
        throw new Error("Cannot dispose a disposable on itself!");
      this._toDispose.delete(t), t.dispose();
    }
  }
  deleteAndLeak(t) {
    t && this._toDispose.has(t) && (this._toDispose.delete(t), vi2(t, null));
  }
};
dr2.DISABLE_DISPOSED_WARNING = false;
var Ee2 = dr2;
var D2 = class {
  constructor() {
    this._store = new Ee2;
    fr2(this), vi2(this._store, this);
  }
  dispose() {
    pr(this), this._store.dispose();
  }
  _register(t) {
    if (t === this)
      throw new Error("Cannot register a disposable on itself!");
    return this._store.add(t);
  }
};
D2.None = Object.freeze({ dispose() {} });
var ye2 = class {
  constructor() {
    this._isDisposed = false;
    fr2(this);
  }
  get value() {
    return this._isDisposed ? undefined : this._value;
  }
  set value(t) {
    this._isDisposed || t === this._value || (this._value?.dispose(), t && vi2(t, this), this._value = t);
  }
  clear() {
    this.value = undefined;
  }
  dispose() {
    this._isDisposed = true, pr(this), this._value?.dispose(), this._value = undefined;
  }
  clearAndLeak() {
    let t = this._value;
    return this._value = undefined, t && vi2(t, null), t;
  }
};
var fe2 = typeof window == "object" ? window : globalThis;
var kt2 = class kt {
  constructor(t) {
    this.element = t, this.next = kt.Undefined, this.prev = kt.Undefined;
  }
};
kt2.Undefined = new kt2(undefined);
var G = kt2;
var Ct2 = class {
  constructor() {
    this._first = G.Undefined;
    this._last = G.Undefined;
    this._size = 0;
  }
  get size() {
    return this._size;
  }
  isEmpty() {
    return this._first === G.Undefined;
  }
  clear() {
    let t = this._first;
    for (;t !== G.Undefined; ) {
      let e = t.next;
      t.prev = G.Undefined, t.next = G.Undefined, t = e;
    }
    this._first = G.Undefined, this._last = G.Undefined, this._size = 0;
  }
  unshift(t) {
    return this._insert(t, false);
  }
  push(t) {
    return this._insert(t, true);
  }
  _insert(t, e) {
    let i = new G(t);
    if (this._first === G.Undefined)
      this._first = i, this._last = i;
    else if (e) {
      let n = this._last;
      this._last = i, i.prev = n, n.next = i;
    } else {
      let n = this._first;
      this._first = i, i.next = n, n.prev = i;
    }
    this._size += 1;
    let r = false;
    return () => {
      r || (r = true, this._remove(i));
    };
  }
  shift() {
    if (this._first !== G.Undefined) {
      let t = this._first.element;
      return this._remove(this._first), t;
    }
  }
  pop() {
    if (this._last !== G.Undefined) {
      let t = this._last.element;
      return this._remove(this._last), t;
    }
  }
  _remove(t) {
    if (t.prev !== G.Undefined && t.next !== G.Undefined) {
      let e = t.prev;
      e.next = t.next, t.next.prev = e;
    } else
      t.prev === G.Undefined && t.next === G.Undefined ? (this._first = G.Undefined, this._last = G.Undefined) : t.next === G.Undefined ? (this._last = this._last.prev, this._last.next = G.Undefined) : t.prev === G.Undefined && (this._first = this._first.next, this._first.prev = G.Undefined);
    this._size -= 1;
  }
  *[Symbol.iterator]() {
    let t = this._first;
    for (;t !== G.Undefined; )
      yield t.element, t = t.next;
  }
};
var zl = globalThis.performance && typeof globalThis.performance.now == "function";
var mr2 = class s {
  static create(t) {
    return new s(t);
  }
  constructor(t) {
    this._now = zl && t === false ? Date.now : globalThis.performance.now.bind(globalThis.performance), this._startTime = this._now(), this._stopTime = -1;
  }
  stop() {
    this._stopTime = this._now();
  }
  reset() {
    this._startTime = this._now(), this._stopTime = -1;
  }
  elapsed() {
    return this._stopTime !== -1 ? this._stopTime - this._startTime : this._now() - this._startTime;
  }
};
var Gl = false;
var fo = false;
var $l = false;
var $;
((Qe) => {
  Qe.None = () => D2.None;
  function t(y) {
    if ($l) {
      let { onDidAddListener: T } = y, g = gi2.create(), w = 0;
      y.onDidAddListener = () => {
        ++w === 2 && (console.warn("snapshotted emitter LIKELY used public and SHOULD HAVE BEEN created with DisposableStore. snapshotted here"), g.print()), T?.();
      };
    }
  }
  function e(y, T) {
    return d(y, () => {}, 0, undefined, true, undefined, T);
  }
  Qe.defer = e;
  function i(y) {
    return (T, g = null, w) => {
      let E = false, x;
      return x = y((N) => {
        if (!E)
          return x ? x.dispose() : E = true, T.call(g, N);
      }, null, w), E && x.dispose(), x;
    };
  }
  Qe.once = i;
  function r(y, T, g) {
    return h((w, E = null, x) => y((N) => w.call(E, T(N)), null, x), g);
  }
  Qe.map = r;
  function n(y, T, g) {
    return h((w, E = null, x) => y((N) => {
      T(N), w.call(E, N);
    }, null, x), g);
  }
  Qe.forEach = n;
  function o(y, T, g) {
    return h((w, E = null, x) => y((N) => T(N) && w.call(E, N), null, x), g);
  }
  Qe.filter = o;
  function l(y) {
    return y;
  }
  Qe.signal = l;
  function a(...y) {
    return (T, g = null, w) => {
      let E = ho(...y.map((x) => x((N) => T.call(g, N))));
      return c(E, w);
    };
  }
  Qe.any = a;
  function u(y, T, g, w) {
    let E = g;
    return r(y, (x) => (E = T(E, x), E), w);
  }
  Qe.reduce = u;
  function h(y, T) {
    let g, w = { onWillAddFirstListener() {
      g = y(E.fire, E);
    }, onDidRemoveLastListener() {
      g?.dispose();
    } };
    T || t(w);
    let E = new v(w);
    return T?.add(E), E.event;
  }
  function c(y, T) {
    return T instanceof Array ? T.push(y) : T && T.add(y), y;
  }
  function d(y, T, g = 100, w = false, E = false, x, N) {
    let Z, te, Oe, ze = 0, le, et = { leakWarningThreshold: x, onWillAddFirstListener() {
      Z = y((ht) => {
        ze++, te = T(te, ht), w && !Oe && (me.fire(te), te = undefined), le = () => {
          let fi = te;
          te = undefined, Oe = undefined, (!w || ze > 1) && me.fire(fi), ze = 0;
        }, typeof g == "number" ? (clearTimeout(Oe), Oe = setTimeout(le, g)) : Oe === undefined && (Oe = 0, queueMicrotask(le));
      });
    }, onWillRemoveListener() {
      E && ze > 0 && le?.();
    }, onDidRemoveLastListener() {
      le = undefined, Z.dispose();
    } };
    N || t(et);
    let me = new v(et);
    return N?.add(me), me.event;
  }
  Qe.debounce = d;
  function _(y, T = 0, g) {
    return Qe.debounce(y, (w, E) => w ? (w.push(E), w) : [E], T, undefined, true, undefined, g);
  }
  Qe.accumulate = _;
  function p(y, T = (w, E) => w === E, g) {
    let w = true, E;
    return o(y, (x) => {
      let N = w || !T(x, E);
      return w = false, E = x, N;
    }, g);
  }
  Qe.latch = p;
  function m(y, T, g) {
    return [Qe.filter(y, T, g), Qe.filter(y, (w) => !T(w), g)];
  }
  Qe.split = m;
  function f(y, T = false, g = [], w) {
    let E = g.slice(), x = y((te) => {
      E ? E.push(te) : Z.fire(te);
    });
    w && w.add(x);
    let N = () => {
      E?.forEach((te) => Z.fire(te)), E = null;
    }, Z = new v({ onWillAddFirstListener() {
      x || (x = y((te) => Z.fire(te)), w && w.add(x));
    }, onDidAddFirstListener() {
      E && (T ? setTimeout(N) : N());
    }, onDidRemoveLastListener() {
      x && x.dispose(), x = null;
    } });
    return w && w.add(Z), Z.event;
  }
  Qe.buffer = f;
  function A(y, T) {
    return (w, E, x) => {
      let N = T(new O);
      return y(function(Z) {
        let te = N.evaluate(Z);
        te !== R && w.call(E, te);
      }, undefined, x);
    };
  }
  Qe.chain = A;
  let R = Symbol("HaltChainable");

  class O {
    constructor() {
      this.steps = [];
    }
    map(T) {
      return this.steps.push(T), this;
    }
    forEach(T) {
      return this.steps.push((g) => (T(g), g)), this;
    }
    filter(T) {
      return this.steps.push((g) => T(g) ? g : R), this;
    }
    reduce(T, g) {
      let w = g;
      return this.steps.push((E) => (w = T(w, E), w)), this;
    }
    latch(T = (g, w) => g === w) {
      let g = true, w;
      return this.steps.push((E) => {
        let x = g || !T(E, w);
        return g = false, w = E, x ? E : R;
      }), this;
    }
    evaluate(T) {
      for (let g of this.steps)
        if (T = g(T), T === R)
          break;
      return T;
    }
  }
  function I(y, T, g = (w) => w) {
    let w = (...Z) => N.fire(g(...Z)), E = () => y.on(T, w), x = () => y.removeListener(T, w), N = new v({ onWillAddFirstListener: E, onDidRemoveLastListener: x });
    return N.event;
  }
  Qe.fromNodeEventEmitter = I;
  function k(y, T, g = (w) => w) {
    let w = (...Z) => N.fire(g(...Z)), E = () => y.addEventListener(T, w), x = () => y.removeEventListener(T, w), N = new v({ onWillAddFirstListener: E, onDidRemoveLastListener: x });
    return N.event;
  }
  Qe.fromDOMEventEmitter = k;
  function P(y) {
    return new Promise((T) => i(y)(T));
  }
  Qe.toPromise = P;
  function oe(y) {
    let T = new v;
    return y.then((g) => {
      T.fire(g);
    }, () => {
      T.fire(undefined);
    }).finally(() => {
      T.dispose();
    }), T.event;
  }
  Qe.fromPromise = oe;
  function Me(y, T) {
    return y((g) => T.fire(g));
  }
  Qe.forward = Me;
  function Pe(y, T, g) {
    return T(g), y((w) => T(w));
  }
  Qe.runAndSubscribe = Pe;

  class Ke {
    constructor(T, g) {
      this._observable = T;
      this._counter = 0;
      this._hasChanged = false;
      let w = { onWillAddFirstListener: () => {
        T.addObserver(this);
      }, onDidRemoveLastListener: () => {
        T.removeObserver(this);
      } };
      g || t(w), this.emitter = new v(w), g && g.add(this.emitter);
    }
    beginUpdate(T) {
      this._counter++;
    }
    handlePossibleChange(T) {}
    handleChange(T, g) {
      this._hasChanged = true;
    }
    endUpdate(T) {
      this._counter--, this._counter === 0 && (this._observable.reportChanges(), this._hasChanged && (this._hasChanged = false, this.emitter.fire(this._observable.get())));
    }
  }
  function di(y, T) {
    return new Ke(y, T).emitter.event;
  }
  Qe.fromObservable = di;
  function V(y) {
    return (T, g, w) => {
      let E = 0, x = false, N = { beginUpdate() {
        E++;
      }, endUpdate() {
        E--, E === 0 && (y.reportChanges(), x && (x = false, T.call(g)));
      }, handlePossibleChange() {}, handleChange() {
        x = true;
      } };
      y.addObserver(N), y.reportChanges();
      let Z = { dispose() {
        y.removeObserver(N);
      } };
      return w instanceof Ee2 ? w.add(Z) : Array.isArray(w) && w.push(Z), Z;
    };
  }
  Qe.fromObservableLight = V;
})($ ||= {});
var Mt2 = class Mt {
  constructor(t) {
    this.listenerCount = 0;
    this.invocationCount = 0;
    this.elapsedOverall = 0;
    this.durations = [];
    this.name = `${t}_${Mt._idPool++}`, Mt.all.add(this);
  }
  start(t) {
    this._stopWatch = new mr2, this.listenerCount = t;
  }
  stop() {
    if (this._stopWatch) {
      let t = this._stopWatch.elapsed();
      this.durations.push(t), this.elapsedOverall += t, this.invocationCount += 1, this._stopWatch = undefined;
    }
  }
};
Mt2.all = new Set, Mt2._idPool = 0;
var $n2 = Mt2;
var po = -1;
var br2 = class br {
  constructor(t, e, i = (br._idPool++).toString(16).padStart(3, "0")) {
    this._errorHandler = t;
    this.threshold = e;
    this.name = i;
    this._warnCountdown = 0;
  }
  dispose() {
    this._stacks?.clear();
  }
  check(t, e) {
    let i = this.threshold;
    if (i <= 0 || e < i)
      return;
    this._stacks || (this._stacks = new Map);
    let r = this._stacks.get(t.value) || 0;
    if (this._stacks.set(t.value, r + 1), this._warnCountdown -= 1, this._warnCountdown <= 0) {
      this._warnCountdown = i * 0.5;
      let [n, o] = this.getMostFrequentStack(), l = `[${this.name}] potential listener LEAK detected, having ${e} listeners already. MOST frequent listener (${o}):`;
      console.warn(l), console.warn(n);
      let a = new qn2(l, n);
      this._errorHandler(a);
    }
    return () => {
      let n = this._stacks.get(t.value) || 0;
      this._stacks.set(t.value, n - 1);
    };
  }
  getMostFrequentStack() {
    if (!this._stacks)
      return;
    let t, e = 0;
    for (let [i, r] of this._stacks)
      (!t || e < r) && (t = [i, r], e = r);
    return t;
  }
};
br2._idPool = 1;
var Vn2 = br2;
var gi2 = class s {
  constructor(t) {
    this.value = t;
  }
  static create() {
    let t = new Error;
    return new s(t.stack ?? "");
  }
  print() {
    console.warn(this.value.split(`
`).slice(2).join(`
`));
  }
};
var qn2 = class extends Error {
  constructor(t, e) {
    super(t), this.name = "ListenerLeakError", this.stack = e;
  }
};
var Yn2 = class extends Error {
  constructor(t, e) {
    super(t), this.name = "ListenerRefusalError", this.stack = e;
  }
};
var Vl = 0;
var Pt2 = class {
  constructor(t) {
    this.value = t;
    this.id = Vl++;
  }
};
var ql = 2;
var Yl = (s, t) => {
  if (s instanceof Pt2)
    t(s);
  else
    for (let e = 0;e < s.length; e++) {
      let i = s[e];
      i && t(i);
    }
};
var _r2;
if (Gl) {
  let s = [];
  setInterval(() => {
    s.length !== 0 && (console.warn("[LEAKING LISTENERS] GC'ed these listeners that were NOT yet disposed:"), console.warn(s.join(`
`)), s.length = 0);
  }, 3000), _r2 = new FinalizationRegistry((t) => {
    typeof t == "string" && s.push(t);
  });
}
var v = class {
  constructor(t) {
    this._size = 0;
    this._options = t, this._leakageMon = po > 0 || this._options?.leakWarningThreshold ? new Vn2(t?.onListenerError ?? Lt2, this._options?.leakWarningThreshold ?? po) : undefined, this._perfMon = this._options?._profName ? new $n2(this._options._profName) : undefined, this._deliveryQueue = this._options?.deliveryQueue;
  }
  dispose() {
    if (!this._disposed) {
      if (this._disposed = true, this._deliveryQueue?.current === this && this._deliveryQueue.reset(), this._listeners) {
        if (fo) {
          let t = this._listeners;
          queueMicrotask(() => {
            Yl(t, (e) => e.stack?.print());
          });
        }
        this._listeners = undefined, this._size = 0;
      }
      this._options?.onDidRemoveLastListener?.(), this._leakageMon?.dispose();
    }
  }
  get event() {
    return this._event ??= (t, e, i) => {
      if (this._leakageMon && this._size > this._leakageMon.threshold ** 2) {
        let a = `[${this._leakageMon.name}] REFUSES to accept new listeners because it exceeded its threshold by far (${this._size} vs ${this._leakageMon.threshold})`;
        console.warn(a);
        let u = this._leakageMon.getMostFrequentStack() ?? ["UNKNOWN stack", -1], h = new Yn2(`${a}. HINT: Stack shows most frequent listener (${u[1]}-times)`, u[0]);
        return (this._options?.onListenerError || Lt2)(h), D2.None;
      }
      if (this._disposed)
        return D2.None;
      e && (t = t.bind(e));
      let r = new Pt2(t), n, o;
      this._leakageMon && this._size >= Math.ceil(this._leakageMon.threshold * 0.2) && (r.stack = gi2.create(), n = this._leakageMon.check(r.stack, this._size + 1)), fo && (r.stack = o ?? gi2.create()), this._listeners ? this._listeners instanceof Pt2 ? (this._deliveryQueue ??= new jn2, this._listeners = [this._listeners, r]) : this._listeners.push(r) : (this._options?.onWillAddFirstListener?.(this), this._listeners = r, this._options?.onDidAddFirstListener?.(this)), this._size++;
      let l = C2(() => {
        _r2?.unregister(l), n?.(), this._removeListener(r);
      });
      if (i instanceof Ee2 ? i.add(l) : Array.isArray(i) && i.push(l), _r2) {
        let a = new Error().stack.split(`
`).slice(2, 3).join(`
`).trim(), u = /(file:|vscode-file:\/\/vscode-app)?(\/[^:]*:\d+:\d+)/.exec(a);
        _r2.register(l, u?.[2] ?? a, l);
      }
      return l;
    }, this._event;
  }
  _removeListener(t) {
    if (this._options?.onWillRemoveListener?.(this), !this._listeners)
      return;
    if (this._size === 1) {
      this._listeners = undefined, this._options?.onDidRemoveLastListener?.(this), this._size = 0;
      return;
    }
    let e = this._listeners, i = e.indexOf(t);
    if (i === -1)
      throw console.log("disposed?", this._disposed), console.log("size?", this._size), console.log("arr?", JSON.stringify(this._listeners)), new Error("Attempted to dispose unknown listener");
    this._size--, e[i] = undefined;
    let r = this._deliveryQueue.current === this;
    if (this._size * ql <= e.length) {
      let n = 0;
      for (let o = 0;o < e.length; o++)
        e[o] ? e[n++] = e[o] : r && (this._deliveryQueue.end--, n < this._deliveryQueue.i && this._deliveryQueue.i--);
      e.length = n;
    }
  }
  _deliver(t, e) {
    if (!t)
      return;
    let i = this._options?.onListenerError || Lt2;
    if (!i) {
      t.value(e);
      return;
    }
    try {
      t.value(e);
    } catch (r) {
      i(r);
    }
  }
  _deliverQueue(t) {
    let e = t.current._listeners;
    for (;t.i < t.end; )
      this._deliver(e[t.i++], t.value);
    t.reset();
  }
  fire(t) {
    if (this._deliveryQueue?.current && (this._deliverQueue(this._deliveryQueue), this._perfMon?.stop()), this._perfMon?.start(this._size), this._listeners)
      if (this._listeners instanceof Pt2)
        this._deliver(this._listeners, t);
      else {
        let e = this._deliveryQueue;
        e.enqueue(this, t, this._listeners.length), this._deliverQueue(e);
      }
    this._perfMon?.stop();
  }
  hasListeners() {
    return this._size > 0;
  }
};
var jn2 = class {
  constructor() {
    this.i = -1;
    this.end = 0;
  }
  enqueue(t, e, i) {
    this.i = 0, this.end = i, this.current = t, this.value = e;
  }
  reset() {
    this.i = this.end, this.current = undefined, this.value = undefined;
  }
};
var gr2 = class gr {
  constructor() {
    this.mapWindowIdToZoomLevel = new Map;
    this._onDidChangeZoomLevel = new v;
    this.onDidChangeZoomLevel = this._onDidChangeZoomLevel.event;
    this.mapWindowIdToZoomFactor = new Map;
    this._onDidChangeFullscreen = new v;
    this.onDidChangeFullscreen = this._onDidChangeFullscreen.event;
    this.mapWindowIdToFullScreen = new Map;
  }
  getZoomLevel(t) {
    return this.mapWindowIdToZoomLevel.get(this.getWindowId(t)) ?? 0;
  }
  setZoomLevel(t, e) {
    if (this.getZoomLevel(e) === t)
      return;
    let i = this.getWindowId(e);
    this.mapWindowIdToZoomLevel.set(i, t), this._onDidChangeZoomLevel.fire(i);
  }
  getZoomFactor(t) {
    return this.mapWindowIdToZoomFactor.get(this.getWindowId(t)) ?? 1;
  }
  setZoomFactor(t, e) {
    this.mapWindowIdToZoomFactor.set(this.getWindowId(e), t);
  }
  setFullscreen(t, e) {
    if (this.isFullscreen(e) === t)
      return;
    let i = this.getWindowId(e);
    this.mapWindowIdToFullScreen.set(i, t), this._onDidChangeFullscreen.fire(i);
  }
  isFullscreen(t) {
    return !!this.mapWindowIdToFullScreen.get(this.getWindowId(t));
  }
  getWindowId(t) {
    return t.vscodeWindowId;
  }
};
gr2.INSTANCE = new gr2;
var Si2 = gr2;
function Xl(s, t, e) {
  typeof t == "string" && (t = s.matchMedia(t)), t.addEventListener("change", e);
}
var Eu2 = Si2.INSTANCE.onDidChangeZoomLevel;
function mo(s) {
  return Si2.INSTANCE.getZoomFactor(s);
}
var Tu2 = Si2.INSTANCE.onDidChangeFullscreen;
var Ot2 = typeof navigator == "object" ? navigator.userAgent : "";
var Ei2 = Ot2.indexOf("Firefox") >= 0;
var Bt2 = Ot2.indexOf("AppleWebKit") >= 0;
var Ti2 = Ot2.indexOf("Chrome") >= 0;
var Sr2 = !Ti2 && Ot2.indexOf("Safari") >= 0;
var Iu = Ot2.indexOf("Electron/") >= 0;
var yu2 = Ot2.indexOf("Android") >= 0;
var vr2 = false;
if (typeof fe2.matchMedia == "function") {
  let s = fe2.matchMedia("(display-mode: standalone) or (display-mode: window-controls-overlay)"), t = fe2.matchMedia("(display-mode: fullscreen)");
  vr2 = s.matches, Xl(fe2, s, ({ matches: e }) => {
    vr2 && t.matches || (vr2 = e);
  });
}
function _o() {
  return vr2;
}
var Nt2 = "en";
var yr = false;
var xr2 = false;
var Ii2 = false;
var Zl = false;
var vo = false;
var go = false;
var Jl = false;
var Ql = false;
var ea = false;
var ta = false;
var Tr2;
var Ir = Nt2;
var bo = Nt2;
var ia;
var $e;
var Ve2 = globalThis;
var xe2;
typeof Ve2.vscode < "u" && typeof Ve2.vscode.process < "u" ? xe2 = Ve2.vscode.process : typeof process < "u" && typeof process?.versions?.node == "string" && (xe2 = process);
var So = typeof xe2?.versions?.electron == "string";
var ra = So && xe2?.type === "renderer";
if (typeof xe2 == "object") {
  yr = xe2.platform === "win32", xr2 = xe2.platform === "darwin", Ii2 = xe2.platform === "linux", Zl = Ii2 && !!xe2.env.SNAP && !!xe2.env.SNAP_REVISION, Jl = So, ea = !!xe2.env.CI || !!xe2.env.BUILD_ARTIFACTSTAGINGDIRECTORY, Tr2 = Nt2, Ir = Nt2;
  let s = xe2.env.VSCODE_NLS_CONFIG;
  if (s)
    try {
      let t = JSON.parse(s);
      Tr2 = t.userLocale, bo = t.osLocale, Ir = t.resolvedLanguage || Nt2, ia = t.languagePack?.translationsConfigFile;
    } catch {}
  vo = true;
} else
  typeof navigator == "object" && !ra ? ($e = navigator.userAgent, yr = $e.indexOf("Windows") >= 0, xr2 = $e.indexOf("Macintosh") >= 0, Ql = ($e.indexOf("Macintosh") >= 0 || $e.indexOf("iPad") >= 0 || $e.indexOf("iPhone") >= 0) && !!navigator.maxTouchPoints && navigator.maxTouchPoints > 0, Ii2 = $e.indexOf("Linux") >= 0, ta = $e?.indexOf("Mobi") >= 0, go = true, Ir = globalThis._VSCODE_NLS_LANGUAGE || Nt2, Tr2 = navigator.language.toLowerCase(), bo = Tr2) : console.error("Unable to resolve platform.");
var Xn2 = 0;
xr2 ? Xn2 = 1 : yr ? Xn2 = 3 : Ii2 && (Xn2 = 2);
var wr2 = yr;
var Te2 = xr2;
var Zn2 = Ii2;
var Dr2 = vo;
var na = go && typeof Ve2.importScripts == "function";
var xu2 = na ? Ve2.origin : undefined;
var Fe = $e;
var st2 = Ir;
var sa;
((i) => {
  function s() {
    return st2;
  }
  i.value = s;
  function t() {
    return st2.length === 2 ? st2 === "en" : st2.length >= 3 ? st2[0] === "e" && st2[1] === "n" && st2[2] === "-" : false;
  }
  i.isDefaultVariant = t;
  function e() {
    return st2 === "en";
  }
  i.isDefault = e;
})(sa ||= {});
var oa = typeof Ve2.postMessage == "function" && !Ve2.importScripts;
var Eo = (() => {
  if (oa) {
    let s = [];
    Ve2.addEventListener("message", (e) => {
      if (e.data && e.data.vscodeScheduleAsyncWork)
        for (let i = 0, r = s.length;i < r; i++) {
          let n = s[i];
          if (n.id === e.data.vscodeScheduleAsyncWork) {
            s.splice(i, 1), n.callback();
            return;
          }
        }
    });
    let t = 0;
    return (e) => {
      let i = ++t;
      s.push({ id: i, callback: e }), Ve2.postMessage({ vscodeScheduleAsyncWork: i }, "*");
    };
  }
  return (s) => setTimeout(s);
})();
var la = !!(Fe && Fe.indexOf("Chrome") >= 0);
var wu = !!(Fe && Fe.indexOf("Firefox") >= 0);
var Du = !!(!la && Fe && Fe.indexOf("Safari") >= 0);
var Ru = !!(Fe && Fe.indexOf("Edg/") >= 0);
var Lu = !!(Fe && Fe.indexOf("Android") >= 0);
var ot2 = typeof navigator == "object" ? navigator : {};
var aa = { clipboard: { writeText: Dr2 || document.queryCommandSupported && document.queryCommandSupported("copy") || !!(ot2 && ot2.clipboard && ot2.clipboard.writeText), readText: Dr2 || !!(ot2 && ot2.clipboard && ot2.clipboard.readText) }, keyboard: Dr2 || _o() ? 0 : ot2.keyboard || Sr2 ? 1 : 2, touch: "ontouchstart" in fe2 || ot2.maxTouchPoints > 0, pointerEvents: fe2.PointerEvent && (("ontouchstart" in fe2) || navigator.maxTouchPoints > 0) };
var yi2 = class {
  constructor() {
    this._keyCodeToStr = [], this._strToKeyCode = Object.create(null);
  }
  define(t, e) {
    this._keyCodeToStr[t] = e, this._strToKeyCode[e.toLowerCase()] = t;
  }
  keyCodeToStr(t) {
    return this._keyCodeToStr[t];
  }
  strToKeyCode(t) {
    return this._strToKeyCode[t.toLowerCase()] || 0;
  }
};
var Jn2 = new yi2;
var To = new yi2;
var Io = new yi2;
var yo = new Array(230);
var Qn2;
((o) => {
  function s(l) {
    return Jn2.keyCodeToStr(l);
  }
  o.toString = s;
  function t(l) {
    return Jn2.strToKeyCode(l);
  }
  o.fromString = t;
  function e(l) {
    return To.keyCodeToStr(l);
  }
  o.toUserSettingsUS = e;
  function i(l) {
    return Io.keyCodeToStr(l);
  }
  o.toUserSettingsGeneral = i;
  function r(l) {
    return To.strToKeyCode(l) || Io.strToKeyCode(l);
  }
  o.fromUserSettings = r;
  function n(l) {
    if (l >= 98 && l <= 113)
      return null;
    switch (l) {
      case 16:
        return "Up";
      case 18:
        return "Down";
      case 15:
        return "Left";
      case 17:
        return "Right";
    }
    return Jn2.keyCodeToStr(l);
  }
  o.toElectronAccelerator = n;
})(Qn2 ||= {});
var Rr2 = class s {
  constructor(t, e, i, r, n) {
    this.ctrlKey = t;
    this.shiftKey = e;
    this.altKey = i;
    this.metaKey = r;
    this.keyCode = n;
  }
  equals(t) {
    return t instanceof s && this.ctrlKey === t.ctrlKey && this.shiftKey === t.shiftKey && this.altKey === t.altKey && this.metaKey === t.metaKey && this.keyCode === t.keyCode;
  }
  getHashCode() {
    let t = this.ctrlKey ? "1" : "0", e = this.shiftKey ? "1" : "0", i = this.altKey ? "1" : "0", r = this.metaKey ? "1" : "0";
    return `K${t}${e}${i}${r}${this.keyCode}`;
  }
  isModifierKey() {
    return this.keyCode === 0 || this.keyCode === 5 || this.keyCode === 57 || this.keyCode === 6 || this.keyCode === 4;
  }
  toKeybinding() {
    return new es([this]);
  }
  isDuplicateModifierCase() {
    return this.ctrlKey && this.keyCode === 5 || this.shiftKey && this.keyCode === 4 || this.altKey && this.keyCode === 6 || this.metaKey && this.keyCode === 57;
  }
};
var es = class {
  constructor(t) {
    if (t.length === 0)
      throw eo("chords");
    this.chords = t;
  }
  getHashCode() {
    let t = "";
    for (let e = 0, i = this.chords.length;e < i; e++)
      e !== 0 && (t += ";"), t += this.chords[e].getHashCode();
    return t;
  }
  equals(t) {
    if (t === null || this.chords.length !== t.chords.length)
      return false;
    for (let e = 0;e < this.chords.length; e++)
      if (!this.chords[e].equals(t.chords[e]))
        return false;
    return true;
  }
};
function ca(s) {
  if (s.charCode) {
    let e = String.fromCharCode(s.charCode).toUpperCase();
    return Qn2.fromString(e);
  }
  let t = s.keyCode;
  if (t === 3)
    return 7;
  if (Ei2)
    switch (t) {
      case 59:
        return 85;
      case 60:
        if (Zn2)
          return 97;
        break;
      case 61:
        return 86;
      case 107:
        return 109;
      case 109:
        return 111;
      case 173:
        return 88;
      case 224:
        if (Te2)
          return 57;
        break;
    }
  else if (Bt2) {
    if (Te2 && t === 93)
      return 57;
    if (!Te2 && t === 92)
      return 57;
  }
  return yo[t] || 0;
}
var ua = Te2 ? 256 : 2048;
var ha = 512;
var da = 1024;
var fa = Te2 ? 2048 : 256;
var ft = class {
  constructor(t) {
    this._standardKeyboardEventBrand = true;
    let e = t;
    this.browserEvent = e, this.target = e.target, this.ctrlKey = e.ctrlKey, this.shiftKey = e.shiftKey, this.altKey = e.altKey, this.metaKey = e.metaKey, this.altGraphKey = e.getModifierState?.("AltGraph"), this.keyCode = ca(e), this.code = e.code, this.ctrlKey = this.ctrlKey || this.keyCode === 5, this.altKey = this.altKey || this.keyCode === 6, this.shiftKey = this.shiftKey || this.keyCode === 4, this.metaKey = this.metaKey || this.keyCode === 57, this._asKeybinding = this._computeKeybinding(), this._asKeyCodeChord = this._computeKeyCodeChord();
  }
  preventDefault() {
    this.browserEvent && this.browserEvent.preventDefault && this.browserEvent.preventDefault();
  }
  stopPropagation() {
    this.browserEvent && this.browserEvent.stopPropagation && this.browserEvent.stopPropagation();
  }
  toKeyCodeChord() {
    return this._asKeyCodeChord;
  }
  equals(t) {
    return this._asKeybinding === t;
  }
  _computeKeybinding() {
    let t = 0;
    this.keyCode !== 5 && this.keyCode !== 4 && this.keyCode !== 6 && this.keyCode !== 57 && (t = this.keyCode);
    let e = 0;
    return this.ctrlKey && (e |= ua), this.altKey && (e |= ha), this.shiftKey && (e |= da), this.metaKey && (e |= fa), e |= t, e;
  }
  _computeKeyCodeChord() {
    let t = 0;
    return this.keyCode !== 5 && this.keyCode !== 4 && this.keyCode !== 6 && this.keyCode !== 57 && (t = this.keyCode), new Rr2(this.ctrlKey, this.shiftKey, this.altKey, this.metaKey, t);
  }
};
var wo = new WeakMap;
function pa(s) {
  if (!s.parent || s.parent === s)
    return null;
  try {
    let t = s.location, e = s.parent.location;
    if (t.origin !== "null" && e.origin !== "null" && t.origin !== e.origin)
      return null;
  } catch {
    return null;
  }
  return s.parent;
}
var Lr2 = class {
  static getSameOriginWindowChain(t) {
    let e = wo.get(t);
    if (!e) {
      e = [], wo.set(t, e);
      let i = t, r;
      do
        r = pa(i), r ? e.push({ window: new WeakRef(i), iframeElement: i.frameElement || null }) : e.push({ window: new WeakRef(i), iframeElement: null }), i = r;
      while (i);
    }
    return e.slice(0);
  }
  static getPositionOfChildWindowRelativeToAncestorWindow(t, e) {
    if (!e || t === e)
      return { top: 0, left: 0 };
    let i = 0, r = 0, n = this.getSameOriginWindowChain(t);
    for (let o of n) {
      let l = o.window.deref();
      if (i += l?.scrollY ?? 0, r += l?.scrollX ?? 0, l === e || !o.iframeElement)
        break;
      let a = o.iframeElement.getBoundingClientRect();
      i += a.top, r += a.left;
    }
    return { top: i, left: r };
  }
};
var qe2 = class {
  constructor(t, e) {
    this.timestamp = Date.now(), this.browserEvent = e, this.leftButton = e.button === 0, this.middleButton = e.button === 1, this.rightButton = e.button === 2, this.buttons = e.buttons, this.target = e.target, this.detail = e.detail || 1, e.type === "dblclick" && (this.detail = 2), this.ctrlKey = e.ctrlKey, this.shiftKey = e.shiftKey, this.altKey = e.altKey, this.metaKey = e.metaKey, typeof e.pageX == "number" ? (this.posx = e.pageX, this.posy = e.pageY) : (this.posx = e.clientX + this.target.ownerDocument.body.scrollLeft + this.target.ownerDocument.documentElement.scrollLeft, this.posy = e.clientY + this.target.ownerDocument.body.scrollTop + this.target.ownerDocument.documentElement.scrollTop);
    let i = Lr2.getPositionOfChildWindowRelativeToAncestorWindow(t, e.view);
    this.posx -= i.left, this.posy -= i.top;
  }
  preventDefault() {
    this.browserEvent.preventDefault();
  }
  stopPropagation() {
    this.browserEvent.stopPropagation();
  }
};
var xi2 = class {
  constructor(t, e = 0, i = 0) {
    this.browserEvent = t || null, this.target = t ? t.target || t.targetNode || t.srcElement : null, this.deltaY = i, this.deltaX = e;
    let r = false;
    if (Ti2) {
      let n = navigator.userAgent.match(/Chrome\/(\d+)/);
      r = (n ? parseInt(n[1]) : 123) <= 122;
    }
    if (t) {
      let n = t, o = t, l = t.view?.devicePixelRatio || 1;
      if (typeof n.wheelDeltaY < "u")
        r ? this.deltaY = n.wheelDeltaY / (120 * l) : this.deltaY = n.wheelDeltaY / 120;
      else if (typeof o.VERTICAL_AXIS < "u" && o.axis === o.VERTICAL_AXIS)
        this.deltaY = -o.detail / 3;
      else if (t.type === "wheel") {
        let a = t;
        a.deltaMode === a.DOM_DELTA_LINE ? Ei2 && !Te2 ? this.deltaY = -t.deltaY / 3 : this.deltaY = -t.deltaY : this.deltaY = -t.deltaY / 40;
      }
      if (typeof n.wheelDeltaX < "u")
        Sr2 && wr2 ? this.deltaX = -(n.wheelDeltaX / 120) : r ? this.deltaX = n.wheelDeltaX / (120 * l) : this.deltaX = n.wheelDeltaX / 120;
      else if (typeof o.HORIZONTAL_AXIS < "u" && o.axis === o.HORIZONTAL_AXIS)
        this.deltaX = -t.detail / 3;
      else if (t.type === "wheel") {
        let a = t;
        a.deltaMode === a.DOM_DELTA_LINE ? Ei2 && !Te2 ? this.deltaX = -t.deltaX / 3 : this.deltaX = -t.deltaX : this.deltaX = -t.deltaX / 40;
      }
      this.deltaY === 0 && this.deltaX === 0 && t.wheelDelta && (r ? this.deltaY = t.wheelDelta / (120 * l) : this.deltaY = t.wheelDelta / 120);
    }
  }
  preventDefault() {
    this.browserEvent?.preventDefault();
  }
  stopPropagation() {
    this.browserEvent?.stopPropagation();
  }
};
var Do = Object.freeze(function(s, t) {
  let e = setTimeout(s.bind(t), 0);
  return { dispose() {
    clearTimeout(e);
  } };
});
var ma;
((i) => {
  function s(r) {
    return r === i.None || r === i.Cancelled || r instanceof ts ? true : !r || typeof r != "object" ? false : typeof r.isCancellationRequested == "boolean" && typeof r.onCancellationRequested == "function";
  }
  i.isCancellationToken = s, i.None = Object.freeze({ isCancellationRequested: false, onCancellationRequested: $.None }), i.Cancelled = Object.freeze({ isCancellationRequested: true, onCancellationRequested: Do });
})(ma ||= {});
var ts = class {
  constructor() {
    this._isCancelled = false;
    this._emitter = null;
  }
  cancel() {
    this._isCancelled || (this._isCancelled = true, this._emitter && (this._emitter.fire(undefined), this.dispose()));
  }
  get isCancellationRequested() {
    return this._isCancelled;
  }
  get onCancellationRequested() {
    return this._isCancelled ? Do : (this._emitter || (this._emitter = new v), this._emitter.event);
  }
  dispose() {
    this._emitter && (this._emitter.dispose(), this._emitter = null);
  }
};
var _a = Symbol("MicrotaskDelay");
var Ye2 = class {
  constructor(t, e) {
    this._isDisposed = false;
    this._token = -1, typeof t == "function" && typeof e == "number" && this.setIfNotSet(t, e);
  }
  dispose() {
    this.cancel(), this._isDisposed = true;
  }
  cancel() {
    this._token !== -1 && (clearTimeout(this._token), this._token = -1);
  }
  cancelAndSet(t, e) {
    if (this._isDisposed)
      throw new Rt2("Calling 'cancelAndSet' on a disposed TimeoutTimer");
    this.cancel(), this._token = setTimeout(() => {
      this._token = -1, t();
    }, e);
  }
  setIfNotSet(t, e) {
    if (this._isDisposed)
      throw new Rt2("Calling 'setIfNotSet' on a disposed TimeoutTimer");
    this._token === -1 && (this._token = setTimeout(() => {
      this._token = -1, t();
    }, e));
  }
};
var kr2 = class {
  constructor() {
    this.disposable = undefined;
    this.isDisposed = false;
  }
  cancel() {
    this.disposable?.dispose(), this.disposable = undefined;
  }
  cancelAndSet(t, e, i = globalThis) {
    if (this.isDisposed)
      throw new Rt2("Calling 'cancelAndSet' on a disposed IntervalTimer");
    this.cancel();
    let r = i.setInterval(() => {
      t();
    }, e);
    this.disposable = C2(() => {
      i.clearInterval(r), this.disposable = undefined;
    });
  }
  dispose() {
    this.cancel(), this.isDisposed = true;
  }
};
var ba;
var Ar2;
(function() {
  typeof globalThis.requestIdleCallback != "function" || typeof globalThis.cancelIdleCallback != "function" ? Ar2 = (s, t) => {
    Eo(() => {
      if (e)
        return;
      let i = Date.now() + 15;
      t(Object.freeze({ didTimeout: true, timeRemaining() {
        return Math.max(0, i - Date.now());
      } }));
    });
    let e = false;
    return { dispose() {
      e || (e = true);
    } };
  } : Ar2 = (s, t, e) => {
    let i = s.requestIdleCallback(t, typeof e == "number" ? { timeout: e } : undefined), r = false;
    return { dispose() {
      r || (r = true, s.cancelIdleCallback(i));
    } };
  }, ba = (s) => Ar2(globalThis, s);
})();
var va;
((e) => {
  async function s(i) {
    let r, n = await Promise.all(i.map((o) => o.then((l) => l, (l) => {
      r || (r = l);
    })));
    if (typeof r < "u")
      throw r;
    return n;
  }
  e.settled = s;
  function t(i) {
    return new Promise(async (r, n) => {
      try {
        await i(r, n);
      } catch (o) {
        n(o);
      }
    });
  }
  e.withAsyncBody = t;
})(va ||= {});
var _e2 = class _e {
  static fromArray(t) {
    return new _e((e) => {
      e.emitMany(t);
    });
  }
  static fromPromise(t) {
    return new _e(async (e) => {
      e.emitMany(await t);
    });
  }
  static fromPromises(t) {
    return new _e(async (e) => {
      await Promise.all(t.map(async (i) => e.emitOne(await i)));
    });
  }
  static merge(t) {
    return new _e(async (e) => {
      await Promise.all(t.map(async (i) => {
        for await (let r of i)
          e.emitOne(r);
      }));
    });
  }
  constructor(t, e) {
    this._state = 0, this._results = [], this._error = null, this._onReturn = e, this._onStateChanged = new v, queueMicrotask(async () => {
      let i = { emitOne: (r) => this.emitOne(r), emitMany: (r) => this.emitMany(r), reject: (r) => this.reject(r) };
      try {
        await Promise.resolve(t(i)), this.resolve();
      } catch (r) {
        this.reject(r);
      } finally {
        i.emitOne = undefined, i.emitMany = undefined, i.reject = undefined;
      }
    });
  }
  [Symbol.asyncIterator]() {
    let t = 0;
    return { next: async () => {
      do {
        if (this._state === 2)
          throw this._error;
        if (t < this._results.length)
          return { done: false, value: this._results[t++] };
        if (this._state === 1)
          return { done: true, value: undefined };
        await $.toPromise(this._onStateChanged.event);
      } while (true);
    }, return: async () => (this._onReturn?.(), { done: true, value: undefined }) };
  }
  static map(t, e) {
    return new _e(async (i) => {
      for await (let r of t)
        i.emitOne(e(r));
    });
  }
  map(t) {
    return _e.map(this, t);
  }
  static filter(t, e) {
    return new _e(async (i) => {
      for await (let r of t)
        e(r) && i.emitOne(r);
    });
  }
  filter(t) {
    return _e.filter(this, t);
  }
  static coalesce(t) {
    return _e.filter(t, (e) => !!e);
  }
  coalesce() {
    return _e.coalesce(this);
  }
  static async toPromise(t) {
    let e = [];
    for await (let i of t)
      e.push(i);
    return e;
  }
  toPromise() {
    return _e.toPromise(this);
  }
  emitOne(t) {
    this._state === 0 && (this._results.push(t), this._onStateChanged.fire());
  }
  emitMany(t) {
    this._state === 0 && (this._results = this._results.concat(t), this._onStateChanged.fire());
  }
  resolve() {
    this._state === 0 && (this._state = 1, this._onStateChanged.fire());
  }
  reject(t) {
    this._state === 0 && (this._state = 2, this._error = t, this._onStateChanged.fire());
  }
};
_e2.EMPTY = _e2.fromArray([]);
function Lo(s) {
  return 55296 <= s && s <= 56319;
}
function is2(s) {
  return 56320 <= s && s <= 57343;
}
function Ao(s, t) {
  return (s - 55296 << 10) + (t - 56320) + 65536;
}
function Mo(s) {
  return ns2(s, 0);
}
function ns2(s, t) {
  switch (typeof s) {
    case "object":
      return s === null ? je2(349, t) : Array.isArray(s) ? Ea(s, t) : Ta(s, t);
    case "string":
      return Po(s, t);
    case "boolean":
      return Sa(s, t);
    case "number":
      return je2(s, t);
    case "undefined":
      return je2(937, t);
    default:
      return je2(617, t);
  }
}
function je2(s, t) {
  return (t << 5) - t + s | 0;
}
function Sa(s, t) {
  return je2(s ? 433 : 863, t);
}
function Po(s, t) {
  t = je2(149417, t);
  for (let e = 0, i = s.length;e < i; e++)
    t = je2(s.charCodeAt(e), t);
  return t;
}
function Ea(s, t) {
  return t = je2(104579, t), s.reduce((e, i) => ns2(i, e), t);
}
function Ta(s, t) {
  return t = je2(181387, t), Object.keys(s).sort().reduce((e, i) => (e = Po(i, e), ns2(s[i], e)), t);
}
function rs2(s, t, e = 32) {
  let i = e - t, r = ~((1 << i) - 1);
  return (s << t | (r & s) >>> i) >>> 0;
}
function ko(s, t = 0, e = s.byteLength, i = 0) {
  for (let r = 0;r < e; r++)
    s[t + r] = i;
}
function Ia(s, t, e = "0") {
  for (;s.length < t; )
    s = e + s;
  return s;
}
function wi2(s, t = 32) {
  return s instanceof ArrayBuffer ? Array.from(new Uint8Array(s)).map((e) => e.toString(16).padStart(2, "0")).join("") : Ia((s >>> 0).toString(16), t / 4);
}
var Cr2 = class Cr {
  constructor() {
    this._h0 = 1732584193;
    this._h1 = 4023233417;
    this._h2 = 2562383102;
    this._h3 = 271733878;
    this._h4 = 3285377520;
    this._buff = new Uint8Array(67), this._buffDV = new DataView(this._buff.buffer), this._buffLen = 0, this._totalLen = 0, this._leftoverHighSurrogate = 0, this._finished = false;
  }
  update(t) {
    let e = t.length;
    if (e === 0)
      return;
    let i = this._buff, r = this._buffLen, n = this._leftoverHighSurrogate, o, l;
    for (n !== 0 ? (o = n, l = -1, n = 0) : (o = t.charCodeAt(0), l = 0);; ) {
      let a = o;
      if (Lo(o))
        if (l + 1 < e) {
          let u = t.charCodeAt(l + 1);
          is2(u) ? (l++, a = Ao(o, u)) : a = 65533;
        } else {
          n = o;
          break;
        }
      else
        is2(o) && (a = 65533);
      if (r = this._push(i, r, a), l++, l < e)
        o = t.charCodeAt(l);
      else
        break;
    }
    this._buffLen = r, this._leftoverHighSurrogate = n;
  }
  _push(t, e, i) {
    return i < 128 ? t[e++] = i : i < 2048 ? (t[e++] = 192 | (i & 1984) >>> 6, t[e++] = 128 | (i & 63) >>> 0) : i < 65536 ? (t[e++] = 224 | (i & 61440) >>> 12, t[e++] = 128 | (i & 4032) >>> 6, t[e++] = 128 | (i & 63) >>> 0) : (t[e++] = 240 | (i & 1835008) >>> 18, t[e++] = 128 | (i & 258048) >>> 12, t[e++] = 128 | (i & 4032) >>> 6, t[e++] = 128 | (i & 63) >>> 0), e >= 64 && (this._step(), e -= 64, this._totalLen += 64, t[0] = t[64], t[1] = t[65], t[2] = t[66]), e;
  }
  digest() {
    return this._finished || (this._finished = true, this._leftoverHighSurrogate && (this._leftoverHighSurrogate = 0, this._buffLen = this._push(this._buff, this._buffLen, 65533)), this._totalLen += this._buffLen, this._wrapUp()), wi2(this._h0) + wi2(this._h1) + wi2(this._h2) + wi2(this._h3) + wi2(this._h4);
  }
  _wrapUp() {
    this._buff[this._buffLen++] = 128, ko(this._buff, this._buffLen), this._buffLen > 56 && (this._step(), ko(this._buff));
    let t = 8 * this._totalLen;
    this._buffDV.setUint32(56, Math.floor(t / 4294967296), false), this._buffDV.setUint32(60, t % 4294967296, false), this._step();
  }
  _step() {
    let t = Cr._bigBlock32, e = this._buffDV;
    for (let c = 0;c < 64; c += 4)
      t.setUint32(c, e.getUint32(c, false), false);
    for (let c = 64;c < 320; c += 4)
      t.setUint32(c, rs2(t.getUint32(c - 12, false) ^ t.getUint32(c - 32, false) ^ t.getUint32(c - 56, false) ^ t.getUint32(c - 64, false), 1), false);
    let i = this._h0, r = this._h1, n = this._h2, o = this._h3, l = this._h4, a, u, h;
    for (let c = 0;c < 80; c++)
      c < 20 ? (a = r & n | ~r & o, u = 1518500249) : c < 40 ? (a = r ^ n ^ o, u = 1859775393) : c < 60 ? (a = r & n | r & o | n & o, u = 2400959708) : (a = r ^ n ^ o, u = 3395469782), h = rs2(i, 5) + a + l + u + t.getUint32(c * 4, false) & 4294967295, l = o, o = n, n = rs2(r, 30), r = i, i = h;
    this._h0 = this._h0 + i & 4294967295, this._h1 = this._h1 + r & 4294967295, this._h2 = this._h2 + n & 4294967295, this._h3 = this._h3 + o & 4294967295, this._h4 = this._h4 + l & 4294967295;
  }
};
Cr2._bigBlock32 = new DataView(new ArrayBuffer(320));
var { registerWindow: Bh, getWindow: be2, getDocument: Nh, getWindows: Fh, getWindowsCount: Hh, getWindowId: Oo, getWindowById: Wh, hasWindow: Uh, onDidRegisterWindow: No, onWillUnregisterWindow: Kh, onDidUnregisterWindow: zh } = function() {
  let s = new Map;
  let t = { window: fe2, disposables: new Ee2 };
  s.set(fe2.vscodeWindowId, t);
  let e = new v, i = new v, r = new v;
  function n(o, l) {
    return (typeof o == "number" ? s.get(o) : undefined) ?? (l ? t : undefined);
  }
  return { onDidRegisterWindow: e.event, onWillUnregisterWindow: r.event, onDidUnregisterWindow: i.event, registerWindow(o) {
    if (s.has(o.vscodeWindowId))
      return D2.None;
    let l = new Ee2, a = { window: o, disposables: l.add(new Ee2) };
    return s.set(o.vscodeWindowId, a), l.add(C2(() => {
      s.delete(o.vscodeWindowId), i.fire(o);
    })), l.add(L(o, Y2.BEFORE_UNLOAD, () => {
      r.fire(o);
    })), e.fire(a), l;
  }, getWindows() {
    return s.values();
  }, getWindowsCount() {
    return s.size;
  }, getWindowId(o) {
    return o.vscodeWindowId;
  }, hasWindow(o) {
    return s.has(o);
  }, getWindowById: n, getWindow(o) {
    let l = o;
    if (l?.ownerDocument?.defaultView)
      return l.ownerDocument.defaultView.window;
    let a = o;
    return a?.view ? a.view.window : fe2;
  }, getDocument(o) {
    return be2(o).document;
  } };
}();
var ss2 = class {
  constructor(t, e, i, r) {
    this._node = t, this._type = e, this._handler = i, this._options = r || false, this._node.addEventListener(this._type, this._handler, this._options);
  }
  dispose() {
    this._handler && (this._node.removeEventListener(this._type, this._handler, this._options), this._node = null, this._handler = null);
  }
};
function L(s, t, e, i) {
  return new ss2(s, t, e, i);
}
function ya(s, t) {
  return function(e) {
    return t(new qe2(s, e));
  };
}
function xa(s) {
  return function(t) {
    return s(new ft(t));
  };
}
var os2 = function(t, e, i, r) {
  let n = i;
  return e === "click" || e === "mousedown" || e === "contextmenu" ? n = ya(be2(t), i) : (e === "keydown" || e === "keypress" || e === "keyup") && (n = xa(i)), L(t, e, n, r);
};
var wa;
var mt;
var Mr2 = class extends kr2 {
  constructor(t) {
    super(), this.defaultTarget = t && be2(t);
  }
  cancelAndSet(t, e, i) {
    return super.cancelAndSet(t, e, i ?? this.defaultTarget);
  }
};
var Di2 = class {
  constructor(t, e = 0) {
    this._runner = t, this.priority = e, this._canceled = false;
  }
  dispose() {
    this._canceled = true;
  }
  execute() {
    if (!this._canceled)
      try {
        this._runner();
      } catch (t) {
        Lt2(t);
      }
  }
  static sort(t, e) {
    return e.priority - t.priority;
  }
};
(function() {
  let s = new Map, t = new Map, e = new Map, i = new Map, r = (n) => {
    e.set(n, false);
    let o = s.get(n) ?? [];
    for (t.set(n, o), s.set(n, []), i.set(n, true);o.length > 0; )
      o.sort(Di2.sort), o.shift().execute();
    i.set(n, false);
  };
  mt = (n, o, l = 0) => {
    let a = Oo(n), u = new Di2(o, l), h = s.get(a);
    return h || (h = [], s.set(a, h)), h.push(u), e.get(a) || (e.set(a, true), n.requestAnimationFrame(() => r(a))), u;
  }, wa = (n, o, l) => {
    let a = Oo(n);
    if (i.get(a)) {
      let u = new Di2(o, l), h = t.get(a);
      return h || (h = [], t.set(a, h)), h.push(u), u;
    } else
      return mt(n, o, l);
  };
})();
var pt2 = class pt {
  constructor(t, e) {
    this.width = t;
    this.height = e;
  }
  with(t = this.width, e = this.height) {
    return t !== this.width || e !== this.height ? new pt(t, e) : this;
  }
  static is(t) {
    return typeof t == "object" && typeof t.height == "number" && typeof t.width == "number";
  }
  static lift(t) {
    return t instanceof pt ? t : new pt(t.width, t.height);
  }
  static equals(t, e) {
    return t === e ? true : !t || !e ? false : t.width === e.width && t.height === e.height;
  }
};
pt2.None = new pt2(0, 0);
function Fo(s) {
  let t = s.getBoundingClientRect(), e = be2(s);
  return { left: t.left + e.scrollX, top: t.top + e.scrollY, width: t.width, height: t.height };
}
var Gh = new class {
  constructor() {
    this.mutationObservers = new Map;
  }
  observe(s, t, e) {
    let i = this.mutationObservers.get(s);
    i || (i = new Map, this.mutationObservers.set(s, i));
    let r = Mo(e), n = i.get(r);
    if (n)
      n.users += 1;
    else {
      let o = new v, l = new MutationObserver((u) => o.fire(u));
      l.observe(s, e);
      let a = n = { users: 1, observer: l, onDidMutate: o.event };
      t.add(C2(() => {
        a.users -= 1, a.users === 0 && (o.dispose(), l.disconnect(), i?.delete(r), i?.size === 0 && this.mutationObservers.delete(s));
      })), i.set(r, n);
    }
    return n.onDidMutate;
  }
};
var Y2 = { CLICK: "click", AUXCLICK: "auxclick", DBLCLICK: "dblclick", MOUSE_UP: "mouseup", MOUSE_DOWN: "mousedown", MOUSE_OVER: "mouseover", MOUSE_MOVE: "mousemove", MOUSE_OUT: "mouseout", MOUSE_ENTER: "mouseenter", MOUSE_LEAVE: "mouseleave", MOUSE_WHEEL: "wheel", POINTER_UP: "pointerup", POINTER_DOWN: "pointerdown", POINTER_MOVE: "pointermove", POINTER_LEAVE: "pointerleave", CONTEXT_MENU: "contextmenu", WHEEL: "wheel", KEY_DOWN: "keydown", KEY_PRESS: "keypress", KEY_UP: "keyup", LOAD: "load", BEFORE_UNLOAD: "beforeunload", UNLOAD: "unload", PAGE_SHOW: "pageshow", PAGE_HIDE: "pagehide", PASTE: "paste", ABORT: "abort", ERROR: "error", RESIZE: "resize", SCROLL: "scroll", FULLSCREEN_CHANGE: "fullscreenchange", WK_FULLSCREEN_CHANGE: "webkitfullscreenchange", SELECT: "select", CHANGE: "change", SUBMIT: "submit", RESET: "reset", FOCUS: "focus", FOCUS_IN: "focusin", FOCUS_OUT: "focusout", BLUR: "blur", INPUT: "input", STORAGE: "storage", DRAG_START: "dragstart", DRAG: "drag", DRAG_ENTER: "dragenter", DRAG_LEAVE: "dragleave", DRAG_OVER: "dragover", DROP: "drop", DRAG_END: "dragend", ANIMATION_START: Bt2 ? "webkitAnimationStart" : "animationstart", ANIMATION_END: Bt2 ? "webkitAnimationEnd" : "animationend", ANIMATION_ITERATION: Bt2 ? "webkitAnimationIteration" : "animationiteration" };
var Da = /([\w\-]+)?(#([\w\-]+))?((\.([\w\-]+))*)/;
function Ho(s, t, e, ...i) {
  let r = Da.exec(t);
  if (!r)
    throw new Error("Bad use of emmet");
  let n = r[1] || "div", o;
  return s !== "http://www.w3.org/1999/xhtml" ? o = document.createElementNS(s, n) : o = document.createElement(n), r[3] && (o.id = r[3]), r[4] && (o.className = r[4].replace(/\./g, " ").trim()), e && Object.entries(e).forEach(([l, a]) => {
    typeof a > "u" || (/^on\w+$/.test(l) ? o[l] = a : l === "selected" ? a && o.setAttribute(l, "true") : o.setAttribute(l, a));
  }), o.append(...i), o;
}
function Ra(s, t, ...e) {
  return Ho("http://www.w3.org/1999/xhtml", s, t, ...e);
}
Ra.SVG = function(s, t, ...e) {
  return Ho("http://www.w3.org/2000/svg", s, t, ...e);
};
var ls = class {
  constructor(t) {
    this.domNode = t;
    this._maxWidth = "";
    this._width = "";
    this._height = "";
    this._top = "";
    this._left = "";
    this._bottom = "";
    this._right = "";
    this._paddingTop = "";
    this._paddingLeft = "";
    this._paddingBottom = "";
    this._paddingRight = "";
    this._fontFamily = "";
    this._fontWeight = "";
    this._fontSize = "";
    this._fontStyle = "";
    this._fontFeatureSettings = "";
    this._fontVariationSettings = "";
    this._textDecoration = "";
    this._lineHeight = "";
    this._letterSpacing = "";
    this._className = "";
    this._display = "";
    this._position = "";
    this._visibility = "";
    this._color = "";
    this._backgroundColor = "";
    this._layerHint = false;
    this._contain = "none";
    this._boxShadow = "";
  }
  setMaxWidth(t) {
    let e = Ie2(t);
    this._maxWidth !== e && (this._maxWidth = e, this.domNode.style.maxWidth = this._maxWidth);
  }
  setWidth(t) {
    let e = Ie2(t);
    this._width !== e && (this._width = e, this.domNode.style.width = this._width);
  }
  setHeight(t) {
    let e = Ie2(t);
    this._height !== e && (this._height = e, this.domNode.style.height = this._height);
  }
  setTop(t) {
    let e = Ie2(t);
    this._top !== e && (this._top = e, this.domNode.style.top = this._top);
  }
  setLeft(t) {
    let e = Ie2(t);
    this._left !== e && (this._left = e, this.domNode.style.left = this._left);
  }
  setBottom(t) {
    let e = Ie2(t);
    this._bottom !== e && (this._bottom = e, this.domNode.style.bottom = this._bottom);
  }
  setRight(t) {
    let e = Ie2(t);
    this._right !== e && (this._right = e, this.domNode.style.right = this._right);
  }
  setPaddingTop(t) {
    let e = Ie2(t);
    this._paddingTop !== e && (this._paddingTop = e, this.domNode.style.paddingTop = this._paddingTop);
  }
  setPaddingLeft(t) {
    let e = Ie2(t);
    this._paddingLeft !== e && (this._paddingLeft = e, this.domNode.style.paddingLeft = this._paddingLeft);
  }
  setPaddingBottom(t) {
    let e = Ie2(t);
    this._paddingBottom !== e && (this._paddingBottom = e, this.domNode.style.paddingBottom = this._paddingBottom);
  }
  setPaddingRight(t) {
    let e = Ie2(t);
    this._paddingRight !== e && (this._paddingRight = e, this.domNode.style.paddingRight = this._paddingRight);
  }
  setFontFamily(t) {
    this._fontFamily !== t && (this._fontFamily = t, this.domNode.style.fontFamily = this._fontFamily);
  }
  setFontWeight(t) {
    this._fontWeight !== t && (this._fontWeight = t, this.domNode.style.fontWeight = this._fontWeight);
  }
  setFontSize(t) {
    let e = Ie2(t);
    this._fontSize !== e && (this._fontSize = e, this.domNode.style.fontSize = this._fontSize);
  }
  setFontStyle(t) {
    this._fontStyle !== t && (this._fontStyle = t, this.domNode.style.fontStyle = this._fontStyle);
  }
  setFontFeatureSettings(t) {
    this._fontFeatureSettings !== t && (this._fontFeatureSettings = t, this.domNode.style.fontFeatureSettings = this._fontFeatureSettings);
  }
  setFontVariationSettings(t) {
    this._fontVariationSettings !== t && (this._fontVariationSettings = t, this.domNode.style.fontVariationSettings = this._fontVariationSettings);
  }
  setTextDecoration(t) {
    this._textDecoration !== t && (this._textDecoration = t, this.domNode.style.textDecoration = this._textDecoration);
  }
  setLineHeight(t) {
    let e = Ie2(t);
    this._lineHeight !== e && (this._lineHeight = e, this.domNode.style.lineHeight = this._lineHeight);
  }
  setLetterSpacing(t) {
    let e = Ie2(t);
    this._letterSpacing !== e && (this._letterSpacing = e, this.domNode.style.letterSpacing = this._letterSpacing);
  }
  setClassName(t) {
    this._className !== t && (this._className = t, this.domNode.className = this._className);
  }
  toggleClassName(t, e) {
    this.domNode.classList.toggle(t, e), this._className = this.domNode.className;
  }
  setDisplay(t) {
    this._display !== t && (this._display = t, this.domNode.style.display = this._display);
  }
  setPosition(t) {
    this._position !== t && (this._position = t, this.domNode.style.position = this._position);
  }
  setVisibility(t) {
    this._visibility !== t && (this._visibility = t, this.domNode.style.visibility = this._visibility);
  }
  setColor(t) {
    this._color !== t && (this._color = t, this.domNode.style.color = this._color);
  }
  setBackgroundColor(t) {
    this._backgroundColor !== t && (this._backgroundColor = t, this.domNode.style.backgroundColor = this._backgroundColor);
  }
  setLayerHinting(t) {
    this._layerHint !== t && (this._layerHint = t, this.domNode.style.transform = this._layerHint ? "translate3d(0px, 0px, 0px)" : "");
  }
  setBoxShadow(t) {
    this._boxShadow !== t && (this._boxShadow = t, this.domNode.style.boxShadow = t);
  }
  setContain(t) {
    this._contain !== t && (this._contain = t, this.domNode.style.contain = this._contain);
  }
  setAttribute(t, e) {
    this.domNode.setAttribute(t, e);
  }
  removeAttribute(t) {
    this.domNode.removeAttribute(t);
  }
  appendChild(t) {
    this.domNode.appendChild(t.domNode);
  }
  removeChild(t) {
    this.domNode.removeChild(t.domNode);
  }
};
function Ie2(s) {
  return typeof s == "number" ? `${s}px` : s;
}
function _t(s) {
  return new ls(s);
}
var Wt2 = class {
  constructor() {
    this._hooks = new Ee2;
    this._pointerMoveCallback = null;
    this._onStopCallback = null;
  }
  dispose() {
    this.stopMonitoring(false), this._hooks.dispose();
  }
  stopMonitoring(t, e) {
    if (!this.isMonitoring())
      return;
    this._hooks.clear(), this._pointerMoveCallback = null;
    let i = this._onStopCallback;
    this._onStopCallback = null, t && i && i(e);
  }
  isMonitoring() {
    return !!this._pointerMoveCallback;
  }
  startMonitoring(t, e, i, r, n) {
    this.isMonitoring() && this.stopMonitoring(false), this._pointerMoveCallback = r, this._onStopCallback = n;
    let o = t;
    try {
      t.setPointerCapture(e), this._hooks.add(C2(() => {
        try {
          t.releasePointerCapture(e);
        } catch {}
      }));
    } catch {
      o = be2(t);
    }
    this._hooks.add(L(o, Y2.POINTER_MOVE, (l) => {
      if (l.buttons !== i) {
        this.stopMonitoring(true);
        return;
      }
      l.preventDefault(), this._pointerMoveCallback(l);
    })), this._hooks.add(L(o, Y2.POINTER_UP, (l) => this.stopMonitoring(true)));
  }
};
function Wo(s, t, e) {
  let i = null, r = null;
  if (typeof e.value == "function" ? (i = "value", r = e.value, r.length !== 0 && console.warn("Memoize should only be used in functions with zero parameters")) : typeof e.get == "function" && (i = "get", r = e.get), !r)
    throw new Error("not supported");
  let n = `$memoize$${t}`;
  e[i] = function(...o) {
    return this.hasOwnProperty(n) || Object.defineProperty(this, n, { configurable: false, enumerable: false, writable: false, value: r.apply(this, o) }), this[n];
  };
}
var He2;
((n) => (n.Tap = "-xterm-gesturetap", n.Change = "-xterm-gesturechange", n.Start = "-xterm-gesturestart", n.End = "-xterm-gesturesend", n.Contextmenu = "-xterm-gesturecontextmenu"))(He2 ||= {});
var Q2 = class Q extends D2 {
  constructor() {
    super();
    this.dispatched = false;
    this.targets = new Ct2;
    this.ignoreTargets = new Ct2;
    this.activeTouches = {}, this.handle = null, this._lastSetTapCountTime = 0, this._register($.runAndSubscribe(No, ({ window: e, disposables: i }) => {
      i.add(L(e.document, "touchstart", (r) => this.onTouchStart(r), { passive: false })), i.add(L(e.document, "touchend", (r) => this.onTouchEnd(e, r))), i.add(L(e.document, "touchmove", (r) => this.onTouchMove(r), { passive: false }));
    }, { window: fe2, disposables: this._store }));
  }
  static addTarget(e) {
    if (!Q.isTouchDevice())
      return D2.None;
    Q.INSTANCE || (Q.INSTANCE = Gn2(new Q));
    let i = Q.INSTANCE.targets.push(e);
    return C2(i);
  }
  static ignoreTarget(e) {
    if (!Q.isTouchDevice())
      return D2.None;
    Q.INSTANCE || (Q.INSTANCE = Gn2(new Q));
    let i = Q.INSTANCE.ignoreTargets.push(e);
    return C2(i);
  }
  static isTouchDevice() {
    return "ontouchstart" in fe2 || navigator.maxTouchPoints > 0;
  }
  dispose() {
    this.handle && (this.handle.dispose(), this.handle = null), super.dispose();
  }
  onTouchStart(e) {
    let i = Date.now();
    this.handle && (this.handle.dispose(), this.handle = null);
    for (let r = 0, n = e.targetTouches.length;r < n; r++) {
      let o = e.targetTouches.item(r);
      this.activeTouches[o.identifier] = { id: o.identifier, initialTarget: o.target, initialTimeStamp: i, initialPageX: o.pageX, initialPageY: o.pageY, rollingTimestamps: [i], rollingPageX: [o.pageX], rollingPageY: [o.pageY] };
      let l = this.newGestureEvent(He2.Start, o.target);
      l.pageX = o.pageX, l.pageY = o.pageY, this.dispatchEvent(l);
    }
    this.dispatched && (e.preventDefault(), e.stopPropagation(), this.dispatched = false);
  }
  onTouchEnd(e, i) {
    let r = Date.now(), n = Object.keys(this.activeTouches).length;
    for (let o = 0, l = i.changedTouches.length;o < l; o++) {
      let a = i.changedTouches.item(o);
      if (!this.activeTouches.hasOwnProperty(String(a.identifier))) {
        console.warn("move of an UNKNOWN touch", a);
        continue;
      }
      let u = this.activeTouches[a.identifier], h = Date.now() - u.initialTimeStamp;
      if (h < Q.HOLD_DELAY && Math.abs(u.initialPageX - Se(u.rollingPageX)) < 30 && Math.abs(u.initialPageY - Se(u.rollingPageY)) < 30) {
        let c = this.newGestureEvent(He2.Tap, u.initialTarget);
        c.pageX = Se(u.rollingPageX), c.pageY = Se(u.rollingPageY), this.dispatchEvent(c);
      } else if (h >= Q.HOLD_DELAY && Math.abs(u.initialPageX - Se(u.rollingPageX)) < 30 && Math.abs(u.initialPageY - Se(u.rollingPageY)) < 30) {
        let c = this.newGestureEvent(He2.Contextmenu, u.initialTarget);
        c.pageX = Se(u.rollingPageX), c.pageY = Se(u.rollingPageY), this.dispatchEvent(c);
      } else if (n === 1) {
        let c = Se(u.rollingPageX), d = Se(u.rollingPageY), _ = Se(u.rollingTimestamps) - u.rollingTimestamps[0], p = c - u.rollingPageX[0], m = d - u.rollingPageY[0], f = [...this.targets].filter((A) => u.initialTarget instanceof Node && A.contains(u.initialTarget));
        this.inertia(e, f, r, Math.abs(p) / _, p > 0 ? 1 : -1, c, Math.abs(m) / _, m > 0 ? 1 : -1, d);
      }
      this.dispatchEvent(this.newGestureEvent(He2.End, u.initialTarget)), delete this.activeTouches[a.identifier];
    }
    this.dispatched && (i.preventDefault(), i.stopPropagation(), this.dispatched = false);
  }
  newGestureEvent(e, i) {
    let r = document.createEvent("CustomEvent");
    return r.initEvent(e, false, true), r.initialTarget = i, r.tapCount = 0, r;
  }
  dispatchEvent(e) {
    if (e.type === He2.Tap) {
      let i = new Date().getTime(), r = 0;
      i - this._lastSetTapCountTime > Q.CLEAR_TAP_COUNT_TIME ? r = 1 : r = 2, this._lastSetTapCountTime = i, e.tapCount = r;
    } else
      (e.type === He2.Change || e.type === He2.Contextmenu) && (this._lastSetTapCountTime = 0);
    if (e.initialTarget instanceof Node) {
      for (let r of this.ignoreTargets)
        if (r.contains(e.initialTarget))
          return;
      let i = [];
      for (let r of this.targets)
        if (r.contains(e.initialTarget)) {
          let n = 0, o = e.initialTarget;
          for (;o && o !== r; )
            n++, o = o.parentElement;
          i.push([n, r]);
        }
      i.sort((r, n) => r[0] - n[0]);
      for (let [r, n] of i)
        n.dispatchEvent(e), this.dispatched = true;
    }
  }
  inertia(e, i, r, n, o, l, a, u, h) {
    this.handle = mt(e, () => {
      let c = Date.now(), d = c - r, _ = 0, p = 0, m = true;
      n += Q.SCROLL_FRICTION * d, a += Q.SCROLL_FRICTION * d, n > 0 && (m = false, _ = o * n * d), a > 0 && (m = false, p = u * a * d);
      let f = this.newGestureEvent(He2.Change);
      f.translationX = _, f.translationY = p, i.forEach((A) => A.dispatchEvent(f)), m || this.inertia(e, i, c, n, o, l + _, a, u, h + p);
    });
  }
  onTouchMove(e) {
    let i = Date.now();
    for (let r = 0, n = e.changedTouches.length;r < n; r++) {
      let o = e.changedTouches.item(r);
      if (!this.activeTouches.hasOwnProperty(String(o.identifier))) {
        console.warn("end of an UNKNOWN touch", o);
        continue;
      }
      let l = this.activeTouches[o.identifier], a = this.newGestureEvent(He2.Change, l.initialTarget);
      a.translationX = o.pageX - Se(l.rollingPageX), a.translationY = o.pageY - Se(l.rollingPageY), a.pageX = o.pageX, a.pageY = o.pageY, this.dispatchEvent(a), l.rollingPageX.length > 3 && (l.rollingPageX.shift(), l.rollingPageY.shift(), l.rollingTimestamps.shift()), l.rollingPageX.push(o.pageX), l.rollingPageY.push(o.pageY), l.rollingTimestamps.push(i);
    }
    this.dispatched && (e.preventDefault(), e.stopPropagation(), this.dispatched = false);
  }
};
Q2.SCROLL_FRICTION = -0.005, Q2.HOLD_DELAY = 700, Q2.CLEAR_TAP_COUNT_TIME = 400, M([Wo], Q2, "isTouchDevice", 1);
var Pr2 = Q2;
var lt2 = class extends D2 {
  onclick(t, e) {
    this._register(L(t, Y2.CLICK, (i) => e(new qe2(be2(t), i))));
  }
  onmousedown(t, e) {
    this._register(L(t, Y2.MOUSE_DOWN, (i) => e(new qe2(be2(t), i))));
  }
  onmouseover(t, e) {
    this._register(L(t, Y2.MOUSE_OVER, (i) => e(new qe2(be2(t), i))));
  }
  onmouseleave(t, e) {
    this._register(L(t, Y2.MOUSE_LEAVE, (i) => e(new qe2(be2(t), i))));
  }
  onkeydown(t, e) {
    this._register(L(t, Y2.KEY_DOWN, (i) => e(new ft(i))));
  }
  onkeyup(t, e) {
    this._register(L(t, Y2.KEY_UP, (i) => e(new ft(i))));
  }
  oninput(t, e) {
    this._register(L(t, Y2.INPUT, e));
  }
  onblur(t, e) {
    this._register(L(t, Y2.BLUR, e));
  }
  onfocus(t, e) {
    this._register(L(t, Y2.FOCUS, e));
  }
  onchange(t, e) {
    this._register(L(t, Y2.CHANGE, e));
  }
  ignoreGesture(t) {
    return Pr2.ignoreTarget(t);
  }
};
var Uo = 11;
var Or2 = class extends lt2 {
  constructor(t) {
    super(), this._onActivate = t.onActivate, this.bgDomNode = document.createElement("div"), this.bgDomNode.className = "arrow-background", this.bgDomNode.style.position = "absolute", this.bgDomNode.style.width = t.bgWidth + "px", this.bgDomNode.style.height = t.bgHeight + "px", typeof t.top < "u" && (this.bgDomNode.style.top = "0px"), typeof t.left < "u" && (this.bgDomNode.style.left = "0px"), typeof t.bottom < "u" && (this.bgDomNode.style.bottom = "0px"), typeof t.right < "u" && (this.bgDomNode.style.right = "0px"), this.domNode = document.createElement("div"), this.domNode.className = t.className, this.domNode.style.position = "absolute", this.domNode.style.width = Uo + "px", this.domNode.style.height = Uo + "px", typeof t.top < "u" && (this.domNode.style.top = t.top + "px"), typeof t.left < "u" && (this.domNode.style.left = t.left + "px"), typeof t.bottom < "u" && (this.domNode.style.bottom = t.bottom + "px"), typeof t.right < "u" && (this.domNode.style.right = t.right + "px"), this._pointerMoveMonitor = this._register(new Wt2), this._register(os2(this.bgDomNode, Y2.POINTER_DOWN, (e) => this._arrowPointerDown(e))), this._register(os2(this.domNode, Y2.POINTER_DOWN, (e) => this._arrowPointerDown(e))), this._pointerdownRepeatTimer = this._register(new Mr2), this._pointerdownScheduleRepeatTimer = this._register(new Ye2);
  }
  _arrowPointerDown(t) {
    if (!t.target || !(t.target instanceof Element))
      return;
    let e = () => {
      this._pointerdownRepeatTimer.cancelAndSet(() => this._onActivate(), 1000 / 24, be2(t));
    };
    this._onActivate(), this._pointerdownRepeatTimer.cancel(), this._pointerdownScheduleRepeatTimer.cancelAndSet(e, 200), this._pointerMoveMonitor.startMonitoring(t.target, t.pointerId, t.buttons, (i) => {}, () => {
      this._pointerdownRepeatTimer.cancel(), this._pointerdownScheduleRepeatTimer.cancel();
    }), t.preventDefault();
  }
};
var cs2 = class s {
  constructor(t, e, i, r, n, o, l) {
    this._forceIntegerValues = t;
    this._scrollStateBrand = undefined;
    this._forceIntegerValues && (e = e | 0, i = i | 0, r = r | 0, n = n | 0, o = o | 0, l = l | 0), this.rawScrollLeft = r, this.rawScrollTop = l, e < 0 && (e = 0), r + e > i && (r = i - e), r < 0 && (r = 0), n < 0 && (n = 0), l + n > o && (l = o - n), l < 0 && (l = 0), this.width = e, this.scrollWidth = i, this.scrollLeft = r, this.height = n, this.scrollHeight = o, this.scrollTop = l;
  }
  equals(t) {
    return this.rawScrollLeft === t.rawScrollLeft && this.rawScrollTop === t.rawScrollTop && this.width === t.width && this.scrollWidth === t.scrollWidth && this.scrollLeft === t.scrollLeft && this.height === t.height && this.scrollHeight === t.scrollHeight && this.scrollTop === t.scrollTop;
  }
  withScrollDimensions(t, e) {
    return new s(this._forceIntegerValues, typeof t.width < "u" ? t.width : this.width, typeof t.scrollWidth < "u" ? t.scrollWidth : this.scrollWidth, e ? this.rawScrollLeft : this.scrollLeft, typeof t.height < "u" ? t.height : this.height, typeof t.scrollHeight < "u" ? t.scrollHeight : this.scrollHeight, e ? this.rawScrollTop : this.scrollTop);
  }
  withScrollPosition(t) {
    return new s(this._forceIntegerValues, this.width, this.scrollWidth, typeof t.scrollLeft < "u" ? t.scrollLeft : this.rawScrollLeft, this.height, this.scrollHeight, typeof t.scrollTop < "u" ? t.scrollTop : this.rawScrollTop);
  }
  createScrollEvent(t, e) {
    let i = this.width !== t.width, r = this.scrollWidth !== t.scrollWidth, n = this.scrollLeft !== t.scrollLeft, o = this.height !== t.height, l = this.scrollHeight !== t.scrollHeight, a = this.scrollTop !== t.scrollTop;
    return { inSmoothScrolling: e, oldWidth: t.width, oldScrollWidth: t.scrollWidth, oldScrollLeft: t.scrollLeft, width: this.width, scrollWidth: this.scrollWidth, scrollLeft: this.scrollLeft, oldHeight: t.height, oldScrollHeight: t.scrollHeight, oldScrollTop: t.scrollTop, height: this.height, scrollHeight: this.scrollHeight, scrollTop: this.scrollTop, widthChanged: i, scrollWidthChanged: r, scrollLeftChanged: n, heightChanged: o, scrollHeightChanged: l, scrollTopChanged: a };
  }
};
var Ri = class extends D2 {
  constructor(e) {
    super();
    this._scrollableBrand = undefined;
    this._onScroll = this._register(new v);
    this.onScroll = this._onScroll.event;
    this._smoothScrollDuration = e.smoothScrollDuration, this._scheduleAtNextAnimationFrame = e.scheduleAtNextAnimationFrame, this._state = new cs2(e.forceIntegerValues, 0, 0, 0, 0, 0, 0), this._smoothScrolling = null;
  }
  dispose() {
    this._smoothScrolling && (this._smoothScrolling.dispose(), this._smoothScrolling = null), super.dispose();
  }
  setSmoothScrollDuration(e) {
    this._smoothScrollDuration = e;
  }
  validateScrollPosition(e) {
    return this._state.withScrollPosition(e);
  }
  getScrollDimensions() {
    return this._state;
  }
  setScrollDimensions(e, i) {
    let r = this._state.withScrollDimensions(e, i);
    this._setState(r, !!this._smoothScrolling), this._smoothScrolling?.acceptScrollDimensions(this._state);
  }
  getFutureScrollPosition() {
    return this._smoothScrolling ? this._smoothScrolling.to : this._state;
  }
  getCurrentScrollPosition() {
    return this._state;
  }
  setScrollPositionNow(e) {
    let i = this._state.withScrollPosition(e);
    this._smoothScrolling && (this._smoothScrolling.dispose(), this._smoothScrolling = null), this._setState(i, false);
  }
  setScrollPositionSmooth(e, i) {
    if (this._smoothScrollDuration === 0)
      return this.setScrollPositionNow(e);
    if (this._smoothScrolling) {
      e = { scrollLeft: typeof e.scrollLeft > "u" ? this._smoothScrolling.to.scrollLeft : e.scrollLeft, scrollTop: typeof e.scrollTop > "u" ? this._smoothScrolling.to.scrollTop : e.scrollTop };
      let r = this._state.withScrollPosition(e);
      if (this._smoothScrolling.to.scrollLeft === r.scrollLeft && this._smoothScrolling.to.scrollTop === r.scrollTop)
        return;
      let n;
      i ? n = new Nr(this._smoothScrolling.from, r, this._smoothScrolling.startTime, this._smoothScrolling.duration) : n = this._smoothScrolling.combine(this._state, r, this._smoothScrollDuration), this._smoothScrolling.dispose(), this._smoothScrolling = n;
    } else {
      let r = this._state.withScrollPosition(e);
      this._smoothScrolling = Nr.start(this._state, r, this._smoothScrollDuration);
    }
    this._smoothScrolling.animationFrameDisposable = this._scheduleAtNextAnimationFrame(() => {
      this._smoothScrolling && (this._smoothScrolling.animationFrameDisposable = null, this._performSmoothScrolling());
    });
  }
  hasPendingScrollAnimation() {
    return !!this._smoothScrolling;
  }
  _performSmoothScrolling() {
    if (!this._smoothScrolling)
      return;
    let e = this._smoothScrolling.tick(), i = this._state.withScrollPosition(e);
    if (this._setState(i, true), !!this._smoothScrolling) {
      if (e.isDone) {
        this._smoothScrolling.dispose(), this._smoothScrolling = null;
        return;
      }
      this._smoothScrolling.animationFrameDisposable = this._scheduleAtNextAnimationFrame(() => {
        this._smoothScrolling && (this._smoothScrolling.animationFrameDisposable = null, this._performSmoothScrolling());
      });
    }
  }
  _setState(e, i) {
    let r = this._state;
    r.equals(e) || (this._state = e, this._onScroll.fire(this._state.createScrollEvent(r, i)));
  }
};
var Br2 = class {
  constructor(t, e, i) {
    this.scrollLeft = t, this.scrollTop = e, this.isDone = i;
  }
};
function as2(s, t) {
  let e = t - s;
  return function(i) {
    return s + e * ka(i);
  };
}
function La(s, t, e) {
  return function(i) {
    return i < e ? s(i / e) : t((i - e) / (1 - e));
  };
}
var Nr = class s {
  constructor(t, e, i, r) {
    this.from = t, this.to = e, this.duration = r, this.startTime = i, this.animationFrameDisposable = null, this._initAnimations();
  }
  _initAnimations() {
    this.scrollLeft = this._initAnimation(this.from.scrollLeft, this.to.scrollLeft, this.to.width), this.scrollTop = this._initAnimation(this.from.scrollTop, this.to.scrollTop, this.to.height);
  }
  _initAnimation(t, e, i) {
    if (Math.abs(t - e) > 2.5 * i) {
      let n, o;
      return t < e ? (n = t + 0.75 * i, o = e - 0.75 * i) : (n = t - 0.75 * i, o = e + 0.75 * i), La(as2(t, n), as2(o, e), 0.33);
    }
    return as2(t, e);
  }
  dispose() {
    this.animationFrameDisposable !== null && (this.animationFrameDisposable.dispose(), this.animationFrameDisposable = null);
  }
  acceptScrollDimensions(t) {
    this.to = t.withScrollPosition(this.to), this._initAnimations();
  }
  tick() {
    return this._tick(Date.now());
  }
  _tick(t) {
    let e = (t - this.startTime) / this.duration;
    if (e < 1) {
      let i = this.scrollLeft(e), r = this.scrollTop(e);
      return new Br2(i, r, false);
    }
    return new Br2(this.to.scrollLeft, this.to.scrollTop, true);
  }
  combine(t, e, i) {
    return s.start(t, e, i);
  }
  static start(t, e, i) {
    i = i + 10;
    let r = Date.now() - 10;
    return new s(t, e, r, i);
  }
};
function Aa(s) {
  return Math.pow(s, 3);
}
function ka(s) {
  return 1 - Aa(1 - s);
}
var Fr2 = class extends D2 {
  constructor(t, e, i) {
    super(), this._visibility = t, this._visibleClassName = e, this._invisibleClassName = i, this._domNode = null, this._isVisible = false, this._isNeeded = false, this._rawShouldBeVisible = false, this._shouldBeVisible = false, this._revealTimer = this._register(new Ye2);
  }
  setVisibility(t) {
    this._visibility !== t && (this._visibility = t, this._updateShouldBeVisible());
  }
  setShouldBeVisible(t) {
    this._rawShouldBeVisible = t, this._updateShouldBeVisible();
  }
  _applyVisibilitySetting() {
    return this._visibility === 2 ? false : this._visibility === 3 ? true : this._rawShouldBeVisible;
  }
  _updateShouldBeVisible() {
    let t = this._applyVisibilitySetting();
    this._shouldBeVisible !== t && (this._shouldBeVisible = t, this.ensureVisibility());
  }
  setIsNeeded(t) {
    this._isNeeded !== t && (this._isNeeded = t, this.ensureVisibility());
  }
  setDomNode(t) {
    this._domNode = t, this._domNode.setClassName(this._invisibleClassName), this.setShouldBeVisible(false);
  }
  ensureVisibility() {
    if (!this._isNeeded) {
      this._hide(false);
      return;
    }
    this._shouldBeVisible ? this._reveal() : this._hide(true);
  }
  _reveal() {
    this._isVisible || (this._isVisible = true, this._revealTimer.setIfNotSet(() => {
      this._domNode?.setClassName(this._visibleClassName);
    }, 0));
  }
  _hide(t) {
    this._revealTimer.cancel(), this._isVisible && (this._isVisible = false, this._domNode?.setClassName(this._invisibleClassName + (t ? " fade" : "")));
  }
};
var Ca2 = 140;
var Ut2 = class extends lt2 {
  constructor(t) {
    super(), this._lazyRender = t.lazyRender, this._host = t.host, this._scrollable = t.scrollable, this._scrollByPage = t.scrollByPage, this._scrollbarState = t.scrollbarState, this._visibilityController = this._register(new Fr2(t.visibility, "visible scrollbar " + t.extraScrollbarClassName, "invisible scrollbar " + t.extraScrollbarClassName)), this._visibilityController.setIsNeeded(this._scrollbarState.isNeeded()), this._pointerMoveMonitor = this._register(new Wt2), this._shouldRender = true, this.domNode = _t(document.createElement("div")), this.domNode.setAttribute("role", "presentation"), this.domNode.setAttribute("aria-hidden", "true"), this._visibilityController.setDomNode(this.domNode), this.domNode.setPosition("absolute"), this._register(L(this.domNode.domNode, Y2.POINTER_DOWN, (e) => this._domNodePointerDown(e)));
  }
  _createArrow(t) {
    let e = this._register(new Or2(t));
    this.domNode.domNode.appendChild(e.bgDomNode), this.domNode.domNode.appendChild(e.domNode);
  }
  _createSlider(t, e, i, r) {
    this.slider = _t(document.createElement("div")), this.slider.setClassName("slider"), this.slider.setPosition("absolute"), this.slider.setTop(t), this.slider.setLeft(e), typeof i == "number" && this.slider.setWidth(i), typeof r == "number" && this.slider.setHeight(r), this.slider.setLayerHinting(true), this.slider.setContain("strict"), this.domNode.domNode.appendChild(this.slider.domNode), this._register(L(this.slider.domNode, Y2.POINTER_DOWN, (n) => {
      n.button === 0 && (n.preventDefault(), this._sliderPointerDown(n));
    })), this.onclick(this.slider.domNode, (n) => {
      n.leftButton && n.stopPropagation();
    });
  }
  _onElementSize(t) {
    return this._scrollbarState.setVisibleSize(t) && (this._visibilityController.setIsNeeded(this._scrollbarState.isNeeded()), this._shouldRender = true, this._lazyRender || this.render()), this._shouldRender;
  }
  _onElementScrollSize(t) {
    return this._scrollbarState.setScrollSize(t) && (this._visibilityController.setIsNeeded(this._scrollbarState.isNeeded()), this._shouldRender = true, this._lazyRender || this.render()), this._shouldRender;
  }
  _onElementScrollPosition(t) {
    return this._scrollbarState.setScrollPosition(t) && (this._visibilityController.setIsNeeded(this._scrollbarState.isNeeded()), this._shouldRender = true, this._lazyRender || this.render()), this._shouldRender;
  }
  beginReveal() {
    this._visibilityController.setShouldBeVisible(true);
  }
  beginHide() {
    this._visibilityController.setShouldBeVisible(false);
  }
  render() {
    this._shouldRender && (this._shouldRender = false, this._renderDomNode(this._scrollbarState.getRectangleLargeSize(), this._scrollbarState.getRectangleSmallSize()), this._updateSlider(this._scrollbarState.getSliderSize(), this._scrollbarState.getArrowSize() + this._scrollbarState.getSliderPosition()));
  }
  _domNodePointerDown(t) {
    t.target === this.domNode.domNode && this._onPointerDown(t);
  }
  delegatePointerDown(t) {
    let e = this.domNode.domNode.getClientRects()[0].top, i = e + this._scrollbarState.getSliderPosition(), r = e + this._scrollbarState.getSliderPosition() + this._scrollbarState.getSliderSize(), n = this._sliderPointerPosition(t);
    i <= n && n <= r ? t.button === 0 && (t.preventDefault(), this._sliderPointerDown(t)) : this._onPointerDown(t);
  }
  _onPointerDown(t) {
    let e, i;
    if (t.target === this.domNode.domNode && typeof t.offsetX == "number" && typeof t.offsetY == "number")
      e = t.offsetX, i = t.offsetY;
    else {
      let n = Fo(this.domNode.domNode);
      e = t.pageX - n.left, i = t.pageY - n.top;
    }
    let r = this._pointerDownRelativePosition(e, i);
    this._setDesiredScrollPositionNow(this._scrollByPage ? this._scrollbarState.getDesiredScrollPositionFromOffsetPaged(r) : this._scrollbarState.getDesiredScrollPositionFromOffset(r)), t.button === 0 && (t.preventDefault(), this._sliderPointerDown(t));
  }
  _sliderPointerDown(t) {
    if (!t.target || !(t.target instanceof Element))
      return;
    let e = this._sliderPointerPosition(t), i = this._sliderOrthogonalPointerPosition(t), r = this._scrollbarState.clone();
    this.slider.toggleClassName("active", true), this._pointerMoveMonitor.startMonitoring(t.target, t.pointerId, t.buttons, (n) => {
      let o = this._sliderOrthogonalPointerPosition(n), l = Math.abs(o - i);
      if (wr2 && l > Ca2) {
        this._setDesiredScrollPositionNow(r.getScrollPosition());
        return;
      }
      let u = this._sliderPointerPosition(n) - e;
      this._setDesiredScrollPositionNow(r.getDesiredScrollPositionFromDelta(u));
    }, () => {
      this.slider.toggleClassName("active", false), this._host.onDragEnd();
    }), this._host.onDragStart();
  }
  _setDesiredScrollPositionNow(t) {
    let e = {};
    this.writeScrollPosition(e, t), this._scrollable.setScrollPositionNow(e);
  }
  updateScrollbarSize(t) {
    this._updateScrollbarSize(t), this._scrollbarState.setScrollbarSize(t), this._shouldRender = true, this._lazyRender || this.render();
  }
  isNeeded() {
    return this._scrollbarState.isNeeded();
  }
};
var Kt2 = class s {
  constructor(t, e, i, r, n, o) {
    this._scrollbarSize = Math.round(e), this._oppositeScrollbarSize = Math.round(i), this._arrowSize = Math.round(t), this._visibleSize = r, this._scrollSize = n, this._scrollPosition = o, this._computedAvailableSize = 0, this._computedIsNeeded = false, this._computedSliderSize = 0, this._computedSliderRatio = 0, this._computedSliderPosition = 0, this._refreshComputedValues();
  }
  clone() {
    return new s(this._arrowSize, this._scrollbarSize, this._oppositeScrollbarSize, this._visibleSize, this._scrollSize, this._scrollPosition);
  }
  setVisibleSize(t) {
    let e = Math.round(t);
    return this._visibleSize !== e ? (this._visibleSize = e, this._refreshComputedValues(), true) : false;
  }
  setScrollSize(t) {
    let e = Math.round(t);
    return this._scrollSize !== e ? (this._scrollSize = e, this._refreshComputedValues(), true) : false;
  }
  setScrollPosition(t) {
    let e = Math.round(t);
    return this._scrollPosition !== e ? (this._scrollPosition = e, this._refreshComputedValues(), true) : false;
  }
  setScrollbarSize(t) {
    this._scrollbarSize = Math.round(t);
  }
  setOppositeScrollbarSize(t) {
    this._oppositeScrollbarSize = Math.round(t);
  }
  static _computeValues(t, e, i, r, n) {
    let o = Math.max(0, i - t), l = Math.max(0, o - 2 * e), a = r > 0 && r > i;
    if (!a)
      return { computedAvailableSize: Math.round(o), computedIsNeeded: a, computedSliderSize: Math.round(l), computedSliderRatio: 0, computedSliderPosition: 0 };
    let u = Math.round(Math.max(20, Math.floor(i * l / r))), h = (l - u) / (r - i), c = n * h;
    return { computedAvailableSize: Math.round(o), computedIsNeeded: a, computedSliderSize: Math.round(u), computedSliderRatio: h, computedSliderPosition: Math.round(c) };
  }
  _refreshComputedValues() {
    let t = s._computeValues(this._oppositeScrollbarSize, this._arrowSize, this._visibleSize, this._scrollSize, this._scrollPosition);
    this._computedAvailableSize = t.computedAvailableSize, this._computedIsNeeded = t.computedIsNeeded, this._computedSliderSize = t.computedSliderSize, this._computedSliderRatio = t.computedSliderRatio, this._computedSliderPosition = t.computedSliderPosition;
  }
  getArrowSize() {
    return this._arrowSize;
  }
  getScrollPosition() {
    return this._scrollPosition;
  }
  getRectangleLargeSize() {
    return this._computedAvailableSize;
  }
  getRectangleSmallSize() {
    return this._scrollbarSize;
  }
  isNeeded() {
    return this._computedIsNeeded;
  }
  getSliderSize() {
    return this._computedSliderSize;
  }
  getSliderPosition() {
    return this._computedSliderPosition;
  }
  getDesiredScrollPositionFromOffset(t) {
    if (!this._computedIsNeeded)
      return 0;
    let e = t - this._arrowSize - this._computedSliderSize / 2;
    return Math.round(e / this._computedSliderRatio);
  }
  getDesiredScrollPositionFromOffsetPaged(t) {
    if (!this._computedIsNeeded)
      return 0;
    let e = t - this._arrowSize, i = this._scrollPosition;
    return e < this._computedSliderPosition ? i -= this._visibleSize : i += this._visibleSize, i;
  }
  getDesiredScrollPositionFromDelta(t) {
    if (!this._computedIsNeeded)
      return 0;
    let e = this._computedSliderPosition + t;
    return Math.round(e / this._computedSliderRatio);
  }
};
var Wr2 = class extends Ut2 {
  constructor(t, e, i) {
    let r = t.getScrollDimensions(), n = t.getCurrentScrollPosition();
    if (super({ lazyRender: e.lazyRender, host: i, scrollbarState: new Kt2(e.horizontalHasArrows ? e.arrowSize : 0, e.horizontal === 2 ? 0 : e.horizontalScrollbarSize, e.vertical === 2 ? 0 : e.verticalScrollbarSize, r.width, r.scrollWidth, n.scrollLeft), visibility: e.horizontal, extraScrollbarClassName: "horizontal", scrollable: t, scrollByPage: e.scrollByPage }), e.horizontalHasArrows)
      throw new Error("horizontalHasArrows is not supported in xterm.js");
    this._createSlider(Math.floor((e.horizontalScrollbarSize - e.horizontalSliderSize) / 2), 0, undefined, e.horizontalSliderSize);
  }
  _updateSlider(t, e) {
    this.slider.setWidth(t), this.slider.setLeft(e);
  }
  _renderDomNode(t, e) {
    this.domNode.setWidth(t), this.domNode.setHeight(e), this.domNode.setLeft(0), this.domNode.setBottom(0);
  }
  onDidScroll(t) {
    return this._shouldRender = this._onElementScrollSize(t.scrollWidth) || this._shouldRender, this._shouldRender = this._onElementScrollPosition(t.scrollLeft) || this._shouldRender, this._shouldRender = this._onElementSize(t.width) || this._shouldRender, this._shouldRender;
  }
  _pointerDownRelativePosition(t, e) {
    return t;
  }
  _sliderPointerPosition(t) {
    return t.pageX;
  }
  _sliderOrthogonalPointerPosition(t) {
    return t.pageY;
  }
  _updateScrollbarSize(t) {
    this.slider.setHeight(t);
  }
  writeScrollPosition(t, e) {
    t.scrollLeft = e;
  }
  updateOptions(t) {
    this.updateScrollbarSize(t.horizontal === 2 ? 0 : t.horizontalScrollbarSize), this._scrollbarState.setOppositeScrollbarSize(t.vertical === 2 ? 0 : t.verticalScrollbarSize), this._visibilityController.setVisibility(t.horizontal), this._scrollByPage = t.scrollByPage;
  }
};
var Ur = class extends Ut2 {
  constructor(t, e, i) {
    let r = t.getScrollDimensions(), n = t.getCurrentScrollPosition();
    if (super({ lazyRender: e.lazyRender, host: i, scrollbarState: new Kt2(e.verticalHasArrows ? e.arrowSize : 0, e.vertical === 2 ? 0 : e.verticalScrollbarSize, 0, r.height, r.scrollHeight, n.scrollTop), visibility: e.vertical, extraScrollbarClassName: "vertical", scrollable: t, scrollByPage: e.scrollByPage }), e.verticalHasArrows)
      throw new Error("horizontalHasArrows is not supported in xterm.js");
    this._createSlider(0, Math.floor((e.verticalScrollbarSize - e.verticalSliderSize) / 2), e.verticalSliderSize, undefined);
  }
  _updateSlider(t, e) {
    this.slider.setHeight(t), this.slider.setTop(e);
  }
  _renderDomNode(t, e) {
    this.domNode.setWidth(e), this.domNode.setHeight(t), this.domNode.setRight(0), this.domNode.setTop(0);
  }
  onDidScroll(t) {
    return this._shouldRender = this._onElementScrollSize(t.scrollHeight) || this._shouldRender, this._shouldRender = this._onElementScrollPosition(t.scrollTop) || this._shouldRender, this._shouldRender = this._onElementSize(t.height) || this._shouldRender, this._shouldRender;
  }
  _pointerDownRelativePosition(t, e) {
    return e;
  }
  _sliderPointerPosition(t) {
    return t.pageY;
  }
  _sliderOrthogonalPointerPosition(t) {
    return t.pageX;
  }
  _updateScrollbarSize(t) {
    this.slider.setWidth(t);
  }
  writeScrollPosition(t, e) {
    t.scrollTop = e;
  }
  updateOptions(t) {
    this.updateScrollbarSize(t.vertical === 2 ? 0 : t.verticalScrollbarSize), this._scrollbarState.setOppositeScrollbarSize(0), this._visibilityController.setVisibility(t.vertical), this._scrollByPage = t.scrollByPage;
  }
};
var Ma = 500;
var Ko = 50;
var zo = true;
var us2 = class {
  constructor(t, e, i) {
    this.timestamp = t, this.deltaX = e, this.deltaY = i, this.score = 0;
  }
};
var zr2 = class zr {
  constructor() {
    this._capacity = 5, this._memory = [], this._front = -1, this._rear = -1;
  }
  isPhysicalMouseWheel() {
    if (this._front === -1 && this._rear === -1)
      return false;
    let t = 1, e = 0, i = 1, r = this._rear;
    do {
      let n = r === this._front ? t : Math.pow(2, -i);
      if (t -= n, e += this._memory[r].score * n, r === this._front)
        break;
      r = (this._capacity + r - 1) % this._capacity, i++;
    } while (true);
    return e <= 0.5;
  }
  acceptStandardWheelEvent(t) {
    if (Ti2) {
      let e = be2(t.browserEvent), i = mo(e);
      this.accept(Date.now(), t.deltaX * i, t.deltaY * i);
    } else
      this.accept(Date.now(), t.deltaX, t.deltaY);
  }
  accept(t, e, i) {
    let r = null, n = new us2(t, e, i);
    this._front === -1 && this._rear === -1 ? (this._memory[0] = n, this._front = 0, this._rear = 0) : (r = this._memory[this._rear], this._rear = (this._rear + 1) % this._capacity, this._rear === this._front && (this._front = (this._front + 1) % this._capacity), this._memory[this._rear] = n), n.score = this._computeScore(n, r);
  }
  _computeScore(t, e) {
    if (Math.abs(t.deltaX) > 0 && Math.abs(t.deltaY) > 0)
      return 1;
    let i = 0.5;
    if ((!this._isAlmostInt(t.deltaX) || !this._isAlmostInt(t.deltaY)) && (i += 0.25), e) {
      let r = Math.abs(t.deltaX), n = Math.abs(t.deltaY), o = Math.abs(e.deltaX), l = Math.abs(e.deltaY), a = Math.max(Math.min(r, o), 1), u = Math.max(Math.min(n, l), 1), h = Math.max(r, o), c = Math.max(n, l);
      h % a === 0 && c % u === 0 && (i -= 0.5);
    }
    return Math.min(Math.max(i, 0), 1);
  }
  _isAlmostInt(t) {
    return Math.abs(Math.round(t) - t) < 0.01;
  }
};
zr2.INSTANCE = new zr2;
var hs2 = zr2;
var ds2 = class extends lt2 {
  constructor(e, i, r) {
    super();
    this._onScroll = this._register(new v);
    this.onScroll = this._onScroll.event;
    this._onWillScroll = this._register(new v);
    this.onWillScroll = this._onWillScroll.event;
    this._options = Pa(i), this._scrollable = r, this._register(this._scrollable.onScroll((o) => {
      this._onWillScroll.fire(o), this._onDidScroll(o), this._onScroll.fire(o);
    }));
    let n = { onMouseWheel: (o) => this._onMouseWheel(o), onDragStart: () => this._onDragStart(), onDragEnd: () => this._onDragEnd() };
    this._verticalScrollbar = this._register(new Ur(this._scrollable, this._options, n)), this._horizontalScrollbar = this._register(new Wr2(this._scrollable, this._options, n)), this._domNode = document.createElement("div"), this._domNode.className = "xterm-scrollable-element " + this._options.className, this._domNode.setAttribute("role", "presentation"), this._domNode.style.position = "relative", this._domNode.appendChild(e), this._domNode.appendChild(this._horizontalScrollbar.domNode.domNode), this._domNode.appendChild(this._verticalScrollbar.domNode.domNode), this._options.useShadows ? (this._leftShadowDomNode = _t(document.createElement("div")), this._leftShadowDomNode.setClassName("shadow"), this._domNode.appendChild(this._leftShadowDomNode.domNode), this._topShadowDomNode = _t(document.createElement("div")), this._topShadowDomNode.setClassName("shadow"), this._domNode.appendChild(this._topShadowDomNode.domNode), this._topLeftShadowDomNode = _t(document.createElement("div")), this._topLeftShadowDomNode.setClassName("shadow"), this._domNode.appendChild(this._topLeftShadowDomNode.domNode)) : (this._leftShadowDomNode = null, this._topShadowDomNode = null, this._topLeftShadowDomNode = null), this._listenOnDomNode = this._options.listenOnDomNode || this._domNode, this._mouseWheelToDispose = [], this._setListeningToMouseWheel(this._options.handleMouseWheel), this.onmouseover(this._listenOnDomNode, (o) => this._onMouseOver(o)), this.onmouseleave(this._listenOnDomNode, (o) => this._onMouseLeave(o)), this._hideTimeout = this._register(new Ye2), this._isDragging = false, this._mouseIsOver = false, this._shouldRender = true, this._revealOnScroll = true;
  }
  get options() {
    return this._options;
  }
  dispose() {
    this._mouseWheelToDispose = Ne2(this._mouseWheelToDispose), super.dispose();
  }
  getDomNode() {
    return this._domNode;
  }
  getOverviewRulerLayoutInfo() {
    return { parent: this._domNode, insertBefore: this._verticalScrollbar.domNode.domNode };
  }
  delegateVerticalScrollbarPointerDown(e) {
    this._verticalScrollbar.delegatePointerDown(e);
  }
  getScrollDimensions() {
    return this._scrollable.getScrollDimensions();
  }
  setScrollDimensions(e) {
    this._scrollable.setScrollDimensions(e, false);
  }
  updateClassName(e) {
    this._options.className = e, Te2 && (this._options.className += " mac"), this._domNode.className = "xterm-scrollable-element " + this._options.className;
  }
  updateOptions(e) {
    typeof e.handleMouseWheel < "u" && (this._options.handleMouseWheel = e.handleMouseWheel, this._setListeningToMouseWheel(this._options.handleMouseWheel)), typeof e.mouseWheelScrollSensitivity < "u" && (this._options.mouseWheelScrollSensitivity = e.mouseWheelScrollSensitivity), typeof e.fastScrollSensitivity < "u" && (this._options.fastScrollSensitivity = e.fastScrollSensitivity), typeof e.scrollPredominantAxis < "u" && (this._options.scrollPredominantAxis = e.scrollPredominantAxis), typeof e.horizontal < "u" && (this._options.horizontal = e.horizontal), typeof e.vertical < "u" && (this._options.vertical = e.vertical), typeof e.horizontalScrollbarSize < "u" && (this._options.horizontalScrollbarSize = e.horizontalScrollbarSize), typeof e.verticalScrollbarSize < "u" && (this._options.verticalScrollbarSize = e.verticalScrollbarSize), typeof e.scrollByPage < "u" && (this._options.scrollByPage = e.scrollByPage), this._horizontalScrollbar.updateOptions(this._options), this._verticalScrollbar.updateOptions(this._options), this._options.lazyRender || this._render();
  }
  setRevealOnScroll(e) {
    this._revealOnScroll = e;
  }
  delegateScrollFromMouseWheelEvent(e) {
    this._onMouseWheel(new xi2(e));
  }
  _setListeningToMouseWheel(e) {
    if (this._mouseWheelToDispose.length > 0 !== e && (this._mouseWheelToDispose = Ne2(this._mouseWheelToDispose), e)) {
      let r = (n) => {
        this._onMouseWheel(new xi2(n));
      };
      this._mouseWheelToDispose.push(L(this._listenOnDomNode, Y2.MOUSE_WHEEL, r, { passive: false }));
    }
  }
  _onMouseWheel(e) {
    if (e.browserEvent?.defaultPrevented)
      return;
    let i = hs2.INSTANCE;
    zo && i.acceptStandardWheelEvent(e);
    let r = false;
    if (e.deltaY || e.deltaX) {
      let o = e.deltaY * this._options.mouseWheelScrollSensitivity, l = e.deltaX * this._options.mouseWheelScrollSensitivity;
      this._options.scrollPredominantAxis && (this._options.scrollYToX && l + o === 0 ? l = o = 0 : Math.abs(o) >= Math.abs(l) ? l = 0 : o = 0), this._options.flipAxes && ([o, l] = [l, o]);
      let a = !Te2 && e.browserEvent && e.browserEvent.shiftKey;
      (this._options.scrollYToX || a) && !l && (l = o, o = 0), e.browserEvent && e.browserEvent.altKey && (l = l * this._options.fastScrollSensitivity, o = o * this._options.fastScrollSensitivity);
      let u = this._scrollable.getFutureScrollPosition(), h = {};
      if (o) {
        let c = Ko * o, d = u.scrollTop - (c < 0 ? Math.floor(c) : Math.ceil(c));
        this._verticalScrollbar.writeScrollPosition(h, d);
      }
      if (l) {
        let c = Ko * l, d = u.scrollLeft - (c < 0 ? Math.floor(c) : Math.ceil(c));
        this._horizontalScrollbar.writeScrollPosition(h, d);
      }
      h = this._scrollable.validateScrollPosition(h), (u.scrollLeft !== h.scrollLeft || u.scrollTop !== h.scrollTop) && (zo && this._options.mouseWheelSmoothScroll && i.isPhysicalMouseWheel() ? this._scrollable.setScrollPositionSmooth(h) : this._scrollable.setScrollPositionNow(h), r = true);
    }
    let n = r;
    !n && this._options.alwaysConsumeMouseWheel && (n = true), !n && this._options.consumeMouseWheelIfScrollbarIsNeeded && (this._verticalScrollbar.isNeeded() || this._horizontalScrollbar.isNeeded()) && (n = true), n && (e.preventDefault(), e.stopPropagation());
  }
  _onDidScroll(e) {
    this._shouldRender = this._horizontalScrollbar.onDidScroll(e) || this._shouldRender, this._shouldRender = this._verticalScrollbar.onDidScroll(e) || this._shouldRender, this._options.useShadows && (this._shouldRender = true), this._revealOnScroll && this._reveal(), this._options.lazyRender || this._render();
  }
  renderNow() {
    if (!this._options.lazyRender)
      throw new Error("Please use `lazyRender` together with `renderNow`!");
    this._render();
  }
  _render() {
    if (this._shouldRender && (this._shouldRender = false, this._horizontalScrollbar.render(), this._verticalScrollbar.render(), this._options.useShadows)) {
      let e = this._scrollable.getCurrentScrollPosition(), i = e.scrollTop > 0, r = e.scrollLeft > 0, n = r ? " left" : "", o = i ? " top" : "", l = r || i ? " top-left-corner" : "";
      this._leftShadowDomNode.setClassName(`shadow${n}`), this._topShadowDomNode.setClassName(`shadow${o}`), this._topLeftShadowDomNode.setClassName(`shadow${l}${o}${n}`);
    }
  }
  _onDragStart() {
    this._isDragging = true, this._reveal();
  }
  _onDragEnd() {
    this._isDragging = false, this._hide();
  }
  _onMouseLeave(e) {
    this._mouseIsOver = false, this._hide();
  }
  _onMouseOver(e) {
    this._mouseIsOver = true, this._reveal();
  }
  _reveal() {
    this._verticalScrollbar.beginReveal(), this._horizontalScrollbar.beginReveal(), this._scheduleHide();
  }
  _hide() {
    !this._mouseIsOver && !this._isDragging && (this._verticalScrollbar.beginHide(), this._horizontalScrollbar.beginHide());
  }
  _scheduleHide() {
    !this._mouseIsOver && !this._isDragging && this._hideTimeout.cancelAndSet(() => this._hide(), Ma);
  }
};
var Kr2 = class extends ds2 {
  constructor(t, e, i) {
    super(t, e, i);
  }
  setScrollPosition(t) {
    t.reuseAnimation ? this._scrollable.setScrollPositionSmooth(t, t.reuseAnimation) : this._scrollable.setScrollPositionNow(t);
  }
  getScrollPosition() {
    return this._scrollable.getCurrentScrollPosition();
  }
};
function Pa(s) {
  let t = { lazyRender: typeof s.lazyRender < "u" ? s.lazyRender : false, className: typeof s.className < "u" ? s.className : "", useShadows: typeof s.useShadows < "u" ? s.useShadows : true, handleMouseWheel: typeof s.handleMouseWheel < "u" ? s.handleMouseWheel : true, flipAxes: typeof s.flipAxes < "u" ? s.flipAxes : false, consumeMouseWheelIfScrollbarIsNeeded: typeof s.consumeMouseWheelIfScrollbarIsNeeded < "u" ? s.consumeMouseWheelIfScrollbarIsNeeded : false, alwaysConsumeMouseWheel: typeof s.alwaysConsumeMouseWheel < "u" ? s.alwaysConsumeMouseWheel : false, scrollYToX: typeof s.scrollYToX < "u" ? s.scrollYToX : false, mouseWheelScrollSensitivity: typeof s.mouseWheelScrollSensitivity < "u" ? s.mouseWheelScrollSensitivity : 1, fastScrollSensitivity: typeof s.fastScrollSensitivity < "u" ? s.fastScrollSensitivity : 5, scrollPredominantAxis: typeof s.scrollPredominantAxis < "u" ? s.scrollPredominantAxis : true, mouseWheelSmoothScroll: typeof s.mouseWheelSmoothScroll < "u" ? s.mouseWheelSmoothScroll : true, arrowSize: typeof s.arrowSize < "u" ? s.arrowSize : 11, listenOnDomNode: typeof s.listenOnDomNode < "u" ? s.listenOnDomNode : null, horizontal: typeof s.horizontal < "u" ? s.horizontal : 1, horizontalScrollbarSize: typeof s.horizontalScrollbarSize < "u" ? s.horizontalScrollbarSize : 10, horizontalSliderSize: typeof s.horizontalSliderSize < "u" ? s.horizontalSliderSize : 0, horizontalHasArrows: typeof s.horizontalHasArrows < "u" ? s.horizontalHasArrows : false, vertical: typeof s.vertical < "u" ? s.vertical : 1, verticalScrollbarSize: typeof s.verticalScrollbarSize < "u" ? s.verticalScrollbarSize : 10, verticalHasArrows: typeof s.verticalHasArrows < "u" ? s.verticalHasArrows : false, verticalSliderSize: typeof s.verticalSliderSize < "u" ? s.verticalSliderSize : 0, scrollByPage: typeof s.scrollByPage < "u" ? s.scrollByPage : false };
  return t.horizontalSliderSize = typeof s.horizontalSliderSize < "u" ? s.horizontalSliderSize : t.horizontalScrollbarSize, t.verticalSliderSize = typeof s.verticalSliderSize < "u" ? s.verticalSliderSize : t.verticalScrollbarSize, Te2 && (t.className += " mac"), t;
}
var zt2 = class extends D2 {
  constructor(e, i, r, n, o, l, a, u) {
    super();
    this._bufferService = r;
    this._optionsService = a;
    this._renderService = u;
    this._onRequestScrollLines = this._register(new v);
    this.onRequestScrollLines = this._onRequestScrollLines.event;
    this._isSyncing = false;
    this._isHandlingScroll = false;
    this._suppressOnScrollHandler = false;
    let h = this._register(new Ri({ forceIntegerValues: false, smoothScrollDuration: this._optionsService.rawOptions.smoothScrollDuration, scheduleAtNextAnimationFrame: (c) => mt(n.window, c) }));
    this._register(this._optionsService.onSpecificOptionChange("smoothScrollDuration", () => {
      h.setSmoothScrollDuration(this._optionsService.rawOptions.smoothScrollDuration);
    })), this._scrollableElement = this._register(new Kr2(i, { vertical: 1, horizontal: 2, useShadows: false, mouseWheelSmoothScroll: true, ...this._getChangeOptions() }, h)), this._register(this._optionsService.onMultipleOptionChange(["scrollSensitivity", "fastScrollSensitivity", "overviewRuler"], () => this._scrollableElement.updateOptions(this._getChangeOptions()))), this._register(o.onProtocolChange((c) => {
      this._scrollableElement.updateOptions({ handleMouseWheel: !(c & 16) });
    })), this._scrollableElement.setScrollDimensions({ height: 0, scrollHeight: 0 }), this._register($.runAndSubscribe(l.onChangeColors, () => {
      this._scrollableElement.getDomNode().style.backgroundColor = l.colors.background.css;
    })), e.appendChild(this._scrollableElement.getDomNode()), this._register(C2(() => this._scrollableElement.getDomNode().remove())), this._styleElement = n.mainDocument.createElement("style"), i.appendChild(this._styleElement), this._register(C2(() => this._styleElement.remove())), this._register($.runAndSubscribe(l.onChangeColors, () => {
      this._styleElement.textContent = [".xterm .xterm-scrollable-element > .scrollbar > .slider {", `  background: ${l.colors.scrollbarSliderBackground.css};`, "}", ".xterm .xterm-scrollable-element > .scrollbar > .slider:hover {", `  background: ${l.colors.scrollbarSliderHoverBackground.css};`, "}", ".xterm .xterm-scrollable-element > .scrollbar > .slider.active {", `  background: ${l.colors.scrollbarSliderActiveBackground.css};`, "}"].join(`
`);
    })), this._register(this._bufferService.onResize(() => this.queueSync())), this._register(this._bufferService.buffers.onBufferActivate(() => {
      this._latestYDisp = undefined, this.queueSync();
    })), this._register(this._bufferService.onScroll(() => this._sync())), this._register(this._scrollableElement.onScroll((c) => this._handleScroll(c)));
  }
  scrollLines(e) {
    let i = this._scrollableElement.getScrollPosition();
    this._scrollableElement.setScrollPosition({ reuseAnimation: true, scrollTop: i.scrollTop + e * this._renderService.dimensions.css.cell.height });
  }
  scrollToLine(e, i) {
    i && (this._latestYDisp = e), this._scrollableElement.setScrollPosition({ reuseAnimation: !i, scrollTop: e * this._renderService.dimensions.css.cell.height });
  }
  _getChangeOptions() {
    return { mouseWheelScrollSensitivity: this._optionsService.rawOptions.scrollSensitivity, fastScrollSensitivity: this._optionsService.rawOptions.fastScrollSensitivity, verticalScrollbarSize: this._optionsService.rawOptions.overviewRuler?.width || 14 };
  }
  queueSync(e) {
    e !== undefined && (this._latestYDisp = e), this._queuedAnimationFrame === undefined && (this._queuedAnimationFrame = this._renderService.addRefreshCallback(() => {
      this._queuedAnimationFrame = undefined, this._sync(this._latestYDisp);
    }));
  }
  _sync(e = this._bufferService.buffer.ydisp) {
    !this._renderService || this._isSyncing || (this._isSyncing = true, this._suppressOnScrollHandler = true, this._scrollableElement.setScrollDimensions({ height: this._renderService.dimensions.css.canvas.height, scrollHeight: this._renderService.dimensions.css.cell.height * this._bufferService.buffer.lines.length }), this._suppressOnScrollHandler = false, e !== this._latestYDisp && this._scrollableElement.setScrollPosition({ scrollTop: e * this._renderService.dimensions.css.cell.height }), this._isSyncing = false);
  }
  _handleScroll(e) {
    if (!this._renderService || this._isHandlingScroll || this._suppressOnScrollHandler)
      return;
    this._isHandlingScroll = true;
    let i = Math.round(e.scrollTop / this._renderService.dimensions.css.cell.height), r = i - this._bufferService.buffer.ydisp;
    r !== 0 && (this._latestYDisp = i, this._onRequestScrollLines.fire(r)), this._isHandlingScroll = false;
  }
};
zt2 = M([S(2, F2), S(3, ae2), S(4, rr), S(5, Re), S(6, H2), S(7, ce)], zt2);
var Gt2 = class extends D2 {
  constructor(e, i, r, n, o) {
    super();
    this._screenElement = e;
    this._bufferService = i;
    this._coreBrowserService = r;
    this._decorationService = n;
    this._renderService = o;
    this._decorationElements = new Map;
    this._altBufferIsActive = false;
    this._dimensionsChanged = false;
    this._container = document.createElement("div"), this._container.classList.add("xterm-decoration-container"), this._screenElement.appendChild(this._container), this._register(this._renderService.onRenderedViewportChange(() => this._doRefreshDecorations())), this._register(this._renderService.onDimensionsChange(() => {
      this._dimensionsChanged = true, this._queueRefresh();
    })), this._register(this._coreBrowserService.onDprChange(() => this._queueRefresh())), this._register(this._bufferService.buffers.onBufferActivate(() => {
      this._altBufferIsActive = this._bufferService.buffer === this._bufferService.buffers.alt;
    })), this._register(this._decorationService.onDecorationRegistered(() => this._queueRefresh())), this._register(this._decorationService.onDecorationRemoved((l) => this._removeDecoration(l))), this._register(C2(() => {
      this._container.remove(), this._decorationElements.clear();
    }));
  }
  _queueRefresh() {
    this._animationFrame === undefined && (this._animationFrame = this._renderService.addRefreshCallback(() => {
      this._doRefreshDecorations(), this._animationFrame = undefined;
    }));
  }
  _doRefreshDecorations() {
    for (let e of this._decorationService.decorations)
      this._renderDecoration(e);
    this._dimensionsChanged = false;
  }
  _renderDecoration(e) {
    this._refreshStyle(e), this._dimensionsChanged && this._refreshXPosition(e);
  }
  _createElement(e) {
    let i = this._coreBrowserService.mainDocument.createElement("div");
    i.classList.add("xterm-decoration"), i.classList.toggle("xterm-decoration-top-layer", e?.options?.layer === "top"), i.style.width = `${Math.round((e.options.width || 1) * this._renderService.dimensions.css.cell.width)}px`, i.style.height = `${(e.options.height || 1) * this._renderService.dimensions.css.cell.height}px`, i.style.top = `${(e.marker.line - this._bufferService.buffers.active.ydisp) * this._renderService.dimensions.css.cell.height}px`, i.style.lineHeight = `${this._renderService.dimensions.css.cell.height}px`;
    let r = e.options.x ?? 0;
    return r && r > this._bufferService.cols && (i.style.display = "none"), this._refreshXPosition(e, i), i;
  }
  _refreshStyle(e) {
    let i = e.marker.line - this._bufferService.buffers.active.ydisp;
    if (i < 0 || i >= this._bufferService.rows)
      e.element && (e.element.style.display = "none", e.onRenderEmitter.fire(e.element));
    else {
      let r = this._decorationElements.get(e);
      r || (r = this._createElement(e), e.element = r, this._decorationElements.set(e, r), this._container.appendChild(r), e.onDispose(() => {
        this._decorationElements.delete(e), r.remove();
      })), r.style.display = this._altBufferIsActive ? "none" : "block", this._altBufferIsActive || (r.style.width = `${Math.round((e.options.width || 1) * this._renderService.dimensions.css.cell.width)}px`, r.style.height = `${(e.options.height || 1) * this._renderService.dimensions.css.cell.height}px`, r.style.top = `${i * this._renderService.dimensions.css.cell.height}px`, r.style.lineHeight = `${this._renderService.dimensions.css.cell.height}px`), e.onRenderEmitter.fire(r);
    }
  }
  _refreshXPosition(e, i = e.element) {
    if (!i)
      return;
    let r = e.options.x ?? 0;
    (e.options.anchor || "left") === "right" ? i.style.right = r ? `${r * this._renderService.dimensions.css.cell.width}px` : "" : i.style.left = r ? `${r * this._renderService.dimensions.css.cell.width}px` : "";
  }
  _removeDecoration(e) {
    this._decorationElements.get(e)?.remove(), this._decorationElements.delete(e), e.dispose();
  }
};
Gt2 = M([S(1, F2), S(2, ae2), S(3, Be2), S(4, ce)], Gt2);
var Gr2 = class {
  constructor() {
    this._zones = [];
    this._zonePool = [];
    this._zonePoolIndex = 0;
    this._linePadding = { full: 0, left: 0, center: 0, right: 0 };
  }
  get zones() {
    return this._zonePool.length = Math.min(this._zonePool.length, this._zones.length), this._zones;
  }
  clear() {
    this._zones.length = 0, this._zonePoolIndex = 0;
  }
  addDecoration(t) {
    if (t.options.overviewRulerOptions) {
      for (let e of this._zones)
        if (e.color === t.options.overviewRulerOptions.color && e.position === t.options.overviewRulerOptions.position) {
          if (this._lineIntersectsZone(e, t.marker.line))
            return;
          if (this._lineAdjacentToZone(e, t.marker.line, t.options.overviewRulerOptions.position)) {
            this._addLineToZone(e, t.marker.line);
            return;
          }
        }
      if (this._zonePoolIndex < this._zonePool.length) {
        this._zonePool[this._zonePoolIndex].color = t.options.overviewRulerOptions.color, this._zonePool[this._zonePoolIndex].position = t.options.overviewRulerOptions.position, this._zonePool[this._zonePoolIndex].startBufferLine = t.marker.line, this._zonePool[this._zonePoolIndex].endBufferLine = t.marker.line, this._zones.push(this._zonePool[this._zonePoolIndex++]);
        return;
      }
      this._zones.push({ color: t.options.overviewRulerOptions.color, position: t.options.overviewRulerOptions.position, startBufferLine: t.marker.line, endBufferLine: t.marker.line }), this._zonePool.push(this._zones[this._zones.length - 1]), this._zonePoolIndex++;
    }
  }
  setPadding(t) {
    this._linePadding = t;
  }
  _lineIntersectsZone(t, e) {
    return e >= t.startBufferLine && e <= t.endBufferLine;
  }
  _lineAdjacentToZone(t, e, i) {
    return e >= t.startBufferLine - this._linePadding[i || "full"] && e <= t.endBufferLine + this._linePadding[i || "full"];
  }
  _addLineToZone(t, e) {
    t.startBufferLine = Math.min(t.startBufferLine, e), t.endBufferLine = Math.max(t.endBufferLine, e);
  }
};
var We2 = { full: 0, left: 0, center: 0, right: 0 };
var at2 = { full: 0, left: 0, center: 0, right: 0 };
var Li2 = { full: 0, left: 0, center: 0, right: 0 };
var bt2 = class extends D2 {
  constructor(e, i, r, n, o, l, a, u) {
    super();
    this._viewportElement = e;
    this._screenElement = i;
    this._bufferService = r;
    this._decorationService = n;
    this._renderService = o;
    this._optionsService = l;
    this._themeService = a;
    this._coreBrowserService = u;
    this._colorZoneStore = new Gr2;
    this._shouldUpdateDimensions = true;
    this._shouldUpdateAnchor = true;
    this._lastKnownBufferLength = 0;
    this._canvas = this._coreBrowserService.mainDocument.createElement("canvas"), this._canvas.classList.add("xterm-decoration-overview-ruler"), this._refreshCanvasDimensions(), this._viewportElement.parentElement?.insertBefore(this._canvas, this._viewportElement), this._register(C2(() => this._canvas?.remove()));
    let h = this._canvas.getContext("2d");
    if (h)
      this._ctx = h;
    else
      throw new Error("Ctx cannot be null");
    this._register(this._decorationService.onDecorationRegistered(() => this._queueRefresh(undefined, true))), this._register(this._decorationService.onDecorationRemoved(() => this._queueRefresh(undefined, true))), this._register(this._renderService.onRenderedViewportChange(() => this._queueRefresh())), this._register(this._bufferService.buffers.onBufferActivate(() => {
      this._canvas.style.display = this._bufferService.buffer === this._bufferService.buffers.alt ? "none" : "block";
    })), this._register(this._bufferService.onScroll(() => {
      this._lastKnownBufferLength !== this._bufferService.buffers.normal.lines.length && (this._refreshDrawHeightConstants(), this._refreshColorZonePadding());
    })), this._register(this._renderService.onRender(() => {
      (!this._containerHeight || this._containerHeight !== this._screenElement.clientHeight) && (this._queueRefresh(true), this._containerHeight = this._screenElement.clientHeight);
    })), this._register(this._coreBrowserService.onDprChange(() => this._queueRefresh(true))), this._register(this._optionsService.onSpecificOptionChange("overviewRuler", () => this._queueRefresh(true))), this._register(this._themeService.onChangeColors(() => this._queueRefresh())), this._queueRefresh(true);
  }
  get _width() {
    return this._optionsService.options.overviewRuler?.width || 0;
  }
  _refreshDrawConstants() {
    let e = Math.floor((this._canvas.width - 1) / 3), i = Math.ceil((this._canvas.width - 1) / 3);
    at2.full = this._canvas.width, at2.left = e, at2.center = i, at2.right = e, this._refreshDrawHeightConstants(), Li2.full = 1, Li2.left = 1, Li2.center = 1 + at2.left, Li2.right = 1 + at2.left + at2.center;
  }
  _refreshDrawHeightConstants() {
    We2.full = Math.round(2 * this._coreBrowserService.dpr);
    let e = this._canvas.height / this._bufferService.buffer.lines.length, i = Math.round(Math.max(Math.min(e, 12), 6) * this._coreBrowserService.dpr);
    We2.left = i, We2.center = i, We2.right = i;
  }
  _refreshColorZonePadding() {
    this._colorZoneStore.setPadding({ full: Math.floor(this._bufferService.buffers.active.lines.length / (this._canvas.height - 1) * We2.full), left: Math.floor(this._bufferService.buffers.active.lines.length / (this._canvas.height - 1) * We2.left), center: Math.floor(this._bufferService.buffers.active.lines.length / (this._canvas.height - 1) * We2.center), right: Math.floor(this._bufferService.buffers.active.lines.length / (this._canvas.height - 1) * We2.right) }), this._lastKnownBufferLength = this._bufferService.buffers.normal.lines.length;
  }
  _refreshCanvasDimensions() {
    this._canvas.style.width = `${this._width}px`, this._canvas.width = Math.round(this._width * this._coreBrowserService.dpr), this._canvas.style.height = `${this._screenElement.clientHeight}px`, this._canvas.height = Math.round(this._screenElement.clientHeight * this._coreBrowserService.dpr), this._refreshDrawConstants(), this._refreshColorZonePadding();
  }
  _refreshDecorations() {
    this._shouldUpdateDimensions && this._refreshCanvasDimensions(), this._ctx.clearRect(0, 0, this._canvas.width, this._canvas.height), this._colorZoneStore.clear();
    for (let i of this._decorationService.decorations)
      this._colorZoneStore.addDecoration(i);
    this._ctx.lineWidth = 1, this._renderRulerOutline();
    let e = this._colorZoneStore.zones;
    for (let i of e)
      i.position !== "full" && this._renderColorZone(i);
    for (let i of e)
      i.position === "full" && this._renderColorZone(i);
    this._shouldUpdateDimensions = false, this._shouldUpdateAnchor = false;
  }
  _renderRulerOutline() {
    this._ctx.fillStyle = this._themeService.colors.overviewRulerBorder.css, this._ctx.fillRect(0, 0, 1, this._canvas.height), this._optionsService.rawOptions.overviewRuler.showTopBorder && this._ctx.fillRect(1, 0, this._canvas.width - 1, 1), this._optionsService.rawOptions.overviewRuler.showBottomBorder && this._ctx.fillRect(1, this._canvas.height - 1, this._canvas.width - 1, this._canvas.height);
  }
  _renderColorZone(e) {
    this._ctx.fillStyle = e.color, this._ctx.fillRect(Li2[e.position || "full"], Math.round((this._canvas.height - 1) * (e.startBufferLine / this._bufferService.buffers.active.lines.length) - We2[e.position || "full"] / 2), at2[e.position || "full"], Math.round((this._canvas.height - 1) * ((e.endBufferLine - e.startBufferLine) / this._bufferService.buffers.active.lines.length) + We2[e.position || "full"]));
  }
  _queueRefresh(e, i) {
    this._shouldUpdateDimensions = e || this._shouldUpdateDimensions, this._shouldUpdateAnchor = i || this._shouldUpdateAnchor, this._animationFrame === undefined && (this._animationFrame = this._coreBrowserService.window.requestAnimationFrame(() => {
      this._refreshDecorations(), this._animationFrame = undefined;
    }));
  }
};
bt2 = M([S(2, F2), S(3, Be2), S(4, ce), S(5, H2), S(6, Re), S(7, ae2)], bt2);
var b;
((E) => (E.NUL = "\x00", E.SOH = "\x01", E.STX = "\x02", E.ETX = "\x03", E.EOT = "\x04", E.ENQ = "\x05", E.ACK = "\x06", E.BEL = "\x07", E.BS = "\b", E.HT = "\t", E.LF = `
`, E.VT = "\v", E.FF = "\f", E.CR = "\r", E.SO = "\x0E", E.SI = "\x0F", E.DLE = "\x10", E.DC1 = "\x11", E.DC2 = "\x12", E.DC3 = "\x13", E.DC4 = "\x14", E.NAK = "\x15", E.SYN = "\x16", E.ETB = "\x17", E.CAN = "\x18", E.EM = "\x19", E.SUB = "\x1A", E.ESC = "\x1B", E.FS = "\x1C", E.GS = "\x1D", E.RS = "\x1E", E.US = "\x1F", E.SP = " ", E.DEL = ""))(b ||= {});
var Ai2;
((g) => (g.PAD = "", g.HOP = "", g.BPH = "", g.NBH = "", g.IND = "", g.NEL = "", g.SSA = "", g.ESA = "", g.HTS = "", g.HTJ = "", g.VTS = "", g.PLD = "", g.PLU = "", g.RI = "", g.SS2 = "", g.SS3 = "", g.DCS = "", g.PU1 = "", g.PU2 = "", g.STS = "", g.CCH = "", g.MW = "", g.SPA = "", g.EPA = "", g.SOS = "", g.SGCI = "", g.SCI = "", g.CSI = "", g.ST = "", g.OSC = "", g.PM = "", g.APC = ""))(Ai2 ||= {});
var fs2;
((t) => t.ST = `${b.ESC}\\`)(fs2 ||= {});
var $t2 = class {
  constructor(t, e, i, r, n, o) {
    this._textarea = t;
    this._compositionView = e;
    this._bufferService = i;
    this._optionsService = r;
    this._coreService = n;
    this._renderService = o;
    this._isComposing = false, this._isSendingComposition = false, this._compositionPosition = { start: 0, end: 0 }, this._dataAlreadySent = "";
  }
  get isComposing() {
    return this._isComposing;
  }
  compositionstart() {
    this._isComposing = true, this._compositionPosition.start = this._textarea.value.length, this._compositionView.textContent = "", this._dataAlreadySent = "", this._compositionView.classList.add("active");
  }
  compositionupdate(t) {
    this._compositionView.textContent = t.data, this.updateCompositionElements(), setTimeout(() => {
      this._compositionPosition.end = this._textarea.value.length;
    }, 0);
  }
  compositionend() {
    this._finalizeComposition(true);
  }
  keydown(t) {
    if (this._isComposing || this._isSendingComposition) {
      if (t.keyCode === 20 || t.keyCode === 229 || t.keyCode === 16 || t.keyCode === 17 || t.keyCode === 18)
        return false;
      this._finalizeComposition(false);
    }
    return t.keyCode === 229 ? (this._handleAnyTextareaChanges(), false) : true;
  }
  _finalizeComposition(t) {
    if (this._compositionView.classList.remove("active"), this._isComposing = false, t) {
      let e = { start: this._compositionPosition.start, end: this._compositionPosition.end };
      this._isSendingComposition = true, setTimeout(() => {
        if (this._isSendingComposition) {
          this._isSendingComposition = false;
          let i;
          e.start += this._dataAlreadySent.length, this._isComposing ? i = this._textarea.value.substring(e.start, this._compositionPosition.start) : i = this._textarea.value.substring(e.start), i.length > 0 && this._coreService.triggerDataEvent(i, true);
        }
      }, 0);
    } else {
      this._isSendingComposition = false;
      let e = this._textarea.value.substring(this._compositionPosition.start, this._compositionPosition.end);
      this._coreService.triggerDataEvent(e, true);
    }
  }
  _handleAnyTextareaChanges() {
    let t = this._textarea.value;
    setTimeout(() => {
      if (!this._isComposing) {
        let e = this._textarea.value, i = e.replace(t, "");
        this._dataAlreadySent = i, e.length > t.length ? this._coreService.triggerDataEvent(i, true) : e.length < t.length ? this._coreService.triggerDataEvent(`${b.DEL}`, true) : e.length === t.length && e !== t && this._coreService.triggerDataEvent(e, true);
      }
    }, 0);
  }
  updateCompositionElements(t) {
    if (this._isComposing) {
      if (this._bufferService.buffer.isCursorInViewport) {
        let e = Math.min(this._bufferService.buffer.x, this._bufferService.cols - 1), i = this._renderService.dimensions.css.cell.height, r = this._bufferService.buffer.y * this._renderService.dimensions.css.cell.height, n = e * this._renderService.dimensions.css.cell.width;
        this._compositionView.style.left = n + "px", this._compositionView.style.top = r + "px", this._compositionView.style.height = i + "px", this._compositionView.style.lineHeight = i + "px", this._compositionView.style.fontFamily = this._optionsService.rawOptions.fontFamily, this._compositionView.style.fontSize = this._optionsService.rawOptions.fontSize + "px";
        let o = this._compositionView.getBoundingClientRect();
        this._textarea.style.left = n + "px", this._textarea.style.top = r + "px", this._textarea.style.width = Math.max(o.width, 1) + "px", this._textarea.style.height = Math.max(o.height, 1) + "px", this._textarea.style.lineHeight = o.height + "px";
      }
      t || setTimeout(() => this.updateCompositionElements(true), 0);
    }
  }
};
$t2 = M([S(2, F2), S(3, H2), S(4, ge2), S(5, ce)], $t2);
var ue = 0;
var he2 = 0;
var de = 0;
var J2 = 0;
var ps2 = { css: "#00000000", rgba: 0 };
var j2;
((i) => {
  function s(r, n, o, l) {
    return l !== undefined ? `#${vt2(r)}${vt2(n)}${vt2(o)}${vt2(l)}` : `#${vt2(r)}${vt2(n)}${vt2(o)}`;
  }
  i.toCss = s;
  function t(r, n, o, l = 255) {
    return (r << 24 | n << 16 | o << 8 | l) >>> 0;
  }
  i.toRgba = t;
  function e(r, n, o, l) {
    return { css: i.toCss(r, n, o, l), rgba: i.toRgba(r, n, o, l) };
  }
  i.toColor = e;
})(j2 ||= {});
var U2;
((l) => {
  function s(a, u) {
    if (J2 = (u.rgba & 255) / 255, J2 === 1)
      return { css: u.css, rgba: u.rgba };
    let h = u.rgba >> 24 & 255, c = u.rgba >> 16 & 255, d = u.rgba >> 8 & 255, _ = a.rgba >> 24 & 255, p = a.rgba >> 16 & 255, m = a.rgba >> 8 & 255;
    ue = _ + Math.round((h - _) * J2), he2 = p + Math.round((c - p) * J2), de = m + Math.round((d - m) * J2);
    let f = j2.toCss(ue, he2, de), A = j2.toRgba(ue, he2, de);
    return { css: f, rgba: A };
  }
  l.blend = s;
  function t(a) {
    return (a.rgba & 255) === 255;
  }
  l.isOpaque = t;
  function e(a, u, h) {
    let c = $r2.ensureContrastRatio(a.rgba, u.rgba, h);
    if (c)
      return j2.toColor(c >> 24 & 255, c >> 16 & 255, c >> 8 & 255);
  }
  l.ensureContrastRatio = e;
  function i(a) {
    let u = (a.rgba | 255) >>> 0;
    return [ue, he2, de] = $r2.toChannels(u), { css: j2.toCss(ue, he2, de), rgba: u };
  }
  l.opaque = i;
  function r(a, u) {
    return J2 = Math.round(u * 255), [ue, he2, de] = $r2.toChannels(a.rgba), { css: j2.toCss(ue, he2, de, J2), rgba: j2.toRgba(ue, he2, de, J2) };
  }
  l.opacity = r;
  function n(a, u) {
    return J2 = a.rgba & 255, r(a, J2 * u / 255);
  }
  l.multiplyOpacity = n;
  function o(a) {
    return [a.rgba >> 24 & 255, a.rgba >> 16 & 255, a.rgba >> 8 & 255];
  }
  l.toColorRGB = o;
})(U2 ||= {});
var z2;
((i) => {
  let s, t;
  try {
    let r = document.createElement("canvas");
    r.width = 1, r.height = 1;
    let n = r.getContext("2d", { willReadFrequently: true });
    n && (s = n, s.globalCompositeOperation = "copy", t = s.createLinearGradient(0, 0, 1, 1));
  } catch {}
  function e(r) {
    if (r.match(/#[\da-f]{3,8}/i))
      switch (r.length) {
        case 4:
          return ue = parseInt(r.slice(1, 2).repeat(2), 16), he2 = parseInt(r.slice(2, 3).repeat(2), 16), de = parseInt(r.slice(3, 4).repeat(2), 16), j2.toColor(ue, he2, de);
        case 5:
          return ue = parseInt(r.slice(1, 2).repeat(2), 16), he2 = parseInt(r.slice(2, 3).repeat(2), 16), de = parseInt(r.slice(3, 4).repeat(2), 16), J2 = parseInt(r.slice(4, 5).repeat(2), 16), j2.toColor(ue, he2, de, J2);
        case 7:
          return { css: r, rgba: (parseInt(r.slice(1), 16) << 8 | 255) >>> 0 };
        case 9:
          return { css: r, rgba: parseInt(r.slice(1), 16) >>> 0 };
      }
    let n = r.match(/rgba?\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*(,\s*(0|1|\d?\.(\d+))\s*)?\)/);
    if (n)
      return ue = parseInt(n[1]), he2 = parseInt(n[2]), de = parseInt(n[3]), J2 = Math.round((n[5] === undefined ? 1 : parseFloat(n[5])) * 255), j2.toColor(ue, he2, de, J2);
    if (!s || !t)
      throw new Error("css.toColor: Unsupported css format");
    if (s.fillStyle = t, s.fillStyle = r, typeof s.fillStyle != "string")
      throw new Error("css.toColor: Unsupported css format");
    if (s.fillRect(0, 0, 1, 1), [ue, he2, de, J2] = s.getImageData(0, 0, 1, 1).data, J2 !== 255)
      throw new Error("css.toColor: Unsupported css format");
    return { rgba: j2.toRgba(ue, he2, de, J2), css: r };
  }
  i.toColor = e;
})(z2 ||= {});
var ve2;
((e) => {
  function s(i) {
    return t(i >> 16 & 255, i >> 8 & 255, i & 255);
  }
  e.relativeLuminance = s;
  function t(i, r, n) {
    let o = i / 255, l = r / 255, a = n / 255, u = o <= 0.03928 ? o / 12.92 : Math.pow((o + 0.055) / 1.055, 2.4), h = l <= 0.03928 ? l / 12.92 : Math.pow((l + 0.055) / 1.055, 2.4), c = a <= 0.03928 ? a / 12.92 : Math.pow((a + 0.055) / 1.055, 2.4);
    return u * 0.2126 + h * 0.7152 + c * 0.0722;
  }
  e.relativeLuminance2 = t;
})(ve2 ||= {});
var $r2;
((n) => {
  function s(o, l) {
    if (J2 = (l & 255) / 255, J2 === 1)
      return l;
    let a = l >> 24 & 255, u = l >> 16 & 255, h = l >> 8 & 255, c = o >> 24 & 255, d = o >> 16 & 255, _ = o >> 8 & 255;
    return ue = c + Math.round((a - c) * J2), he2 = d + Math.round((u - d) * J2), de = _ + Math.round((h - _) * J2), j2.toRgba(ue, he2, de);
  }
  n.blend = s;
  function t(o, l, a) {
    let u = ve2.relativeLuminance(o >> 8), h = ve2.relativeLuminance(l >> 8);
    if (Xe2(u, h) < a) {
      if (h < u) {
        let p = e(o, l, a), m = Xe2(u, ve2.relativeLuminance(p >> 8));
        if (m < a) {
          let f = i(o, l, a), A = Xe2(u, ve2.relativeLuminance(f >> 8));
          return m > A ? p : f;
        }
        return p;
      }
      let d = i(o, l, a), _ = Xe2(u, ve2.relativeLuminance(d >> 8));
      if (_ < a) {
        let p = e(o, l, a), m = Xe2(u, ve2.relativeLuminance(p >> 8));
        return _ > m ? d : p;
      }
      return d;
    }
  }
  n.ensureContrastRatio = t;
  function e(o, l, a) {
    let u = o >> 24 & 255, h = o >> 16 & 255, c = o >> 8 & 255, d = l >> 24 & 255, _ = l >> 16 & 255, p = l >> 8 & 255, m = Xe2(ve2.relativeLuminance2(d, _, p), ve2.relativeLuminance2(u, h, c));
    for (;m < a && (d > 0 || _ > 0 || p > 0); )
      d -= Math.max(0, Math.ceil(d * 0.1)), _ -= Math.max(0, Math.ceil(_ * 0.1)), p -= Math.max(0, Math.ceil(p * 0.1)), m = Xe2(ve2.relativeLuminance2(d, _, p), ve2.relativeLuminance2(u, h, c));
    return (d << 24 | _ << 16 | p << 8 | 255) >>> 0;
  }
  n.reduceLuminance = e;
  function i(o, l, a) {
    let u = o >> 24 & 255, h = o >> 16 & 255, c = o >> 8 & 255, d = l >> 24 & 255, _ = l >> 16 & 255, p = l >> 8 & 255, m = Xe2(ve2.relativeLuminance2(d, _, p), ve2.relativeLuminance2(u, h, c));
    for (;m < a && (d < 255 || _ < 255 || p < 255); )
      d = Math.min(255, d + Math.ceil((255 - d) * 0.1)), _ = Math.min(255, _ + Math.ceil((255 - _) * 0.1)), p = Math.min(255, p + Math.ceil((255 - p) * 0.1)), m = Xe2(ve2.relativeLuminance2(d, _, p), ve2.relativeLuminance2(u, h, c));
    return (d << 24 | _ << 16 | p << 8 | 255) >>> 0;
  }
  n.increaseLuminance = i;
  function r(o) {
    return [o >> 24 & 255, o >> 16 & 255, o >> 8 & 255, o & 255];
  }
  n.toChannels = r;
})($r2 ||= {});
function vt2(s) {
  let t = s.toString(16);
  return t.length < 2 ? "0" + t : t;
}
function Xe2(s, t) {
  return s < t ? (t + 0.05) / (s + 0.05) : (s + 0.05) / (t + 0.05);
}
var Vr2 = class extends De2 {
  constructor(e, i, r) {
    super();
    this.content = 0;
    this.combinedData = "";
    this.fg = e.fg, this.bg = e.bg, this.combinedData = i, this._width = r;
  }
  isCombined() {
    return 2097152;
  }
  getWidth() {
    return this._width;
  }
  getChars() {
    return this.combinedData;
  }
  getCode() {
    return 2097151;
  }
  setFromCharData(e) {
    throw new Error("not implemented");
  }
  getAsCharData() {
    return [this.fg, this.getChars(), this.getWidth(), this.getCode()];
  }
};
var ct = class {
  constructor(t) {
    this._bufferService = t;
    this._characterJoiners = [];
    this._nextCharacterJoinerId = 0;
    this._workCell = new q;
  }
  register(t) {
    let e = { id: this._nextCharacterJoinerId++, handler: t };
    return this._characterJoiners.push(e), e.id;
  }
  deregister(t) {
    for (let e = 0;e < this._characterJoiners.length; e++)
      if (this._characterJoiners[e].id === t)
        return this._characterJoiners.splice(e, 1), true;
    return false;
  }
  getJoinedCharacters(t) {
    if (this._characterJoiners.length === 0)
      return [];
    let e = this._bufferService.buffer.lines.get(t);
    if (!e || e.length === 0)
      return [];
    let i = [], r = e.translateToString(true), n = 0, o = 0, l = 0, a = e.getFg(0), u = e.getBg(0);
    for (let h = 0;h < e.getTrimmedLength(); h++)
      if (e.loadCell(h, this._workCell), this._workCell.getWidth() !== 0) {
        if (this._workCell.fg !== a || this._workCell.bg !== u) {
          if (h - n > 1) {
            let c = this._getJoinedRanges(r, l, o, e, n);
            for (let d = 0;d < c.length; d++)
              i.push(c[d]);
          }
          n = h, l = o, a = this._workCell.fg, u = this._workCell.bg;
        }
        o += this._workCell.getChars().length || we.length;
      }
    if (this._bufferService.cols - n > 1) {
      let h = this._getJoinedRanges(r, l, o, e, n);
      for (let c = 0;c < h.length; c++)
        i.push(h[c]);
    }
    return i;
  }
  _getJoinedRanges(t, e, i, r, n) {
    let o = t.substring(e, i), l = [];
    try {
      l = this._characterJoiners[0].handler(o);
    } catch (a) {
      console.error(a);
    }
    for (let a = 1;a < this._characterJoiners.length; a++)
      try {
        let u = this._characterJoiners[a].handler(o);
        for (let h = 0;h < u.length; h++)
          ct._mergeRanges(l, u[h]);
      } catch (u) {
        console.error(u);
      }
    return this._stringRangesToCellRanges(l, r, n), l;
  }
  _stringRangesToCellRanges(t, e, i) {
    let r = 0, n = false, o = 0, l = t[r];
    if (l) {
      for (let a = i;a < this._bufferService.cols; a++) {
        let u = e.getWidth(a), h = e.getString(a).length || we.length;
        if (u !== 0) {
          if (!n && l[0] <= o && (l[0] = a, n = true), l[1] <= o) {
            if (l[1] = a, l = t[++r], !l)
              break;
            l[0] <= o ? (l[0] = a, n = true) : n = false;
          }
          o += h;
        }
      }
      l && (l[1] = this._bufferService.cols);
    }
  }
  static _mergeRanges(t, e) {
    let i = false;
    for (let r = 0;r < t.length; r++) {
      let n = t[r];
      if (i) {
        if (e[1] <= n[0])
          return t[r - 1][1] = e[1], t;
        if (e[1] <= n[1])
          return t[r - 1][1] = Math.max(e[1], n[1]), t.splice(r, 1), t;
        t.splice(r, 1), r--;
      } else {
        if (e[1] <= n[0])
          return t.splice(r, 0, e), t;
        if (e[1] <= n[1])
          return n[0] = Math.min(e[0], n[0]), t;
        e[0] < n[1] && (n[0] = Math.min(e[0], n[0]), i = true);
        continue;
      }
    }
    return i ? t[t.length - 1][1] = e[1] : t.push(e), t;
  }
};
ct = M([S(0, F2)], ct);
function Oa(s) {
  return 57508 <= s && s <= 57558;
}
function Ba(s) {
  return 9472 <= s && s <= 9631;
}
function $o(s) {
  return Oa(s) || Ba(s);
}
function Vo() {
  return { css: { canvas: qr2(), cell: qr2() }, device: { canvas: qr2(), cell: qr2(), char: { width: 0, height: 0, left: 0, top: 0 } } };
}
function qr2() {
  return { width: 0, height: 0 };
}
var Vt2 = class {
  constructor(t, e, i, r, n, o, l) {
    this._document = t;
    this._characterJoinerService = e;
    this._optionsService = i;
    this._coreBrowserService = r;
    this._coreService = n;
    this._decorationService = o;
    this._themeService = l;
    this._workCell = new q;
    this._columnSelectMode = false;
    this.defaultSpacing = 0;
  }
  handleSelectionChanged(t, e, i) {
    this._selectionStart = t, this._selectionEnd = e, this._columnSelectMode = i;
  }
  createRow(t, e, i, r, n, o, l, a, u, h, c) {
    let d = [], _ = this._characterJoinerService.getJoinedCharacters(e), p = this._themeService.colors, m = t.getNoBgTrimmedLength();
    i && m < o + 1 && (m = o + 1);
    let f, A = 0, R = "", O = 0, I = 0, k = 0, P = 0, oe = false, Me = 0, Pe = false, Ke = 0, di = 0, V = [], Qe = h !== -1 && c !== -1;
    for (let y = 0;y < m; y++) {
      t.loadCell(y, this._workCell);
      let T = this._workCell.getWidth();
      if (T === 0)
        continue;
      let g = false, w = y >= di, E = y, x = this._workCell;
      if (_.length > 0 && y === _[0][0] && w) {
        let W = _.shift(), An = this._isCellInSelection(W[0], e);
        for (O = W[0] + 1;O < W[1]; O++)
          w &&= An === this._isCellInSelection(O, e);
        w &&= !i || o < W[0] || o >= W[1], w ? (g = true, x = new Vr2(this._workCell, t.translateToString(true, W[0], W[1]), W[1] - W[0]), E = W[1] - 1, T = x.getWidth()) : di = W[1];
      }
      let N = this._isCellInSelection(y, e), Z = i && y === o, te = Qe && y >= h && y <= c, Oe = false;
      this._decorationService.forEachDecorationAtCell(y, e, undefined, (W) => {
        Oe = true;
      });
      let ze = x.getChars() || we;
      if (ze === " " && (x.isUnderline() || x.isOverline()) && (ze = " "), Ke = T * a - u.get(ze, x.isBold(), x.isItalic()), !f)
        f = this._document.createElement("span");
      else if (A && (N && Pe || !N && !Pe && x.bg === I) && (N && Pe && p.selectionForeground || x.fg === k) && x.extended.ext === P && te === oe && Ke === Me && !Z && !g && !Oe && w) {
        x.isInvisible() ? R += we : R += ze, A++;
        continue;
      } else
        A && (f.textContent = R), f = this._document.createElement("span"), A = 0, R = "";
      if (I = x.bg, k = x.fg, P = x.extended.ext, oe = te, Me = Ke, Pe = N, g && o >= y && o <= E && (o = y), !this._coreService.isCursorHidden && Z && this._coreService.isCursorInitialized) {
        if (V.push("xterm-cursor"), this._coreBrowserService.isFocused)
          l && V.push("xterm-cursor-blink"), V.push(r === "bar" ? "xterm-cursor-bar" : r === "underline" ? "xterm-cursor-underline" : "xterm-cursor-block");
        else if (n)
          switch (n) {
            case "outline":
              V.push("xterm-cursor-outline");
              break;
            case "block":
              V.push("xterm-cursor-block");
              break;
            case "bar":
              V.push("xterm-cursor-bar");
              break;
            case "underline":
              V.push("xterm-cursor-underline");
              break;
            default:
              break;
          }
      }
      if (x.isBold() && V.push("xterm-bold"), x.isItalic() && V.push("xterm-italic"), x.isDim() && V.push("xterm-dim"), x.isInvisible() ? R = we : R = x.getChars() || we, x.isUnderline() && (V.push(`xterm-underline-${x.extended.underlineStyle}`), R === " " && (R = " "), !x.isUnderlineColorDefault()))
        if (x.isUnderlineColorRGB())
          f.style.textDecorationColor = `rgb(${De2.toColorRGB(x.getUnderlineColor()).join(",")})`;
        else {
          let W = x.getUnderlineColor();
          this._optionsService.rawOptions.drawBoldTextInBrightColors && x.isBold() && W < 8 && (W += 8), f.style.textDecorationColor = p.ansi[W].css;
        }
      x.isOverline() && (V.push("xterm-overline"), R === " " && (R = " ")), x.isStrikethrough() && V.push("xterm-strikethrough"), te && (f.style.textDecoration = "underline");
      let le = x.getFgColor(), et = x.getFgColorMode(), me = x.getBgColor(), ht = x.getBgColorMode(), fi = !!x.isInverse();
      if (fi) {
        let W = le;
        le = me, me = W;
        let An = et;
        et = ht, ht = An;
      }
      let tt, Qi, pi = false;
      this._decorationService.forEachDecorationAtCell(y, e, undefined, (W) => {
        W.options.layer !== "top" && pi || (W.backgroundColorRGB && (ht = 50331648, me = W.backgroundColorRGB.rgba >> 8 & 16777215, tt = W.backgroundColorRGB), W.foregroundColorRGB && (et = 50331648, le = W.foregroundColorRGB.rgba >> 8 & 16777215, Qi = W.foregroundColorRGB), pi = W.options.layer === "top");
      }), !pi && N && (tt = this._coreBrowserService.isFocused ? p.selectionBackgroundOpaque : p.selectionInactiveBackgroundOpaque, me = tt.rgba >> 8 & 16777215, ht = 50331648, pi = true, p.selectionForeground && (et = 50331648, le = p.selectionForeground.rgba >> 8 & 16777215, Qi = p.selectionForeground)), pi && V.push("xterm-decoration-top");
      let it;
      switch (ht) {
        case 16777216:
        case 33554432:
          it = p.ansi[me], V.push(`xterm-bg-${me}`);
          break;
        case 50331648:
          it = j2.toColor(me >> 16, me >> 8 & 255, me & 255), this._addStyle(f, `background-color:#${qo((me >>> 0).toString(16), "0", 6)}`);
          break;
        case 0:
        default:
          fi ? (it = p.foreground, V.push(`xterm-bg-${257}`)) : it = p.background;
      }
      switch (tt || x.isDim() && (tt = U2.multiplyOpacity(it, 0.5)), et) {
        case 16777216:
        case 33554432:
          x.isBold() && le < 8 && this._optionsService.rawOptions.drawBoldTextInBrightColors && (le += 8), this._applyMinimumContrast(f, it, p.ansi[le], x, tt, undefined) || V.push(`xterm-fg-${le}`);
          break;
        case 50331648:
          let W = j2.toColor(le >> 16 & 255, le >> 8 & 255, le & 255);
          this._applyMinimumContrast(f, it, W, x, tt, Qi) || this._addStyle(f, `color:#${qo(le.toString(16), "0", 6)}`);
          break;
        case 0:
        default:
          this._applyMinimumContrast(f, it, p.foreground, x, tt, Qi) || fi && V.push(`xterm-fg-${257}`);
      }
      V.length && (f.className = V.join(" "), V.length = 0), !Z && !g && !Oe && w ? A++ : f.textContent = R, Ke !== this.defaultSpacing && (f.style.letterSpacing = `${Ke}px`), d.push(f), y = E;
    }
    return f && A && (f.textContent = R), d;
  }
  _applyMinimumContrast(t, e, i, r, n, o) {
    if (this._optionsService.rawOptions.minimumContrastRatio === 1 || $o(r.getCode()))
      return false;
    let l = this._getContrastCache(r), a;
    if (!n && !o && (a = l.getColor(e.rgba, i.rgba)), a === undefined) {
      let u = this._optionsService.rawOptions.minimumContrastRatio / (r.isDim() ? 2 : 1);
      a = U2.ensureContrastRatio(n || e, o || i, u), l.setColor((n || e).rgba, (o || i).rgba, a ?? null);
    }
    return a ? (this._addStyle(t, `color:${a.css}`), true) : false;
  }
  _getContrastCache(t) {
    return t.isDim() ? this._themeService.colors.halfContrastCache : this._themeService.colors.contrastCache;
  }
  _addStyle(t, e) {
    t.setAttribute("style", `${t.getAttribute("style") || ""}${e};`);
  }
  _isCellInSelection(t, e) {
    let i = this._selectionStart, r = this._selectionEnd;
    return !i || !r ? false : this._columnSelectMode ? i[0] <= r[0] ? t >= i[0] && e >= i[1] && t < r[0] && e <= r[1] : t < i[0] && e >= i[1] && t >= r[0] && e <= r[1] : e > i[1] && e < r[1] || i[1] === r[1] && e === i[1] && t >= i[0] && t < r[0] || i[1] < r[1] && e === r[1] && t < r[0] || i[1] < r[1] && e === i[1] && t >= i[0];
  }
};
Vt2 = M([S(1, or2), S(2, H2), S(3, ae2), S(4, ge2), S(5, Be2), S(6, Re)], Vt2);
function qo(s, t, e) {
  for (;s.length < e; )
    s = t + s;
  return s;
}
var Yr2 = class {
  constructor(t, e) {
    this._flat = new Float32Array(256);
    this._font = "";
    this._fontSize = 0;
    this._weight = "normal";
    this._weightBold = "bold";
    this._measureElements = [];
    this._container = t.createElement("div"), this._container.classList.add("xterm-width-cache-measure-container"), this._container.setAttribute("aria-hidden", "true"), this._container.style.whiteSpace = "pre", this._container.style.fontKerning = "none";
    let i = t.createElement("span");
    i.classList.add("xterm-char-measure-element");
    let r = t.createElement("span");
    r.classList.add("xterm-char-measure-element"), r.style.fontWeight = "bold";
    let n = t.createElement("span");
    n.classList.add("xterm-char-measure-element"), n.style.fontStyle = "italic";
    let o = t.createElement("span");
    o.classList.add("xterm-char-measure-element"), o.style.fontWeight = "bold", o.style.fontStyle = "italic", this._measureElements = [i, r, n, o], this._container.appendChild(i), this._container.appendChild(r), this._container.appendChild(n), this._container.appendChild(o), e.appendChild(this._container), this.clear();
  }
  dispose() {
    this._container.remove(), this._measureElements.length = 0, this._holey = undefined;
  }
  clear() {
    this._flat.fill(-9999), this._holey = new Map;
  }
  setFont(t, e, i, r) {
    t === this._font && e === this._fontSize && i === this._weight && r === this._weightBold || (this._font = t, this._fontSize = e, this._weight = i, this._weightBold = r, this._container.style.fontFamily = this._font, this._container.style.fontSize = `${this._fontSize}px`, this._measureElements[0].style.fontWeight = `${i}`, this._measureElements[1].style.fontWeight = `${r}`, this._measureElements[2].style.fontWeight = `${i}`, this._measureElements[3].style.fontWeight = `${r}`, this.clear());
  }
  get(t, e, i) {
    let r = 0;
    if (!e && !i && t.length === 1 && (r = t.charCodeAt(0)) < 256) {
      if (this._flat[r] !== -9999)
        return this._flat[r];
      let l = this._measure(t, 0);
      return l > 0 && (this._flat[r] = l), l;
    }
    let n = t;
    e && (n += "B"), i && (n += "I");
    let o = this._holey.get(n);
    if (o === undefined) {
      let l = 0;
      e && (l |= 1), i && (l |= 2), o = this._measure(t, l), o > 0 && this._holey.set(n, o);
    }
    return o;
  }
  _measure(t, e) {
    let i = this._measureElements[e];
    return i.textContent = t.repeat(32), i.offsetWidth / 32;
  }
};
var ms2 = class {
  constructor() {
    this.clear();
  }
  clear() {
    this.hasSelection = false, this.columnSelectMode = false, this.viewportStartRow = 0, this.viewportEndRow = 0, this.viewportCappedStartRow = 0, this.viewportCappedEndRow = 0, this.startCol = 0, this.endCol = 0, this.selectionStart = undefined, this.selectionEnd = undefined;
  }
  update(t, e, i, r = false) {
    if (this.selectionStart = e, this.selectionEnd = i, !e || !i || e[0] === i[0] && e[1] === i[1]) {
      this.clear();
      return;
    }
    let n = t.buffers.active.ydisp, o = e[1] - n, l = i[1] - n, a = Math.max(o, 0), u = Math.min(l, t.rows - 1);
    if (a >= t.rows || u < 0) {
      this.clear();
      return;
    }
    this.hasSelection = true, this.columnSelectMode = r, this.viewportStartRow = o, this.viewportEndRow = l, this.viewportCappedStartRow = a, this.viewportCappedEndRow = u, this.startCol = e[0], this.endCol = i[0];
  }
  isCellSelected(t, e, i) {
    return this.hasSelection ? (i -= t.buffer.active.viewportY, this.columnSelectMode ? this.startCol <= this.endCol ? e >= this.startCol && i >= this.viewportCappedStartRow && e < this.endCol && i <= this.viewportCappedEndRow : e < this.startCol && i >= this.viewportCappedStartRow && e >= this.endCol && i <= this.viewportCappedEndRow : i > this.viewportStartRow && i < this.viewportEndRow || this.viewportStartRow === this.viewportEndRow && i === this.viewportStartRow && e >= this.startCol && e < this.endCol || this.viewportStartRow < this.viewportEndRow && i === this.viewportEndRow && e < this.endCol || this.viewportStartRow < this.viewportEndRow && i === this.viewportStartRow && e >= this.startCol) : false;
  }
};
function Yo() {
  return new ms2;
}
var _s2 = "xterm-dom-renderer-owner-";
var Le2 = "xterm-rows";
var jr2 = "xterm-fg-";
var jo = "xterm-bg-";
var ki2 = "xterm-focus";
var Xr2 = "xterm-selection";
var Na = 1;
var Yt2 = class extends D2 {
  constructor(e, i, r, n, o, l, a, u, h, c, d, _, p, m) {
    super();
    this._terminal = e;
    this._document = i;
    this._element = r;
    this._screenElement = n;
    this._viewportElement = o;
    this._helperContainer = l;
    this._linkifier2 = a;
    this._charSizeService = h;
    this._optionsService = c;
    this._bufferService = d;
    this._coreService = _;
    this._coreBrowserService = p;
    this._themeService = m;
    this._terminalClass = Na++;
    this._rowElements = [];
    this._selectionRenderModel = Yo();
    this.onRequestRedraw = this._register(new v).event;
    this._rowContainer = this._document.createElement("div"), this._rowContainer.classList.add(Le2), this._rowContainer.style.lineHeight = "normal", this._rowContainer.setAttribute("aria-hidden", "true"), this._refreshRowElements(this._bufferService.cols, this._bufferService.rows), this._selectionContainer = this._document.createElement("div"), this._selectionContainer.classList.add(Xr2), this._selectionContainer.setAttribute("aria-hidden", "true"), this.dimensions = Vo(), this._updateDimensions(), this._register(this._optionsService.onOptionChange(() => this._handleOptionsChanged())), this._register(this._themeService.onChangeColors((f) => this._injectCss(f))), this._injectCss(this._themeService.colors), this._rowFactory = u.createInstance(Vt2, document), this._element.classList.add(_s2 + this._terminalClass), this._screenElement.appendChild(this._rowContainer), this._screenElement.appendChild(this._selectionContainer), this._register(this._linkifier2.onShowLinkUnderline((f) => this._handleLinkHover(f))), this._register(this._linkifier2.onHideLinkUnderline((f) => this._handleLinkLeave(f))), this._register(C2(() => {
      this._element.classList.remove(_s2 + this._terminalClass), this._rowContainer.remove(), this._selectionContainer.remove(), this._widthCache.dispose(), this._themeStyleElement.remove(), this._dimensionsStyleElement.remove();
    })), this._widthCache = new Yr2(this._document, this._helperContainer), this._widthCache.setFont(this._optionsService.rawOptions.fontFamily, this._optionsService.rawOptions.fontSize, this._optionsService.rawOptions.fontWeight, this._optionsService.rawOptions.fontWeightBold), this._setDefaultSpacing();
  }
  _updateDimensions() {
    let e = this._coreBrowserService.dpr;
    this.dimensions.device.char.width = this._charSizeService.width * e, this.dimensions.device.char.height = Math.ceil(this._charSizeService.height * e), this.dimensions.device.cell.width = this.dimensions.device.char.width + Math.round(this._optionsService.rawOptions.letterSpacing), this.dimensions.device.cell.height = Math.floor(this.dimensions.device.char.height * this._optionsService.rawOptions.lineHeight), this.dimensions.device.char.left = 0, this.dimensions.device.char.top = 0, this.dimensions.device.canvas.width = this.dimensions.device.cell.width * this._bufferService.cols, this.dimensions.device.canvas.height = this.dimensions.device.cell.height * this._bufferService.rows, this.dimensions.css.canvas.width = Math.round(this.dimensions.device.canvas.width / e), this.dimensions.css.canvas.height = Math.round(this.dimensions.device.canvas.height / e), this.dimensions.css.cell.width = this.dimensions.css.canvas.width / this._bufferService.cols, this.dimensions.css.cell.height = this.dimensions.css.canvas.height / this._bufferService.rows;
    for (let r of this._rowElements)
      r.style.width = `${this.dimensions.css.canvas.width}px`, r.style.height = `${this.dimensions.css.cell.height}px`, r.style.lineHeight = `${this.dimensions.css.cell.height}px`, r.style.overflow = "hidden";
    this._dimensionsStyleElement || (this._dimensionsStyleElement = this._document.createElement("style"), this._screenElement.appendChild(this._dimensionsStyleElement));
    let i = `${this._terminalSelector} .${Le2} span { display: inline-block; height: 100%; vertical-align: top;}`;
    this._dimensionsStyleElement.textContent = i, this._selectionContainer.style.height = this._viewportElement.style.height, this._screenElement.style.width = `${this.dimensions.css.canvas.width}px`, this._screenElement.style.height = `${this.dimensions.css.canvas.height}px`;
  }
  _injectCss(e) {
    this._themeStyleElement || (this._themeStyleElement = this._document.createElement("style"), this._screenElement.appendChild(this._themeStyleElement));
    let i = `${this._terminalSelector} .${Le2} { pointer-events: none; color: ${e.foreground.css}; font-family: ${this._optionsService.rawOptions.fontFamily}; font-size: ${this._optionsService.rawOptions.fontSize}px; font-kerning: none; white-space: pre}`;
    i += `${this._terminalSelector} .${Le2} .xterm-dim { color: ${U2.multiplyOpacity(e.foreground, 0.5).css};}`, i += `${this._terminalSelector} span:not(.xterm-bold) { font-weight: ${this._optionsService.rawOptions.fontWeight};}${this._terminalSelector} span.xterm-bold { font-weight: ${this._optionsService.rawOptions.fontWeightBold};}${this._terminalSelector} span.xterm-italic { font-style: italic;}`;
    let r = `blink_underline_${this._terminalClass}`, n = `blink_bar_${this._terminalClass}`, o = `blink_block_${this._terminalClass}`;
    i += `@keyframes ${r} { 50% {  border-bottom-style: hidden; }}`, i += `@keyframes ${n} { 50% {  box-shadow: none; }}`, i += `@keyframes ${o} { 0% {  background-color: ${e.cursor.css};  color: ${e.cursorAccent.css}; } 50% {  background-color: inherit;  color: ${e.cursor.css}; }}`, i += `${this._terminalSelector} .${Le2}.${ki2} .xterm-cursor.xterm-cursor-blink.xterm-cursor-underline { animation: ${r} 1s step-end infinite;}${this._terminalSelector} .${Le2}.${ki2} .xterm-cursor.xterm-cursor-blink.xterm-cursor-bar { animation: ${n} 1s step-end infinite;}${this._terminalSelector} .${Le2}.${ki2} .xterm-cursor.xterm-cursor-blink.xterm-cursor-block { animation: ${o} 1s step-end infinite;}${this._terminalSelector} .${Le2} .xterm-cursor.xterm-cursor-block { background-color: ${e.cursor.css}; color: ${e.cursorAccent.css};}${this._terminalSelector} .${Le2} .xterm-cursor.xterm-cursor-block:not(.xterm-cursor-blink) { background-color: ${e.cursor.css} !important; color: ${e.cursorAccent.css} !important;}${this._terminalSelector} .${Le2} .xterm-cursor.xterm-cursor-outline { outline: 1px solid ${e.cursor.css}; outline-offset: -1px;}${this._terminalSelector} .${Le2} .xterm-cursor.xterm-cursor-bar { box-shadow: ${this._optionsService.rawOptions.cursorWidth}px 0 0 ${e.cursor.css} inset;}${this._terminalSelector} .${Le2} .xterm-cursor.xterm-cursor-underline { border-bottom: 1px ${e.cursor.css}; border-bottom-style: solid; height: calc(100% - 1px);}`, i += `${this._terminalSelector} .${Xr2} { position: absolute; top: 0; left: 0; z-index: 1; pointer-events: none;}${this._terminalSelector}.focus .${Xr2} div { position: absolute; background-color: ${e.selectionBackgroundOpaque.css};}${this._terminalSelector} .${Xr2} div { position: absolute; background-color: ${e.selectionInactiveBackgroundOpaque.css};}`;
    for (let [l, a] of e.ansi.entries())
      i += `${this._terminalSelector} .${jr2}${l} { color: ${a.css}; }${this._terminalSelector} .${jr2}${l}.xterm-dim { color: ${U2.multiplyOpacity(a, 0.5).css}; }${this._terminalSelector} .${jo}${l} { background-color: ${a.css}; }`;
    i += `${this._terminalSelector} .${jr2}${257} { color: ${U2.opaque(e.background).css}; }${this._terminalSelector} .${jr2}${257}.xterm-dim { color: ${U2.multiplyOpacity(U2.opaque(e.background), 0.5).css}; }${this._terminalSelector} .${jo}${257} { background-color: ${e.foreground.css}; }`, this._themeStyleElement.textContent = i;
  }
  _setDefaultSpacing() {
    let e = this.dimensions.css.cell.width - this._widthCache.get("W", false, false);
    this._rowContainer.style.letterSpacing = `${e}px`, this._rowFactory.defaultSpacing = e;
  }
  handleDevicePixelRatioChange() {
    this._updateDimensions(), this._widthCache.clear(), this._setDefaultSpacing();
  }
  _refreshRowElements(e, i) {
    for (let r = this._rowElements.length;r <= i; r++) {
      let n = this._document.createElement("div");
      this._rowContainer.appendChild(n), this._rowElements.push(n);
    }
    for (;this._rowElements.length > i; )
      this._rowContainer.removeChild(this._rowElements.pop());
  }
  handleResize(e, i) {
    this._refreshRowElements(e, i), this._updateDimensions(), this.handleSelectionChanged(this._selectionRenderModel.selectionStart, this._selectionRenderModel.selectionEnd, this._selectionRenderModel.columnSelectMode);
  }
  handleCharSizeChanged() {
    this._updateDimensions(), this._widthCache.clear(), this._setDefaultSpacing();
  }
  handleBlur() {
    this._rowContainer.classList.remove(ki2), this.renderRows(0, this._bufferService.rows - 1);
  }
  handleFocus() {
    this._rowContainer.classList.add(ki2), this.renderRows(this._bufferService.buffer.y, this._bufferService.buffer.y);
  }
  handleSelectionChanged(e, i, r) {
    if (this._selectionContainer.replaceChildren(), this._rowFactory.handleSelectionChanged(e, i, r), this.renderRows(0, this._bufferService.rows - 1), !e || !i || (this._selectionRenderModel.update(this._terminal, e, i, r), !this._selectionRenderModel.hasSelection))
      return;
    let n = this._selectionRenderModel.viewportStartRow, o = this._selectionRenderModel.viewportEndRow, l = this._selectionRenderModel.viewportCappedStartRow, a = this._selectionRenderModel.viewportCappedEndRow, u = this._document.createDocumentFragment();
    if (r) {
      let h = e[0] > i[0];
      u.appendChild(this._createSelectionElement(l, h ? i[0] : e[0], h ? e[0] : i[0], a - l + 1));
    } else {
      let h = n === l ? e[0] : 0, c = l === o ? i[0] : this._bufferService.cols;
      u.appendChild(this._createSelectionElement(l, h, c));
      let d = a - l - 1;
      if (u.appendChild(this._createSelectionElement(l + 1, 0, this._bufferService.cols, d)), l !== a) {
        let _ = o === a ? i[0] : this._bufferService.cols;
        u.appendChild(this._createSelectionElement(a, 0, _));
      }
    }
    this._selectionContainer.appendChild(u);
  }
  _createSelectionElement(e, i, r, n = 1) {
    let o = this._document.createElement("div"), l = i * this.dimensions.css.cell.width, a = this.dimensions.css.cell.width * (r - i);
    return l + a > this.dimensions.css.canvas.width && (a = this.dimensions.css.canvas.width - l), o.style.height = `${n * this.dimensions.css.cell.height}px`, o.style.top = `${e * this.dimensions.css.cell.height}px`, o.style.left = `${l}px`, o.style.width = `${a}px`, o;
  }
  handleCursorMove() {}
  _handleOptionsChanged() {
    this._updateDimensions(), this._injectCss(this._themeService.colors), this._widthCache.setFont(this._optionsService.rawOptions.fontFamily, this._optionsService.rawOptions.fontSize, this._optionsService.rawOptions.fontWeight, this._optionsService.rawOptions.fontWeightBold), this._setDefaultSpacing();
  }
  clear() {
    for (let e of this._rowElements)
      e.replaceChildren();
  }
  renderRows(e, i) {
    let r = this._bufferService.buffer, n = r.ybase + r.y, o = Math.min(r.x, this._bufferService.cols - 1), l = this._coreService.decPrivateModes.cursorBlink ?? this._optionsService.rawOptions.cursorBlink, a = this._coreService.decPrivateModes.cursorStyle ?? this._optionsService.rawOptions.cursorStyle, u = this._optionsService.rawOptions.cursorInactiveStyle;
    for (let h = e;h <= i; h++) {
      let c = h + r.ydisp, d = this._rowElements[h], _ = r.lines.get(c);
      if (!d || !_)
        break;
      d.replaceChildren(...this._rowFactory.createRow(_, c, c === n, a, u, o, l, this.dimensions.css.cell.width, this._widthCache, -1, -1));
    }
  }
  get _terminalSelector() {
    return `.${_s2}${this._terminalClass}`;
  }
  _handleLinkHover(e) {
    this._setCellUnderline(e.x1, e.x2, e.y1, e.y2, e.cols, true);
  }
  _handleLinkLeave(e) {
    this._setCellUnderline(e.x1, e.x2, e.y1, e.y2, e.cols, false);
  }
  _setCellUnderline(e, i, r, n, o, l) {
    r < 0 && (e = 0), n < 0 && (i = 0);
    let a = this._bufferService.rows - 1;
    r = Math.max(Math.min(r, a), 0), n = Math.max(Math.min(n, a), 0), o = Math.min(o, this._bufferService.cols);
    let u = this._bufferService.buffer, h = u.ybase + u.y, c = Math.min(u.x, o - 1), d = this._optionsService.rawOptions.cursorBlink, _ = this._optionsService.rawOptions.cursorStyle, p = this._optionsService.rawOptions.cursorInactiveStyle;
    for (let m = r;m <= n; ++m) {
      let f = m + u.ydisp, A = this._rowElements[m], R = u.lines.get(f);
      if (!A || !R)
        break;
      A.replaceChildren(...this._rowFactory.createRow(R, f, f === h, _, p, c, d, this.dimensions.css.cell.width, this._widthCache, l ? m === r ? e : 0 : -1, l ? (m === n ? i : o) - 1 : -1));
    }
  }
};
Yt2 = M([S(7, xt2), S(8, nt2), S(9, H2), S(10, F2), S(11, ge2), S(12, ae2), S(13, Re)], Yt2);
var jt2 = class extends D2 {
  constructor(e, i, r) {
    super();
    this._optionsService = r;
    this.width = 0;
    this.height = 0;
    this._onCharSizeChange = this._register(new v);
    this.onCharSizeChange = this._onCharSizeChange.event;
    try {
      this._measureStrategy = this._register(new vs2(this._optionsService));
    } catch {
      this._measureStrategy = this._register(new bs2(e, i, this._optionsService));
    }
    this._register(this._optionsService.onMultipleOptionChange(["fontFamily", "fontSize"], () => this.measure()));
  }
  get hasValidSize() {
    return this.width > 0 && this.height > 0;
  }
  measure() {
    let e = this._measureStrategy.measure();
    (e.width !== this.width || e.height !== this.height) && (this.width = e.width, this.height = e.height, this._onCharSizeChange.fire());
  }
};
jt2 = M([S(2, H2)], jt2);
var Zr = class extends D2 {
  constructor() {
    super(...arguments);
    this._result = { width: 0, height: 0 };
  }
  _validateAndSet(e, i) {
    e !== undefined && e > 0 && i !== undefined && i > 0 && (this._result.width = e, this._result.height = i);
  }
};
var bs2 = class extends Zr {
  constructor(e, i, r) {
    super();
    this._document = e;
    this._parentElement = i;
    this._optionsService = r;
    this._measureElement = this._document.createElement("span"), this._measureElement.classList.add("xterm-char-measure-element"), this._measureElement.textContent = "W".repeat(32), this._measureElement.setAttribute("aria-hidden", "true"), this._measureElement.style.whiteSpace = "pre", this._measureElement.style.fontKerning = "none", this._parentElement.appendChild(this._measureElement);
  }
  measure() {
    return this._measureElement.style.fontFamily = this._optionsService.rawOptions.fontFamily, this._measureElement.style.fontSize = `${this._optionsService.rawOptions.fontSize}px`, this._validateAndSet(Number(this._measureElement.offsetWidth) / 32, Number(this._measureElement.offsetHeight)), this._result;
  }
};
var vs2 = class extends Zr {
  constructor(e) {
    super();
    this._optionsService = e;
    this._canvas = new OffscreenCanvas(100, 100), this._ctx = this._canvas.getContext("2d");
    let i = this._ctx.measureText("W");
    if (!(("width" in i) && ("fontBoundingBoxAscent" in i) && ("fontBoundingBoxDescent" in i)))
      throw new Error("Required font metrics not supported");
  }
  measure() {
    this._ctx.font = `${this._optionsService.rawOptions.fontSize}px ${this._optionsService.rawOptions.fontFamily}`;
    let e = this._ctx.measureText("W");
    return this._validateAndSet(e.width, e.fontBoundingBoxAscent + e.fontBoundingBoxDescent), this._result;
  }
};
var Jr2 = class extends D2 {
  constructor(e, i, r) {
    super();
    this._textarea = e;
    this._window = i;
    this.mainDocument = r;
    this._isFocused = false;
    this._cachedIsFocused = undefined;
    this._screenDprMonitor = this._register(new gs2(this._window));
    this._onDprChange = this._register(new v);
    this.onDprChange = this._onDprChange.event;
    this._onWindowChange = this._register(new v);
    this.onWindowChange = this._onWindowChange.event;
    this._register(this.onWindowChange((n) => this._screenDprMonitor.setWindow(n))), this._register($.forward(this._screenDprMonitor.onDprChange, this._onDprChange)), this._register(L(this._textarea, "focus", () => this._isFocused = true)), this._register(L(this._textarea, "blur", () => this._isFocused = false));
  }
  get window() {
    return this._window;
  }
  set window(e) {
    this._window !== e && (this._window = e, this._onWindowChange.fire(this._window));
  }
  get dpr() {
    return this.window.devicePixelRatio;
  }
  get isFocused() {
    return this._cachedIsFocused === undefined && (this._cachedIsFocused = this._isFocused && this._textarea.ownerDocument.hasFocus(), queueMicrotask(() => this._cachedIsFocused = undefined)), this._cachedIsFocused;
  }
};
var gs2 = class extends D2 {
  constructor(e) {
    super();
    this._parentWindow = e;
    this._windowResizeListener = this._register(new ye2);
    this._onDprChange = this._register(new v);
    this.onDprChange = this._onDprChange.event;
    this._outerListener = () => this._setDprAndFireIfDiffers(), this._currentDevicePixelRatio = this._parentWindow.devicePixelRatio, this._updateDpr(), this._setWindowResizeListener(), this._register(C2(() => this.clearListener()));
  }
  setWindow(e) {
    this._parentWindow = e, this._setWindowResizeListener(), this._setDprAndFireIfDiffers();
  }
  _setWindowResizeListener() {
    this._windowResizeListener.value = L(this._parentWindow, "resize", () => this._setDprAndFireIfDiffers());
  }
  _setDprAndFireIfDiffers() {
    this._parentWindow.devicePixelRatio !== this._currentDevicePixelRatio && this._onDprChange.fire(this._parentWindow.devicePixelRatio), this._updateDpr();
  }
  _updateDpr() {
    this._outerListener && (this._resolutionMediaMatchList?.removeListener(this._outerListener), this._currentDevicePixelRatio = this._parentWindow.devicePixelRatio, this._resolutionMediaMatchList = this._parentWindow.matchMedia(`screen and (resolution: ${this._parentWindow.devicePixelRatio}dppx)`), this._resolutionMediaMatchList.addListener(this._outerListener));
  }
  clearListener() {
    !this._resolutionMediaMatchList || !this._outerListener || (this._resolutionMediaMatchList.removeListener(this._outerListener), this._resolutionMediaMatchList = undefined, this._outerListener = undefined);
  }
};
var Qr2 = class extends D2 {
  constructor() {
    super();
    this.linkProviders = [];
    this._register(C2(() => this.linkProviders.length = 0));
  }
  registerLinkProvider(e) {
    return this.linkProviders.push(e), { dispose: () => {
      let i = this.linkProviders.indexOf(e);
      i !== -1 && this.linkProviders.splice(i, 1);
    } };
  }
};
function Ci2(s, t, e) {
  let i = e.getBoundingClientRect(), r = s.getComputedStyle(e), n = parseInt(r.getPropertyValue("padding-left")), o = parseInt(r.getPropertyValue("padding-top"));
  return [t.clientX - i.left - n, t.clientY - i.top - o];
}
function Xo(s, t, e, i, r, n, o, l, a) {
  if (!n)
    return;
  let u = Ci2(s, t, e);
  if (u)
    return u[0] = Math.ceil((u[0] + (a ? o / 2 : 0)) / o), u[1] = Math.ceil(u[1] / l), u[0] = Math.min(Math.max(u[0], 1), i + (a ? 1 : 0)), u[1] = Math.min(Math.max(u[1], 1), r), u;
}
var Xt2 = class {
  constructor(t, e) {
    this._renderService = t;
    this._charSizeService = e;
  }
  getCoords(t, e, i, r, n) {
    return Xo(window, t, e, i, r, this._charSizeService.hasValidSize, this._renderService.dimensions.css.cell.width, this._renderService.dimensions.css.cell.height, n);
  }
  getMouseReportCoords(t, e) {
    let i = Ci2(window, t, e);
    if (this._charSizeService.hasValidSize)
      return i[0] = Math.min(Math.max(i[0], 0), this._renderService.dimensions.css.canvas.width - 1), i[1] = Math.min(Math.max(i[1], 0), this._renderService.dimensions.css.canvas.height - 1), { col: Math.floor(i[0] / this._renderService.dimensions.css.cell.width), row: Math.floor(i[1] / this._renderService.dimensions.css.cell.height), x: Math.floor(i[0]), y: Math.floor(i[1]) };
  }
};
Xt2 = M([S(0, ce), S(1, nt2)], Xt2);
var en2 = class {
  constructor(t, e) {
    this._renderCallback = t;
    this._coreBrowserService = e;
    this._refreshCallbacks = [];
  }
  dispose() {
    this._animationFrame && (this._coreBrowserService.window.cancelAnimationFrame(this._animationFrame), this._animationFrame = undefined);
  }
  addRefreshCallback(t) {
    return this._refreshCallbacks.push(t), this._animationFrame || (this._animationFrame = this._coreBrowserService.window.requestAnimationFrame(() => this._innerRefresh())), this._animationFrame;
  }
  refresh(t, e, i) {
    this._rowCount = i, t = t !== undefined ? t : 0, e = e !== undefined ? e : this._rowCount - 1, this._rowStart = this._rowStart !== undefined ? Math.min(this._rowStart, t) : t, this._rowEnd = this._rowEnd !== undefined ? Math.max(this._rowEnd, e) : e, !this._animationFrame && (this._animationFrame = this._coreBrowserService.window.requestAnimationFrame(() => this._innerRefresh()));
  }
  _innerRefresh() {
    if (this._animationFrame = undefined, this._rowStart === undefined || this._rowEnd === undefined || this._rowCount === undefined) {
      this._runRefreshCallbacks();
      return;
    }
    let t = Math.max(this._rowStart, 0), e = Math.min(this._rowEnd, this._rowCount - 1);
    this._rowStart = undefined, this._rowEnd = undefined, this._renderCallback(t, e), this._runRefreshCallbacks();
  }
  _runRefreshCallbacks() {
    for (let t of this._refreshCallbacks)
      t(0);
    this._refreshCallbacks = [];
  }
};
var tn2 = {};
Ll(tn2, { getSafariVersion: () => Ha, isChromeOS: () => Ts2, isFirefox: () => Ss2, isIpad: () => Wa2, isIphone: () => Ua, isLegacyEdge: () => Fa, isLinux: () => Bi2, isMac: () => Zt2, isNode: () => Mi2, isSafari: () => Zo, isWindows: () => Es2 });
var Mi2 = typeof process < "u" && "title" in process;
var Pi2 = Mi2 ? "node" : navigator.userAgent;
var Oi = Mi2 ? "node" : navigator.platform;
var Ss2 = Pi2.includes("Firefox");
var Fa = Pi2.includes("Edge");
var Zo = /^((?!chrome|android).)*safari/i.test(Pi2);
function Ha() {
  if (!Zo)
    return 0;
  let s = Pi2.match(/Version\/(\d+)/);
  return s === null || s.length < 2 ? 0 : parseInt(s[1]);
}
var Zt2 = ["Macintosh", "MacIntel", "MacPPC", "Mac68K"].includes(Oi);
var Wa2 = Oi === "iPad";
var Ua = Oi === "iPhone";
var Es2 = ["Windows", "Win16", "Win32", "WinCE"].includes(Oi);
var Bi2 = Oi.indexOf("Linux") >= 0;
var Ts2 = /\bCrOS\b/.test(Pi2);
var rn2 = class {
  constructor() {
    this._tasks = [];
    this._i = 0;
  }
  enqueue(t) {
    this._tasks.push(t), this._start();
  }
  flush() {
    for (;this._i < this._tasks.length; )
      this._tasks[this._i]() || this._i++;
    this.clear();
  }
  clear() {
    this._idleCallback && (this._cancelCallback(this._idleCallback), this._idleCallback = undefined), this._i = 0, this._tasks.length = 0;
  }
  _start() {
    this._idleCallback || (this._idleCallback = this._requestCallback(this._process.bind(this)));
  }
  _process(t) {
    this._idleCallback = undefined;
    let e = 0, i = 0, r = t.timeRemaining(), n = 0;
    for (;this._i < this._tasks.length; ) {
      if (e = performance.now(), this._tasks[this._i]() || this._i++, e = Math.max(1, performance.now() - e), i = Math.max(e, i), n = t.timeRemaining(), i * 1.5 > n) {
        r - e < -20 && console.warn(`task queue exceeded allotted deadline by ${Math.abs(Math.round(r - e))}ms`), this._start();
        return;
      }
      r = n;
    }
    this.clear();
  }
};
var Is2 = class extends rn2 {
  _requestCallback(t) {
    return setTimeout(() => t(this._createDeadline(16)));
  }
  _cancelCallback(t) {
    clearTimeout(t);
  }
  _createDeadline(t) {
    let e = performance.now() + t;
    return { timeRemaining: () => Math.max(0, e - performance.now()) };
  }
};
var ys = class extends rn2 {
  _requestCallback(t) {
    return requestIdleCallback(t);
  }
  _cancelCallback(t) {
    cancelIdleCallback(t);
  }
};
var Jt = !Mi2 && "requestIdleCallback" in window ? ys : Is2;
var nn2 = class {
  constructor() {
    this._queue = new Jt;
  }
  set(t) {
    this._queue.clear(), this._queue.enqueue(t);
  }
  flush() {
    this._queue.flush();
  }
};
var Qt2 = class extends D2 {
  constructor(e, i, r, n, o, l, a, u, h) {
    super();
    this._rowCount = e;
    this._optionsService = r;
    this._charSizeService = n;
    this._coreService = o;
    this._coreBrowserService = u;
    this._renderer = this._register(new ye2);
    this._pausedResizeTask = new nn2;
    this._observerDisposable = this._register(new ye2);
    this._isPaused = false;
    this._needsFullRefresh = false;
    this._isNextRenderRedrawOnly = true;
    this._needsSelectionRefresh = false;
    this._canvasWidth = 0;
    this._canvasHeight = 0;
    this._selectionState = { start: undefined, end: undefined, columnSelectMode: false };
    this._onDimensionsChange = this._register(new v);
    this.onDimensionsChange = this._onDimensionsChange.event;
    this._onRenderedViewportChange = this._register(new v);
    this.onRenderedViewportChange = this._onRenderedViewportChange.event;
    this._onRender = this._register(new v);
    this.onRender = this._onRender.event;
    this._onRefreshRequest = this._register(new v);
    this.onRefreshRequest = this._onRefreshRequest.event;
    this._renderDebouncer = new en2((c, d) => this._renderRows(c, d), this._coreBrowserService), this._register(this._renderDebouncer), this._syncOutputHandler = new xs2(this._coreBrowserService, this._coreService, () => this._fullRefresh()), this._register(C2(() => this._syncOutputHandler.dispose())), this._register(this._coreBrowserService.onDprChange(() => this.handleDevicePixelRatioChange())), this._register(a.onResize(() => this._fullRefresh())), this._register(a.buffers.onBufferActivate(() => this._renderer.value?.clear())), this._register(this._optionsService.onOptionChange(() => this._handleOptionsChanged())), this._register(this._charSizeService.onCharSizeChange(() => this.handleCharSizeChanged())), this._register(l.onDecorationRegistered(() => this._fullRefresh())), this._register(l.onDecorationRemoved(() => this._fullRefresh())), this._register(this._optionsService.onMultipleOptionChange(["customGlyphs", "drawBoldTextInBrightColors", "letterSpacing", "lineHeight", "fontFamily", "fontSize", "fontWeight", "fontWeightBold", "minimumContrastRatio", "rescaleOverlappingGlyphs"], () => {
      this.clear(), this.handleResize(a.cols, a.rows), this._fullRefresh();
    })), this._register(this._optionsService.onMultipleOptionChange(["cursorBlink", "cursorStyle"], () => this.refreshRows(a.buffer.y, a.buffer.y, true))), this._register(h.onChangeColors(() => this._fullRefresh())), this._registerIntersectionObserver(this._coreBrowserService.window, i), this._register(this._coreBrowserService.onWindowChange((c) => this._registerIntersectionObserver(c, i)));
  }
  get dimensions() {
    return this._renderer.value.dimensions;
  }
  _registerIntersectionObserver(e, i) {
    if ("IntersectionObserver" in e) {
      let r = new e.IntersectionObserver((n) => this._handleIntersectionChange(n[n.length - 1]), { threshold: 0 });
      r.observe(i), this._observerDisposable.value = C2(() => r.disconnect());
    }
  }
  _handleIntersectionChange(e) {
    this._isPaused = e.isIntersecting === undefined ? e.intersectionRatio === 0 : !e.isIntersecting, !this._isPaused && !this._charSizeService.hasValidSize && this._charSizeService.measure(), !this._isPaused && this._needsFullRefresh && (this._pausedResizeTask.flush(), this.refreshRows(0, this._rowCount - 1), this._needsFullRefresh = false);
  }
  refreshRows(e, i, r = false) {
    if (this._isPaused) {
      this._needsFullRefresh = true;
      return;
    }
    if (this._coreService.decPrivateModes.synchronizedOutput) {
      this._syncOutputHandler.bufferRows(e, i);
      return;
    }
    let n = this._syncOutputHandler.flush();
    n && (e = Math.min(e, n.start), i = Math.max(i, n.end)), r || (this._isNextRenderRedrawOnly = false), this._renderDebouncer.refresh(e, i, this._rowCount);
  }
  _renderRows(e, i) {
    if (this._renderer.value) {
      if (this._coreService.decPrivateModes.synchronizedOutput) {
        this._syncOutputHandler.bufferRows(e, i);
        return;
      }
      e = Math.min(e, this._rowCount - 1), i = Math.min(i, this._rowCount - 1), this._renderer.value.renderRows(e, i), this._needsSelectionRefresh && (this._renderer.value.handleSelectionChanged(this._selectionState.start, this._selectionState.end, this._selectionState.columnSelectMode), this._needsSelectionRefresh = false), this._isNextRenderRedrawOnly || this._onRenderedViewportChange.fire({ start: e, end: i }), this._onRender.fire({ start: e, end: i }), this._isNextRenderRedrawOnly = true;
    }
  }
  resize(e, i) {
    this._rowCount = i, this._fireOnCanvasResize();
  }
  _handleOptionsChanged() {
    this._renderer.value && (this.refreshRows(0, this._rowCount - 1), this._fireOnCanvasResize());
  }
  _fireOnCanvasResize() {
    this._renderer.value && (this._renderer.value.dimensions.css.canvas.width === this._canvasWidth && this._renderer.value.dimensions.css.canvas.height === this._canvasHeight || this._onDimensionsChange.fire(this._renderer.value.dimensions));
  }
  hasRenderer() {
    return !!this._renderer.value;
  }
  setRenderer(e) {
    this._renderer.value = e, this._renderer.value && (this._renderer.value.onRequestRedraw((i) => this.refreshRows(i.start, i.end, true)), this._needsSelectionRefresh = true, this._fullRefresh());
  }
  addRefreshCallback(e) {
    return this._renderDebouncer.addRefreshCallback(e);
  }
  _fullRefresh() {
    this._isPaused ? this._needsFullRefresh = true : this.refreshRows(0, this._rowCount - 1);
  }
  clearTextureAtlas() {
    this._renderer.value && (this._renderer.value.clearTextureAtlas?.(), this._fullRefresh());
  }
  handleDevicePixelRatioChange() {
    this._charSizeService.measure(), this._renderer.value && (this._renderer.value.handleDevicePixelRatioChange(), this.refreshRows(0, this._rowCount - 1));
  }
  handleResize(e, i) {
    this._renderer.value && (this._isPaused ? this._pausedResizeTask.set(() => this._renderer.value?.handleResize(e, i)) : this._renderer.value.handleResize(e, i), this._fullRefresh());
  }
  handleCharSizeChanged() {
    this._renderer.value?.handleCharSizeChanged();
  }
  handleBlur() {
    this._renderer.value?.handleBlur();
  }
  handleFocus() {
    this._renderer.value?.handleFocus();
  }
  handleSelectionChanged(e, i, r) {
    this._selectionState.start = e, this._selectionState.end = i, this._selectionState.columnSelectMode = r, this._renderer.value?.handleSelectionChanged(e, i, r);
  }
  handleCursorMove() {
    this._renderer.value?.handleCursorMove();
  }
  clear() {
    this._renderer.value?.clear();
  }
};
Qt2 = M([S(2, H2), S(3, nt2), S(4, ge2), S(5, Be2), S(6, F2), S(7, ae2), S(8, Re)], Qt2);
var xs2 = class {
  constructor(t, e, i) {
    this._coreBrowserService = t;
    this._coreService = e;
    this._onTimeout = i;
    this._start = 0;
    this._end = 0;
    this._isBuffering = false;
  }
  bufferRows(t, e) {
    this._isBuffering ? (this._start = Math.min(this._start, t), this._end = Math.max(this._end, e)) : (this._start = t, this._end = e, this._isBuffering = true), this._timeout === undefined && (this._timeout = this._coreBrowserService.window.setTimeout(() => {
      this._timeout = undefined, this._coreService.decPrivateModes.synchronizedOutput = false, this._onTimeout();
    }, 1000));
  }
  flush() {
    if (this._timeout !== undefined && (this._coreBrowserService.window.clearTimeout(this._timeout), this._timeout = undefined), !this._isBuffering)
      return;
    let t = { start: this._start, end: this._end };
    return this._isBuffering = false, t;
  }
  dispose() {
    this._timeout !== undefined && (this._coreBrowserService.window.clearTimeout(this._timeout), this._timeout = undefined);
  }
};
function Jo(s, t, e, i) {
  let r = e.buffer.x, n = e.buffer.y;
  if (!e.buffer.hasScrollback)
    return Ga2(r, n, s, t, e, i) + sn2(n, t, e, i) + $a2(r, n, s, t, e, i);
  let o;
  if (n === t)
    return o = r > s ? "D" : "C", Fi2(Math.abs(r - s), Ni2(o, i));
  o = n > t ? "D" : "C";
  let l = Math.abs(n - t), a = za2(n > t ? s : r, e) + (l - 1) * e.cols + 1 + Ka2(n > t ? r : s, e);
  return Fi2(a, Ni2(o, i));
}
function Ka2(s, t) {
  return s - 1;
}
function za2(s, t) {
  return t.cols - s;
}
function Ga2(s, t, e, i, r, n) {
  return sn2(t, i, r, n).length === 0 ? "" : Fi2(el(s, t, s, t - gt2(t, r), false, r).length, Ni2("D", n));
}
function sn2(s, t, e, i) {
  let r = s - gt2(s, e), n = t - gt2(t, e), o = Math.abs(r - n) - Va2(s, t, e);
  return Fi2(o, Ni2(Qo(s, t), i));
}
function $a2(s, t, e, i, r, n) {
  let o;
  sn2(t, i, r, n).length > 0 ? o = i - gt2(i, r) : o = t;
  let l = i, a = qa2(s, t, e, i, r, n);
  return Fi2(el(s, o, e, l, a === "C", r).length, Ni2(a, n));
}
function Va2(s, t, e) {
  let i = 0, r = s - gt2(s, e), n = t - gt2(t, e);
  for (let o = 0;o < Math.abs(r - n); o++) {
    let l = Qo(s, t) === "A" ? -1 : 1;
    e.buffer.lines.get(r + l * o)?.isWrapped && i++;
  }
  return i;
}
function gt2(s, t) {
  let e = 0, i = t.buffer.lines.get(s), r = i?.isWrapped;
  for (;r && s >= 0 && s < t.rows; )
    e++, i = t.buffer.lines.get(--s), r = i?.isWrapped;
  return e;
}
function qa2(s, t, e, i, r, n) {
  let o;
  return sn2(e, i, r, n).length > 0 ? o = i - gt2(i, r) : o = t, s < e && o <= i || s >= e && o < i ? "C" : "D";
}
function Qo(s, t) {
  return s > t ? "A" : "B";
}
function el(s, t, e, i, r, n) {
  let o = s, l = t, a = "";
  for (;(o !== e || l !== i) && l >= 0 && l < n.buffer.lines.length; )
    o += r ? 1 : -1, r && o > n.cols - 1 ? (a += n.buffer.translateBufferLineToString(l, false, s, o), o = 0, s = 0, l++) : !r && o < 0 && (a += n.buffer.translateBufferLineToString(l, false, 0, s + 1), o = n.cols - 1, s = o, l--);
  return a + n.buffer.translateBufferLineToString(l, false, s, o);
}
function Ni2(s, t) {
  let e = t ? "O" : "[";
  return b.ESC + e + s;
}
function Fi2(s, t) {
  s = Math.floor(s);
  let e = "";
  for (let i = 0;i < s; i++)
    e += t;
  return e;
}
var on2 = class {
  constructor(t) {
    this._bufferService = t;
    this.isSelectAllActive = false;
    this.selectionStartLength = 0;
  }
  clearSelection() {
    this.selectionStart = undefined, this.selectionEnd = undefined, this.isSelectAllActive = false, this.selectionStartLength = 0;
  }
  get finalSelectionStart() {
    return this.isSelectAllActive ? [0, 0] : !this.selectionEnd || !this.selectionStart ? this.selectionStart : this.areSelectionValuesReversed() ? this.selectionEnd : this.selectionStart;
  }
  get finalSelectionEnd() {
    if (this.isSelectAllActive)
      return [this._bufferService.cols, this._bufferService.buffer.ybase + this._bufferService.rows - 1];
    if (this.selectionStart) {
      if (!this.selectionEnd || this.areSelectionValuesReversed()) {
        let t = this.selectionStart[0] + this.selectionStartLength;
        return t > this._bufferService.cols ? t % this._bufferService.cols === 0 ? [this._bufferService.cols, this.selectionStart[1] + Math.floor(t / this._bufferService.cols) - 1] : [t % this._bufferService.cols, this.selectionStart[1] + Math.floor(t / this._bufferService.cols)] : [t, this.selectionStart[1]];
      }
      if (this.selectionStartLength && this.selectionEnd[1] === this.selectionStart[1]) {
        let t = this.selectionStart[0] + this.selectionStartLength;
        return t > this._bufferService.cols ? [t % this._bufferService.cols, this.selectionStart[1] + Math.floor(t / this._bufferService.cols)] : [Math.max(t, this.selectionEnd[0]), this.selectionEnd[1]];
      }
      return this.selectionEnd;
    }
  }
  areSelectionValuesReversed() {
    let t = this.selectionStart, e = this.selectionEnd;
    return !t || !e ? false : t[1] > e[1] || t[1] === e[1] && t[0] > e[0];
  }
  handleTrim(t) {
    return this.selectionStart && (this.selectionStart[1] -= t), this.selectionEnd && (this.selectionEnd[1] -= t), this.selectionEnd && this.selectionEnd[1] < 0 ? (this.clearSelection(), true) : (this.selectionStart && this.selectionStart[1] < 0 && (this.selectionStart[1] = 0), false);
  }
};
function ws2(s, t) {
  if (s.start.y > s.end.y)
    throw new Error(`Buffer range end (${s.end.x}, ${s.end.y}) cannot be before start (${s.start.x}, ${s.start.y})`);
  return t * (s.end.y - s.start.y) + (s.end.x - s.start.x + 1);
}
var Ds = 50;
var Ya = 15;
var ja2 = 50;
var Xa = 500;
var Za = " ";
var Ja = new RegExp(Za, "g");
var ei2 = class extends D2 {
  constructor(e, i, r, n, o, l, a, u, h) {
    super();
    this._element = e;
    this._screenElement = i;
    this._linkifier = r;
    this._bufferService = n;
    this._coreService = o;
    this._mouseService = l;
    this._optionsService = a;
    this._renderService = u;
    this._coreBrowserService = h;
    this._dragScrollAmount = 0;
    this._enabled = true;
    this._workCell = new q;
    this._mouseDownTimeStamp = 0;
    this._oldHasSelection = false;
    this._oldSelectionStart = undefined;
    this._oldSelectionEnd = undefined;
    this._onLinuxMouseSelection = this._register(new v);
    this.onLinuxMouseSelection = this._onLinuxMouseSelection.event;
    this._onRedrawRequest = this._register(new v);
    this.onRequestRedraw = this._onRedrawRequest.event;
    this._onSelectionChange = this._register(new v);
    this.onSelectionChange = this._onSelectionChange.event;
    this._onRequestScrollLines = this._register(new v);
    this.onRequestScrollLines = this._onRequestScrollLines.event;
    this._mouseMoveListener = (c) => this._handleMouseMove(c), this._mouseUpListener = (c) => this._handleMouseUp(c), this._coreService.onUserInput(() => {
      this.hasSelection && this.clearSelection();
    }), this._trimListener = this._bufferService.buffer.lines.onTrim((c) => this._handleTrim(c)), this._register(this._bufferService.buffers.onBufferActivate((c) => this._handleBufferActivate(c))), this.enable(), this._model = new on2(this._bufferService), this._activeSelectionMode = 0, this._register(C2(() => {
      this._removeMouseDownListeners();
    })), this._register(this._bufferService.onResize((c) => {
      c.rowsChanged && this.clearSelection();
    }));
  }
  reset() {
    this.clearSelection();
  }
  disable() {
    this.clearSelection(), this._enabled = false;
  }
  enable() {
    this._enabled = true;
  }
  get selectionStart() {
    return this._model.finalSelectionStart;
  }
  get selectionEnd() {
    return this._model.finalSelectionEnd;
  }
  get hasSelection() {
    let e = this._model.finalSelectionStart, i = this._model.finalSelectionEnd;
    return !e || !i ? false : e[0] !== i[0] || e[1] !== i[1];
  }
  get selectionText() {
    let e = this._model.finalSelectionStart, i = this._model.finalSelectionEnd;
    if (!e || !i)
      return "";
    let r = this._bufferService.buffer, n = [];
    if (this._activeSelectionMode === 3) {
      if (e[0] === i[0])
        return "";
      let l = e[0] < i[0] ? e[0] : i[0], a = e[0] < i[0] ? i[0] : e[0];
      for (let u = e[1];u <= i[1]; u++) {
        let h = r.translateBufferLineToString(u, true, l, a);
        n.push(h);
      }
    } else {
      let l = e[1] === i[1] ? i[0] : undefined;
      n.push(r.translateBufferLineToString(e[1], true, e[0], l));
      for (let a = e[1] + 1;a <= i[1] - 1; a++) {
        let u = r.lines.get(a), h = r.translateBufferLineToString(a, true);
        u?.isWrapped ? n[n.length - 1] += h : n.push(h);
      }
      if (e[1] !== i[1]) {
        let a = r.lines.get(i[1]), u = r.translateBufferLineToString(i[1], true, 0, i[0]);
        a && a.isWrapped ? n[n.length - 1] += u : n.push(u);
      }
    }
    return n.map((l) => l.replace(Ja, " ")).join(Es2 ? `\r
` : `
`);
  }
  clearSelection() {
    this._model.clearSelection(), this._removeMouseDownListeners(), this.refresh(), this._onSelectionChange.fire();
  }
  refresh(e) {
    this._refreshAnimationFrame || (this._refreshAnimationFrame = this._coreBrowserService.window.requestAnimationFrame(() => this._refresh())), Bi2 && e && this.selectionText.length && this._onLinuxMouseSelection.fire(this.selectionText);
  }
  _refresh() {
    this._refreshAnimationFrame = undefined, this._onRedrawRequest.fire({ start: this._model.finalSelectionStart, end: this._model.finalSelectionEnd, columnSelectMode: this._activeSelectionMode === 3 });
  }
  _isClickInSelection(e) {
    let i = this._getMouseBufferCoords(e), r = this._model.finalSelectionStart, n = this._model.finalSelectionEnd;
    return !r || !n || !i ? false : this._areCoordsInSelection(i, r, n);
  }
  isCellInSelection(e, i) {
    let r = this._model.finalSelectionStart, n = this._model.finalSelectionEnd;
    return !r || !n ? false : this._areCoordsInSelection([e, i], r, n);
  }
  _areCoordsInSelection(e, i, r) {
    return e[1] > i[1] && e[1] < r[1] || i[1] === r[1] && e[1] === i[1] && e[0] >= i[0] && e[0] < r[0] || i[1] < r[1] && e[1] === r[1] && e[0] < r[0] || i[1] < r[1] && e[1] === i[1] && e[0] >= i[0];
  }
  _selectWordAtCursor(e, i) {
    let r = this._linkifier.currentLink?.link?.range;
    if (r)
      return this._model.selectionStart = [r.start.x - 1, r.start.y - 1], this._model.selectionStartLength = ws2(r, this._bufferService.cols), this._model.selectionEnd = undefined, true;
    let n = this._getMouseBufferCoords(e);
    return n ? (this._selectWordAt(n, i), this._model.selectionEnd = undefined, true) : false;
  }
  selectAll() {
    this._model.isSelectAllActive = true, this.refresh(), this._onSelectionChange.fire();
  }
  selectLines(e, i) {
    this._model.clearSelection(), e = Math.max(e, 0), i = Math.min(i, this._bufferService.buffer.lines.length - 1), this._model.selectionStart = [0, e], this._model.selectionEnd = [this._bufferService.cols, i], this.refresh(), this._onSelectionChange.fire();
  }
  _handleTrim(e) {
    this._model.handleTrim(e) && this.refresh();
  }
  _getMouseBufferCoords(e) {
    let i = this._mouseService.getCoords(e, this._screenElement, this._bufferService.cols, this._bufferService.rows, true);
    if (i)
      return i[0]--, i[1]--, i[1] += this._bufferService.buffer.ydisp, i;
  }
  _getMouseEventScrollAmount(e) {
    let i = Ci2(this._coreBrowserService.window, e, this._screenElement)[1], r = this._renderService.dimensions.css.canvas.height;
    return i >= 0 && i <= r ? 0 : (i > r && (i -= r), i = Math.min(Math.max(i, -Ds), Ds), i /= Ds, i / Math.abs(i) + Math.round(i * (Ya - 1)));
  }
  shouldForceSelection(e) {
    return Zt2 ? e.altKey && this._optionsService.rawOptions.macOptionClickForcesSelection : e.shiftKey;
  }
  handleMouseDown(e) {
    if (this._mouseDownTimeStamp = e.timeStamp, !(e.button === 2 && this.hasSelection) && e.button === 0) {
      if (!this._enabled) {
        if (!this.shouldForceSelection(e))
          return;
        e.stopPropagation();
      }
      e.preventDefault(), this._dragScrollAmount = 0, this._enabled && e.shiftKey ? this._handleIncrementalClick(e) : e.detail === 1 ? this._handleSingleClick(e) : e.detail === 2 ? this._handleDoubleClick(e) : e.detail === 3 && this._handleTripleClick(e), this._addMouseDownListeners(), this.refresh(true);
    }
  }
  _addMouseDownListeners() {
    this._screenElement.ownerDocument && (this._screenElement.ownerDocument.addEventListener("mousemove", this._mouseMoveListener), this._screenElement.ownerDocument.addEventListener("mouseup", this._mouseUpListener)), this._dragScrollIntervalTimer = this._coreBrowserService.window.setInterval(() => this._dragScroll(), ja2);
  }
  _removeMouseDownListeners() {
    this._screenElement.ownerDocument && (this._screenElement.ownerDocument.removeEventListener("mousemove", this._mouseMoveListener), this._screenElement.ownerDocument.removeEventListener("mouseup", this._mouseUpListener)), this._coreBrowserService.window.clearInterval(this._dragScrollIntervalTimer), this._dragScrollIntervalTimer = undefined;
  }
  _handleIncrementalClick(e) {
    this._model.selectionStart && (this._model.selectionEnd = this._getMouseBufferCoords(e));
  }
  _handleSingleClick(e) {
    if (this._model.selectionStartLength = 0, this._model.isSelectAllActive = false, this._activeSelectionMode = this.shouldColumnSelect(e) ? 3 : 0, this._model.selectionStart = this._getMouseBufferCoords(e), !this._model.selectionStart)
      return;
    this._model.selectionEnd = undefined;
    let i = this._bufferService.buffer.lines.get(this._model.selectionStart[1]);
    i && i.length !== this._model.selectionStart[0] && i.hasWidth(this._model.selectionStart[0]) === 0 && this._model.selectionStart[0]++;
  }
  _handleDoubleClick(e) {
    this._selectWordAtCursor(e, true) && (this._activeSelectionMode = 1);
  }
  _handleTripleClick(e) {
    let i = this._getMouseBufferCoords(e);
    i && (this._activeSelectionMode = 2, this._selectLineAt(i[1]));
  }
  shouldColumnSelect(e) {
    return e.altKey && !(Zt2 && this._optionsService.rawOptions.macOptionClickForcesSelection);
  }
  _handleMouseMove(e) {
    if (e.stopImmediatePropagation(), !this._model.selectionStart)
      return;
    let i = this._model.selectionEnd ? [this._model.selectionEnd[0], this._model.selectionEnd[1]] : null;
    if (this._model.selectionEnd = this._getMouseBufferCoords(e), !this._model.selectionEnd) {
      this.refresh(true);
      return;
    }
    this._activeSelectionMode === 2 ? this._model.selectionEnd[1] < this._model.selectionStart[1] ? this._model.selectionEnd[0] = 0 : this._model.selectionEnd[0] = this._bufferService.cols : this._activeSelectionMode === 1 && this._selectToWordAt(this._model.selectionEnd), this._dragScrollAmount = this._getMouseEventScrollAmount(e), this._activeSelectionMode !== 3 && (this._dragScrollAmount > 0 ? this._model.selectionEnd[0] = this._bufferService.cols : this._dragScrollAmount < 0 && (this._model.selectionEnd[0] = 0));
    let r = this._bufferService.buffer;
    if (this._model.selectionEnd[1] < r.lines.length) {
      let n = r.lines.get(this._model.selectionEnd[1]);
      n && n.hasWidth(this._model.selectionEnd[0]) === 0 && this._model.selectionEnd[0] < this._bufferService.cols && this._model.selectionEnd[0]++;
    }
    (!i || i[0] !== this._model.selectionEnd[0] || i[1] !== this._model.selectionEnd[1]) && this.refresh(true);
  }
  _dragScroll() {
    if (!(!this._model.selectionEnd || !this._model.selectionStart) && this._dragScrollAmount) {
      this._onRequestScrollLines.fire({ amount: this._dragScrollAmount, suppressScrollEvent: false });
      let e = this._bufferService.buffer;
      this._dragScrollAmount > 0 ? (this._activeSelectionMode !== 3 && (this._model.selectionEnd[0] = this._bufferService.cols), this._model.selectionEnd[1] = Math.min(e.ydisp + this._bufferService.rows, e.lines.length - 1)) : (this._activeSelectionMode !== 3 && (this._model.selectionEnd[0] = 0), this._model.selectionEnd[1] = e.ydisp), this.refresh();
    }
  }
  _handleMouseUp(e) {
    let i = e.timeStamp - this._mouseDownTimeStamp;
    if (this._removeMouseDownListeners(), this.selectionText.length <= 1 && i < Xa && e.altKey && this._optionsService.rawOptions.altClickMovesCursor) {
      if (this._bufferService.buffer.ybase === this._bufferService.buffer.ydisp) {
        let r = this._mouseService.getCoords(e, this._element, this._bufferService.cols, this._bufferService.rows, false);
        if (r && r[0] !== undefined && r[1] !== undefined) {
          let n = Jo(r[0] - 1, r[1] - 1, this._bufferService, this._coreService.decPrivateModes.applicationCursorKeys);
          this._coreService.triggerDataEvent(n, true);
        }
      }
    } else
      this._fireEventIfSelectionChanged();
  }
  _fireEventIfSelectionChanged() {
    let e = this._model.finalSelectionStart, i = this._model.finalSelectionEnd, r = !!e && !!i && (e[0] !== i[0] || e[1] !== i[1]);
    if (!r) {
      this._oldHasSelection && this._fireOnSelectionChange(e, i, r);
      return;
    }
    !e || !i || (!this._oldSelectionStart || !this._oldSelectionEnd || e[0] !== this._oldSelectionStart[0] || e[1] !== this._oldSelectionStart[1] || i[0] !== this._oldSelectionEnd[0] || i[1] !== this._oldSelectionEnd[1]) && this._fireOnSelectionChange(e, i, r);
  }
  _fireOnSelectionChange(e, i, r) {
    this._oldSelectionStart = e, this._oldSelectionEnd = i, this._oldHasSelection = r, this._onSelectionChange.fire();
  }
  _handleBufferActivate(e) {
    this.clearSelection(), this._trimListener.dispose(), this._trimListener = e.activeBuffer.lines.onTrim((i) => this._handleTrim(i));
  }
  _convertViewportColToCharacterIndex(e, i) {
    let r = i;
    for (let n = 0;i >= n; n++) {
      let o = e.loadCell(n, this._workCell).getChars().length;
      this._workCell.getWidth() === 0 ? r-- : o > 1 && i !== n && (r += o - 1);
    }
    return r;
  }
  setSelection(e, i, r) {
    this._model.clearSelection(), this._removeMouseDownListeners(), this._model.selectionStart = [e, i], this._model.selectionStartLength = r, this.refresh(), this._fireEventIfSelectionChanged();
  }
  rightClickSelect(e) {
    this._isClickInSelection(e) || (this._selectWordAtCursor(e, false) && this.refresh(true), this._fireEventIfSelectionChanged());
  }
  _getWordAt(e, i, r = true, n = true) {
    if (e[0] >= this._bufferService.cols)
      return;
    let o = this._bufferService.buffer, l = o.lines.get(e[1]);
    if (!l)
      return;
    let a = o.translateBufferLineToString(e[1], false), u = this._convertViewportColToCharacterIndex(l, e[0]), h = u, c = e[0] - u, d = 0, _ = 0, p = 0, m = 0;
    if (a.charAt(u) === " ") {
      for (;u > 0 && a.charAt(u - 1) === " "; )
        u--;
      for (;h < a.length && a.charAt(h + 1) === " "; )
        h++;
    } else {
      let R = e[0], O = e[0];
      l.getWidth(R) === 0 && (d++, R--), l.getWidth(O) === 2 && (_++, O++);
      let I = l.getString(O).length;
      for (I > 1 && (m += I - 1, h += I - 1);R > 0 && u > 0 && !this._isCharWordSeparator(l.loadCell(R - 1, this._workCell)); ) {
        l.loadCell(R - 1, this._workCell);
        let k = this._workCell.getChars().length;
        this._workCell.getWidth() === 0 ? (d++, R--) : k > 1 && (p += k - 1, u -= k - 1), u--, R--;
      }
      for (;O < l.length && h + 1 < a.length && !this._isCharWordSeparator(l.loadCell(O + 1, this._workCell)); ) {
        l.loadCell(O + 1, this._workCell);
        let k = this._workCell.getChars().length;
        this._workCell.getWidth() === 2 ? (_++, O++) : k > 1 && (m += k - 1, h += k - 1), h++, O++;
      }
    }
    h++;
    let f = u + c - d + p, A = Math.min(this._bufferService.cols, h - u + d + _ - p - m);
    if (!(!i && a.slice(u, h).trim() === "")) {
      if (r && f === 0 && l.getCodePoint(0) !== 32) {
        let R = o.lines.get(e[1] - 1);
        if (R && l.isWrapped && R.getCodePoint(this._bufferService.cols - 1) !== 32) {
          let O = this._getWordAt([this._bufferService.cols - 1, e[1] - 1], false, true, false);
          if (O) {
            let I = this._bufferService.cols - O.start;
            f -= I, A += I;
          }
        }
      }
      if (n && f + A === this._bufferService.cols && l.getCodePoint(this._bufferService.cols - 1) !== 32) {
        let R = o.lines.get(e[1] + 1);
        if (R?.isWrapped && R.getCodePoint(0) !== 32) {
          let O = this._getWordAt([0, e[1] + 1], false, false, true);
          O && (A += O.length);
        }
      }
      return { start: f, length: A };
    }
  }
  _selectWordAt(e, i) {
    let r = this._getWordAt(e, i);
    if (r) {
      for (;r.start < 0; )
        r.start += this._bufferService.cols, e[1]--;
      this._model.selectionStart = [r.start, e[1]], this._model.selectionStartLength = r.length;
    }
  }
  _selectToWordAt(e) {
    let i = this._getWordAt(e, true);
    if (i) {
      let r = e[1];
      for (;i.start < 0; )
        i.start += this._bufferService.cols, r--;
      if (!this._model.areSelectionValuesReversed())
        for (;i.start + i.length > this._bufferService.cols; )
          i.length -= this._bufferService.cols, r++;
      this._model.selectionEnd = [this._model.areSelectionValuesReversed() ? i.start : i.start + i.length, r];
    }
  }
  _isCharWordSeparator(e) {
    return e.getWidth() === 0 ? false : this._optionsService.rawOptions.wordSeparator.indexOf(e.getChars()) >= 0;
  }
  _selectLineAt(e) {
    let i = this._bufferService.buffer.getWrappedRangeForLine(e), r = { start: { x: 0, y: i.first }, end: { x: this._bufferService.cols - 1, y: i.last } };
    this._model.selectionStart = [0, i.first], this._model.selectionEnd = undefined, this._model.selectionStartLength = ws2(r, this._bufferService.cols);
  }
};
ei2 = M([S(3, F2), S(4, ge2), S(5, Dt2), S(6, H2), S(7, ce), S(8, ae2)], ei2);
var Hi2 = class {
  constructor() {
    this._data = {};
  }
  set(t, e, i) {
    this._data[t] || (this._data[t] = {}), this._data[t][e] = i;
  }
  get(t, e) {
    return this._data[t] ? this._data[t][e] : undefined;
  }
  clear() {
    this._data = {};
  }
};
var Wi2 = class {
  constructor() {
    this._color = new Hi2;
    this._css = new Hi2;
  }
  setCss(t, e, i) {
    this._css.set(t, e, i);
  }
  getCss(t, e) {
    return this._css.get(t, e);
  }
  setColor(t, e, i) {
    this._color.set(t, e, i);
  }
  getColor(t, e) {
    return this._color.get(t, e);
  }
  clear() {
    this._color.clear(), this._css.clear();
  }
};
var re2 = Object.freeze((() => {
  let s = [z2.toColor("#2e3436"), z2.toColor("#cc0000"), z2.toColor("#4e9a06"), z2.toColor("#c4a000"), z2.toColor("#3465a4"), z2.toColor("#75507b"), z2.toColor("#06989a"), z2.toColor("#d3d7cf"), z2.toColor("#555753"), z2.toColor("#ef2929"), z2.toColor("#8ae234"), z2.toColor("#fce94f"), z2.toColor("#729fcf"), z2.toColor("#ad7fa8"), z2.toColor("#34e2e2"), z2.toColor("#eeeeec")], t = [0, 95, 135, 175, 215, 255];
  for (let e = 0;e < 216; e++) {
    let i = t[e / 36 % 6 | 0], r = t[e / 6 % 6 | 0], n = t[e % 6];
    s.push({ css: j2.toCss(i, r, n), rgba: j2.toRgba(i, r, n) });
  }
  for (let e = 0;e < 24; e++) {
    let i = 8 + e * 10;
    s.push({ css: j2.toCss(i, i, i), rgba: j2.toRgba(i, i, i) });
  }
  return s;
})());
var St2 = z2.toColor("#ffffff");
var Ki2 = z2.toColor("#000000");
var tl = z2.toColor("#ffffff");
var il = Ki2;
var Ui2 = { css: "rgba(255, 255, 255, 0.3)", rgba: 4294967117 };
var Qa = St2;
var ti2 = class extends D2 {
  constructor(e) {
    super();
    this._optionsService = e;
    this._contrastCache = new Wi2;
    this._halfContrastCache = new Wi2;
    this._onChangeColors = this._register(new v);
    this.onChangeColors = this._onChangeColors.event;
    this._colors = { foreground: St2, background: Ki2, cursor: tl, cursorAccent: il, selectionForeground: undefined, selectionBackgroundTransparent: Ui2, selectionBackgroundOpaque: U2.blend(Ki2, Ui2), selectionInactiveBackgroundTransparent: Ui2, selectionInactiveBackgroundOpaque: U2.blend(Ki2, Ui2), scrollbarSliderBackground: U2.opacity(St2, 0.2), scrollbarSliderHoverBackground: U2.opacity(St2, 0.4), scrollbarSliderActiveBackground: U2.opacity(St2, 0.5), overviewRulerBorder: St2, ansi: re2.slice(), contrastCache: this._contrastCache, halfContrastCache: this._halfContrastCache }, this._updateRestoreColors(), this._setTheme(this._optionsService.rawOptions.theme), this._register(this._optionsService.onSpecificOptionChange("minimumContrastRatio", () => this._contrastCache.clear())), this._register(this._optionsService.onSpecificOptionChange("theme", () => this._setTheme(this._optionsService.rawOptions.theme)));
  }
  get colors() {
    return this._colors;
  }
  _setTheme(e = {}) {
    let i = this._colors;
    if (i.foreground = K2(e.foreground, St2), i.background = K2(e.background, Ki2), i.cursor = U2.blend(i.background, K2(e.cursor, tl)), i.cursorAccent = U2.blend(i.background, K2(e.cursorAccent, il)), i.selectionBackgroundTransparent = K2(e.selectionBackground, Ui2), i.selectionBackgroundOpaque = U2.blend(i.background, i.selectionBackgroundTransparent), i.selectionInactiveBackgroundTransparent = K2(e.selectionInactiveBackground, i.selectionBackgroundTransparent), i.selectionInactiveBackgroundOpaque = U2.blend(i.background, i.selectionInactiveBackgroundTransparent), i.selectionForeground = e.selectionForeground ? K2(e.selectionForeground, ps2) : undefined, i.selectionForeground === ps2 && (i.selectionForeground = undefined), U2.isOpaque(i.selectionBackgroundTransparent) && (i.selectionBackgroundTransparent = U2.opacity(i.selectionBackgroundTransparent, 0.3)), U2.isOpaque(i.selectionInactiveBackgroundTransparent) && (i.selectionInactiveBackgroundTransparent = U2.opacity(i.selectionInactiveBackgroundTransparent, 0.3)), i.scrollbarSliderBackground = K2(e.scrollbarSliderBackground, U2.opacity(i.foreground, 0.2)), i.scrollbarSliderHoverBackground = K2(e.scrollbarSliderHoverBackground, U2.opacity(i.foreground, 0.4)), i.scrollbarSliderActiveBackground = K2(e.scrollbarSliderActiveBackground, U2.opacity(i.foreground, 0.5)), i.overviewRulerBorder = K2(e.overviewRulerBorder, Qa), i.ansi = re2.slice(), i.ansi[0] = K2(e.black, re2[0]), i.ansi[1] = K2(e.red, re2[1]), i.ansi[2] = K2(e.green, re2[2]), i.ansi[3] = K2(e.yellow, re2[3]), i.ansi[4] = K2(e.blue, re2[4]), i.ansi[5] = K2(e.magenta, re2[5]), i.ansi[6] = K2(e.cyan, re2[6]), i.ansi[7] = K2(e.white, re2[7]), i.ansi[8] = K2(e.brightBlack, re2[8]), i.ansi[9] = K2(e.brightRed, re2[9]), i.ansi[10] = K2(e.brightGreen, re2[10]), i.ansi[11] = K2(e.brightYellow, re2[11]), i.ansi[12] = K2(e.brightBlue, re2[12]), i.ansi[13] = K2(e.brightMagenta, re2[13]), i.ansi[14] = K2(e.brightCyan, re2[14]), i.ansi[15] = K2(e.brightWhite, re2[15]), e.extendedAnsi) {
      let r = Math.min(i.ansi.length - 16, e.extendedAnsi.length);
      for (let n = 0;n < r; n++)
        i.ansi[n + 16] = K2(e.extendedAnsi[n], re2[n + 16]);
    }
    this._contrastCache.clear(), this._halfContrastCache.clear(), this._updateRestoreColors(), this._onChangeColors.fire(this.colors);
  }
  restoreColor(e) {
    this._restoreColor(e), this._onChangeColors.fire(this.colors);
  }
  _restoreColor(e) {
    if (e === undefined) {
      for (let i = 0;i < this._restoreColors.ansi.length; ++i)
        this._colors.ansi[i] = this._restoreColors.ansi[i];
      return;
    }
    switch (e) {
      case 256:
        this._colors.foreground = this._restoreColors.foreground;
        break;
      case 257:
        this._colors.background = this._restoreColors.background;
        break;
      case 258:
        this._colors.cursor = this._restoreColors.cursor;
        break;
      default:
        this._colors.ansi[e] = this._restoreColors.ansi[e];
    }
  }
  modifyColors(e) {
    e(this._colors), this._onChangeColors.fire(this.colors);
  }
  _updateRestoreColors() {
    this._restoreColors = { foreground: this._colors.foreground, background: this._colors.background, cursor: this._colors.cursor, ansi: this._colors.ansi.slice() };
  }
};
ti2 = M([S(0, H2)], ti2);
function K2(s, t) {
  if (s !== undefined)
    try {
      return z2.toColor(s);
    } catch {}
  return t;
}
var Rs2 = class {
  constructor(...t) {
    this._entries = new Map;
    for (let [e, i] of t)
      this.set(e, i);
  }
  set(t, e) {
    let i = this._entries.get(t);
    return this._entries.set(t, e), i;
  }
  forEach(t) {
    for (let [e, i] of this._entries.entries())
      t(e, i);
  }
  has(t) {
    return this._entries.has(t);
  }
  get(t) {
    return this._entries.get(t);
  }
};
var ln = class {
  constructor() {
    this._services = new Rs2;
    this._services.set(xt2, this);
  }
  setService(t, e) {
    this._services.set(t, e);
  }
  getService(t) {
    return this._services.get(t);
  }
  createInstance(t, ...e) {
    let i = Xs(t).sort((o, l) => o.index - l.index), r = [];
    for (let o of i) {
      let l = this._services.get(o.id);
      if (!l)
        throw new Error(`[createInstance] ${t.name} depends on UNKNOWN service ${o.id._id}.`);
      r.push(l);
    }
    let n = i.length > 0 ? i[0].index : e.length;
    if (e.length !== n)
      throw new Error(`[createInstance] First service dependency of ${t.name} at position ${n + 1} conflicts with ${e.length} static arguments`);
    return new t(...e, ...r);
  }
};
var ec = { trace: 0, debug: 1, info: 2, warn: 3, error: 4, off: 5 };
var tc = "xterm.js: ";
var ii2 = class extends D2 {
  constructor(e) {
    super();
    this._optionsService = e;
    this._logLevel = 5;
    this._updateLogLevel(), this._register(this._optionsService.onSpecificOptionChange("logLevel", () => this._updateLogLevel())), ic = this;
  }
  get logLevel() {
    return this._logLevel;
  }
  _updateLogLevel() {
    this._logLevel = ec[this._optionsService.rawOptions.logLevel];
  }
  _evalLazyOptionalParams(e) {
    for (let i = 0;i < e.length; i++)
      typeof e[i] == "function" && (e[i] = e[i]());
  }
  _log(e, i, r) {
    this._evalLazyOptionalParams(r), e.call(console, (this._optionsService.options.logger ? "" : tc) + i, ...r);
  }
  trace(e, ...i) {
    this._logLevel <= 0 && this._log(this._optionsService.options.logger?.trace.bind(this._optionsService.options.logger) ?? console.log, e, i);
  }
  debug(e, ...i) {
    this._logLevel <= 1 && this._log(this._optionsService.options.logger?.debug.bind(this._optionsService.options.logger) ?? console.log, e, i);
  }
  info(e, ...i) {
    this._logLevel <= 2 && this._log(this._optionsService.options.logger?.info.bind(this._optionsService.options.logger) ?? console.info, e, i);
  }
  warn(e, ...i) {
    this._logLevel <= 3 && this._log(this._optionsService.options.logger?.warn.bind(this._optionsService.options.logger) ?? console.warn, e, i);
  }
  error(e, ...i) {
    this._logLevel <= 4 && this._log(this._optionsService.options.logger?.error.bind(this._optionsService.options.logger) ?? console.error, e, i);
  }
};
ii2 = M([S(0, H2)], ii2);
var ic;
var zi2 = class extends D2 {
  constructor(e) {
    super();
    this._maxLength = e;
    this.onDeleteEmitter = this._register(new v);
    this.onDelete = this.onDeleteEmitter.event;
    this.onInsertEmitter = this._register(new v);
    this.onInsert = this.onInsertEmitter.event;
    this.onTrimEmitter = this._register(new v);
    this.onTrim = this.onTrimEmitter.event;
    this._array = new Array(this._maxLength), this._startIndex = 0, this._length = 0;
  }
  get maxLength() {
    return this._maxLength;
  }
  set maxLength(e) {
    if (this._maxLength === e)
      return;
    let i = new Array(e);
    for (let r = 0;r < Math.min(e, this.length); r++)
      i[r] = this._array[this._getCyclicIndex(r)];
    this._array = i, this._maxLength = e, this._startIndex = 0;
  }
  get length() {
    return this._length;
  }
  set length(e) {
    if (e > this._length)
      for (let i = this._length;i < e; i++)
        this._array[i] = undefined;
    this._length = e;
  }
  get(e) {
    return this._array[this._getCyclicIndex(e)];
  }
  set(e, i) {
    this._array[this._getCyclicIndex(e)] = i;
  }
  push(e) {
    this._array[this._getCyclicIndex(this._length)] = e, this._length === this._maxLength ? (this._startIndex = ++this._startIndex % this._maxLength, this.onTrimEmitter.fire(1)) : this._length++;
  }
  recycle() {
    if (this._length !== this._maxLength)
      throw new Error("Can only recycle when the buffer is full");
    return this._startIndex = ++this._startIndex % this._maxLength, this.onTrimEmitter.fire(1), this._array[this._getCyclicIndex(this._length - 1)];
  }
  get isFull() {
    return this._length === this._maxLength;
  }
  pop() {
    return this._array[this._getCyclicIndex(this._length-- - 1)];
  }
  splice(e, i, ...r) {
    if (i) {
      for (let n = e;n < this._length - i; n++)
        this._array[this._getCyclicIndex(n)] = this._array[this._getCyclicIndex(n + i)];
      this._length -= i, this.onDeleteEmitter.fire({ index: e, amount: i });
    }
    for (let n = this._length - 1;n >= e; n--)
      this._array[this._getCyclicIndex(n + r.length)] = this._array[this._getCyclicIndex(n)];
    for (let n = 0;n < r.length; n++)
      this._array[this._getCyclicIndex(e + n)] = r[n];
    if (r.length && this.onInsertEmitter.fire({ index: e, amount: r.length }), this._length + r.length > this._maxLength) {
      let n = this._length + r.length - this._maxLength;
      this._startIndex += n, this._length = this._maxLength, this.onTrimEmitter.fire(n);
    } else
      this._length += r.length;
  }
  trimStart(e) {
    e > this._length && (e = this._length), this._startIndex += e, this._length -= e, this.onTrimEmitter.fire(e);
  }
  shiftElements(e, i, r) {
    if (!(i <= 0)) {
      if (e < 0 || e >= this._length)
        throw new Error("start argument out of range");
      if (e + r < 0)
        throw new Error("Cannot shift elements in list beyond index 0");
      if (r > 0) {
        for (let o = i - 1;o >= 0; o--)
          this.set(e + o + r, this.get(e + o));
        let n = e + i + r - this._length;
        if (n > 0)
          for (this._length += n;this._length > this._maxLength; )
            this._length--, this._startIndex++, this.onTrimEmitter.fire(1);
      } else
        for (let n = 0;n < i; n++)
          this.set(e + n + r, this.get(e + n));
    }
  }
  _getCyclicIndex(e) {
    return (this._startIndex + e) % this._maxLength;
  }
};
var B2 = 3;
var X2 = Object.freeze(new De2);
var an2 = 0;
var Ls2 = 2;
var Ze2 = class s {
  constructor(t, e, i = false) {
    this.isWrapped = i;
    this._combined = {};
    this._extendedAttrs = {};
    this._data = new Uint32Array(t * B2);
    let r = e || q.fromCharData([0, ir, 1, 0]);
    for (let n = 0;n < t; ++n)
      this.setCell(n, r);
    this.length = t;
  }
  get(t) {
    let e = this._data[t * B2 + 0], i = e & 2097151;
    return [this._data[t * B2 + 1], e & 2097152 ? this._combined[t] : i ? Ce2(i) : "", e >> 22, e & 2097152 ? this._combined[t].charCodeAt(this._combined[t].length - 1) : i];
  }
  set(t, e) {
    this._data[t * B2 + 1] = e[0], e[1].length > 1 ? (this._combined[t] = e[1], this._data[t * B2 + 0] = t | 2097152 | e[2] << 22) : this._data[t * B2 + 0] = e[1].charCodeAt(0) | e[2] << 22;
  }
  getWidth(t) {
    return this._data[t * B2 + 0] >> 22;
  }
  hasWidth(t) {
    return this._data[t * B2 + 0] & 12582912;
  }
  getFg(t) {
    return this._data[t * B2 + 1];
  }
  getBg(t) {
    return this._data[t * B2 + 2];
  }
  hasContent(t) {
    return this._data[t * B2 + 0] & 4194303;
  }
  getCodePoint(t) {
    let e = this._data[t * B2 + 0];
    return e & 2097152 ? this._combined[t].charCodeAt(this._combined[t].length - 1) : e & 2097151;
  }
  isCombined(t) {
    return this._data[t * B2 + 0] & 2097152;
  }
  getString(t) {
    let e = this._data[t * B2 + 0];
    return e & 2097152 ? this._combined[t] : e & 2097151 ? Ce2(e & 2097151) : "";
  }
  isProtected(t) {
    return this._data[t * B2 + 2] & 536870912;
  }
  loadCell(t, e) {
    return an2 = t * B2, e.content = this._data[an2 + 0], e.fg = this._data[an2 + 1], e.bg = this._data[an2 + 2], e.content & 2097152 && (e.combinedData = this._combined[t]), e.bg & 268435456 && (e.extended = this._extendedAttrs[t]), e;
  }
  setCell(t, e) {
    e.content & 2097152 && (this._combined[t] = e.combinedData), e.bg & 268435456 && (this._extendedAttrs[t] = e.extended), this._data[t * B2 + 0] = e.content, this._data[t * B2 + 1] = e.fg, this._data[t * B2 + 2] = e.bg;
  }
  setCellFromCodepoint(t, e, i, r) {
    r.bg & 268435456 && (this._extendedAttrs[t] = r.extended), this._data[t * B2 + 0] = e | i << 22, this._data[t * B2 + 1] = r.fg, this._data[t * B2 + 2] = r.bg;
  }
  addCodepointToCell(t, e, i) {
    let r = this._data[t * B2 + 0];
    r & 2097152 ? this._combined[t] += Ce2(e) : r & 2097151 ? (this._combined[t] = Ce2(r & 2097151) + Ce2(e), r &= -2097152, r |= 2097152) : r = e | 1 << 22, i && (r &= -12582913, r |= i << 22), this._data[t * B2 + 0] = r;
  }
  insertCells(t, e, i) {
    if (t %= this.length, t && this.getWidth(t - 1) === 2 && this.setCellFromCodepoint(t - 1, 0, 1, i), e < this.length - t) {
      let r = new q;
      for (let n = this.length - t - e - 1;n >= 0; --n)
        this.setCell(t + e + n, this.loadCell(t + n, r));
      for (let n = 0;n < e; ++n)
        this.setCell(t + n, i);
    } else
      for (let r = t;r < this.length; ++r)
        this.setCell(r, i);
    this.getWidth(this.length - 1) === 2 && this.setCellFromCodepoint(this.length - 1, 0, 1, i);
  }
  deleteCells(t, e, i) {
    if (t %= this.length, e < this.length - t) {
      let r = new q;
      for (let n = 0;n < this.length - t - e; ++n)
        this.setCell(t + n, this.loadCell(t + e + n, r));
      for (let n = this.length - e;n < this.length; ++n)
        this.setCell(n, i);
    } else
      for (let r = t;r < this.length; ++r)
        this.setCell(r, i);
    t && this.getWidth(t - 1) === 2 && this.setCellFromCodepoint(t - 1, 0, 1, i), this.getWidth(t) === 0 && !this.hasContent(t) && this.setCellFromCodepoint(t, 0, 1, i);
  }
  replaceCells(t, e, i, r = false) {
    if (r) {
      for (t && this.getWidth(t - 1) === 2 && !this.isProtected(t - 1) && this.setCellFromCodepoint(t - 1, 0, 1, i), e < this.length && this.getWidth(e - 1) === 2 && !this.isProtected(e) && this.setCellFromCodepoint(e, 0, 1, i);t < e && t < this.length; )
        this.isProtected(t) || this.setCell(t, i), t++;
      return;
    }
    for (t && this.getWidth(t - 1) === 2 && this.setCellFromCodepoint(t - 1, 0, 1, i), e < this.length && this.getWidth(e - 1) === 2 && this.setCellFromCodepoint(e, 0, 1, i);t < e && t < this.length; )
      this.setCell(t++, i);
  }
  resize(t, e) {
    if (t === this.length)
      return this._data.length * 4 * Ls2 < this._data.buffer.byteLength;
    let i = t * B2;
    if (t > this.length) {
      if (this._data.buffer.byteLength >= i * 4)
        this._data = new Uint32Array(this._data.buffer, 0, i);
      else {
        let r = new Uint32Array(i);
        r.set(this._data), this._data = r;
      }
      for (let r = this.length;r < t; ++r)
        this.setCell(r, e);
    } else {
      this._data = this._data.subarray(0, i);
      let r = Object.keys(this._combined);
      for (let o = 0;o < r.length; o++) {
        let l = parseInt(r[o], 10);
        l >= t && delete this._combined[l];
      }
      let n = Object.keys(this._extendedAttrs);
      for (let o = 0;o < n.length; o++) {
        let l = parseInt(n[o], 10);
        l >= t && delete this._extendedAttrs[l];
      }
    }
    return this.length = t, i * 4 * Ls2 < this._data.buffer.byteLength;
  }
  cleanupMemory() {
    if (this._data.length * 4 * Ls2 < this._data.buffer.byteLength) {
      let t = new Uint32Array(this._data.length);
      return t.set(this._data), this._data = t, 1;
    }
    return 0;
  }
  fill(t, e = false) {
    if (e) {
      for (let i = 0;i < this.length; ++i)
        this.isProtected(i) || this.setCell(i, t);
      return;
    }
    this._combined = {}, this._extendedAttrs = {};
    for (let i = 0;i < this.length; ++i)
      this.setCell(i, t);
  }
  copyFrom(t) {
    this.length !== t.length ? this._data = new Uint32Array(t._data) : this._data.set(t._data), this.length = t.length, this._combined = {};
    for (let e in t._combined)
      this._combined[e] = t._combined[e];
    this._extendedAttrs = {};
    for (let e in t._extendedAttrs)
      this._extendedAttrs[e] = t._extendedAttrs[e];
    this.isWrapped = t.isWrapped;
  }
  clone() {
    let t = new s(0);
    t._data = new Uint32Array(this._data), t.length = this.length;
    for (let e in this._combined)
      t._combined[e] = this._combined[e];
    for (let e in this._extendedAttrs)
      t._extendedAttrs[e] = this._extendedAttrs[e];
    return t.isWrapped = this.isWrapped, t;
  }
  getTrimmedLength() {
    for (let t = this.length - 1;t >= 0; --t)
      if (this._data[t * B2 + 0] & 4194303)
        return t + (this._data[t * B2 + 0] >> 22);
    return 0;
  }
  getNoBgTrimmedLength() {
    for (let t = this.length - 1;t >= 0; --t)
      if (this._data[t * B2 + 0] & 4194303 || this._data[t * B2 + 2] & 50331648)
        return t + (this._data[t * B2 + 0] >> 22);
    return 0;
  }
  copyCellsFrom(t, e, i, r, n) {
    let o = t._data;
    if (n)
      for (let a = r - 1;a >= 0; a--) {
        for (let u = 0;u < B2; u++)
          this._data[(i + a) * B2 + u] = o[(e + a) * B2 + u];
        o[(e + a) * B2 + 2] & 268435456 && (this._extendedAttrs[i + a] = t._extendedAttrs[e + a]);
      }
    else
      for (let a = 0;a < r; a++) {
        for (let u = 0;u < B2; u++)
          this._data[(i + a) * B2 + u] = o[(e + a) * B2 + u];
        o[(e + a) * B2 + 2] & 268435456 && (this._extendedAttrs[i + a] = t._extendedAttrs[e + a]);
      }
    let l = Object.keys(t._combined);
    for (let a = 0;a < l.length; a++) {
      let u = parseInt(l[a], 10);
      u >= e && (this._combined[u - e + i] = t._combined[u]);
    }
  }
  translateToString(t, e, i, r) {
    e = e ?? 0, i = i ?? this.length, t && (i = Math.min(i, this.getTrimmedLength())), r && (r.length = 0);
    let n = "";
    for (;e < i; ) {
      let o = this._data[e * B2 + 0], l = o & 2097151, a = o & 2097152 ? this._combined[e] : l ? Ce2(l) : we;
      if (n += a, r)
        for (let u = 0;u < a.length; ++u)
          r.push(e);
      e += o >> 22 || 1;
    }
    return r && r.push(e), n;
  }
};
function sl(s, t, e, i, r, n) {
  let o = [];
  for (let l = 0;l < s.length - 1; l++) {
    let a = l, u = s.get(++a);
    if (!u.isWrapped)
      continue;
    let h = [s.get(l)];
    for (;a < s.length && u.isWrapped; )
      h.push(u), u = s.get(++a);
    if (!n && i >= l && i < a) {
      l += h.length - 1;
      continue;
    }
    let c = 0, d = ri2(h, c, t), _ = 1, p = 0;
    for (;_ < h.length; ) {
      let f = ri2(h, _, t), A = f - p, R = e - d, O = Math.min(A, R);
      h[c].copyCellsFrom(h[_], p, d, O, false), d += O, d === e && (c++, d = 0), p += O, p === f && (_++, p = 0), d === 0 && c !== 0 && h[c - 1].getWidth(e - 1) === 2 && (h[c].copyCellsFrom(h[c - 1], e - 1, d++, 1, false), h[c - 1].setCell(e - 1, r));
    }
    h[c].replaceCells(d, e, r);
    let m = 0;
    for (let f = h.length - 1;f > 0 && (f > c || h[f].getTrimmedLength() === 0); f--)
      m++;
    m > 0 && (o.push(l + h.length - m), o.push(m)), l += h.length - 1;
  }
  return o;
}
function ol(s, t) {
  let e = [], i = 0, r = t[i], n = 0;
  for (let o = 0;o < s.length; o++)
    if (r === o) {
      let l = t[++i];
      s.onDeleteEmitter.fire({ index: o - n, amount: l }), o += l - 1, n += l, r = t[++i];
    } else
      e.push(o);
  return { layout: e, countRemoved: n };
}
function ll(s, t) {
  let e = [];
  for (let i = 0;i < t.length; i++)
    e.push(s.get(t[i]));
  for (let i = 0;i < e.length; i++)
    s.set(i, e[i]);
  s.length = t.length;
}
function al(s, t, e) {
  let i = [], r = s.map((a, u) => ri2(s, u, t)).reduce((a, u) => a + u), n = 0, o = 0, l = 0;
  for (;l < r; ) {
    if (r - l < e) {
      i.push(r - l);
      break;
    }
    n += e;
    let a = ri2(s, o, t);
    n > a && (n -= a, o++);
    let u = s[o].getWidth(n - 1) === 2;
    u && n--;
    let h = u ? e - 1 : e;
    i.push(h), l += h;
  }
  return i;
}
function ri2(s, t, e) {
  if (t === s.length - 1)
    return s[t].getTrimmedLength();
  let i = !s[t].hasContent(e - 1) && s[t].getWidth(e - 1) === 1, r = s[t + 1].getWidth(0) === 2;
  return i && r ? e - 1 : e;
}
var un2 = class un {
  constructor(t) {
    this.line = t;
    this.isDisposed = false;
    this._disposables = [];
    this._id = un._nextId++;
    this._onDispose = this.register(new v);
    this.onDispose = this._onDispose.event;
  }
  get id() {
    return this._id;
  }
  dispose() {
    this.isDisposed || (this.isDisposed = true, this.line = -1, this._onDispose.fire(), Ne2(this._disposables), this._disposables.length = 0);
  }
  register(t) {
    return this._disposables.push(t), t;
  }
};
un2._nextId = 1;
var cn2 = un2;
var ne = {};
var Je = ne.B;
ne[0] = { "`": "◆", a: "▒", b: "␉", c: "␌", d: "␍", e: "␊", f: "°", g: "±", h: "␤", i: "␋", j: "┘", k: "┐", l: "┌", m: "└", n: "┼", o: "⎺", p: "⎻", q: "─", r: "⎼", s: "⎽", t: "├", u: "┤", v: "┴", w: "┬", x: "│", y: "≤", z: "≥", "{": "π", "|": "≠", "}": "£", "~": "·" };
ne.A = { "#": "£" };
ne.B = undefined;
ne[4] = { "#": "£", "@": "¾", "[": "ij", "\\": "½", "]": "|", "{": "¨", "|": "f", "}": "¼", "~": "´" };
ne.C = ne[5] = { "[": "Ä", "\\": "Ö", "]": "Å", "^": "Ü", "`": "é", "{": "ä", "|": "ö", "}": "å", "~": "ü" };
ne.R = { "#": "£", "@": "à", "[": "°", "\\": "ç", "]": "§", "{": "é", "|": "ù", "}": "è", "~": "¨" };
ne.Q = { "@": "à", "[": "â", "\\": "ç", "]": "ê", "^": "î", "`": "ô", "{": "é", "|": "ù", "}": "è", "~": "û" };
ne.K = { "@": "§", "[": "Ä", "\\": "Ö", "]": "Ü", "{": "ä", "|": "ö", "}": "ü", "~": "ß" };
ne.Y = { "#": "£", "@": "§", "[": "°", "\\": "ç", "]": "é", "`": "ù", "{": "à", "|": "ò", "}": "è", "~": "ì" };
ne.E = ne[6] = { "@": "Ä", "[": "Æ", "\\": "Ø", "]": "Å", "^": "Ü", "`": "ä", "{": "æ", "|": "ø", "}": "å", "~": "ü" };
ne.Z = { "#": "£", "@": "§", "[": "¡", "\\": "Ñ", "]": "¿", "{": "°", "|": "ñ", "}": "ç" };
ne.H = ne[7] = { "@": "É", "[": "Ä", "\\": "Ö", "]": "Å", "^": "Ü", "`": "é", "{": "ä", "|": "ö", "}": "å", "~": "ü" };
ne["="] = { "#": "ù", "@": "à", "[": "é", "\\": "ç", "]": "ê", "^": "î", _: "è", "`": "ô", "{": "ä", "|": "ö", "}": "ü", "~": "û" };
var cl = 4294967295;
var $i2 = class {
  constructor(t, e, i) {
    this._hasScrollback = t;
    this._optionsService = e;
    this._bufferService = i;
    this.ydisp = 0;
    this.ybase = 0;
    this.y = 0;
    this.x = 0;
    this.tabs = {};
    this.savedY = 0;
    this.savedX = 0;
    this.savedCurAttrData = X2.clone();
    this.savedCharset = Je;
    this.markers = [];
    this._nullCell = q.fromCharData([0, ir, 1, 0]);
    this._whitespaceCell = q.fromCharData([0, we, 1, 32]);
    this._isClearing = false;
    this._memoryCleanupQueue = new Jt;
    this._memoryCleanupPosition = 0;
    this._cols = this._bufferService.cols, this._rows = this._bufferService.rows, this.lines = new zi2(this._getCorrectBufferLength(this._rows)), this.scrollTop = 0, this.scrollBottom = this._rows - 1, this.setupTabStops();
  }
  getNullCell(t) {
    return t ? (this._nullCell.fg = t.fg, this._nullCell.bg = t.bg, this._nullCell.extended = t.extended) : (this._nullCell.fg = 0, this._nullCell.bg = 0, this._nullCell.extended = new rt2), this._nullCell;
  }
  getWhitespaceCell(t) {
    return t ? (this._whitespaceCell.fg = t.fg, this._whitespaceCell.bg = t.bg, this._whitespaceCell.extended = t.extended) : (this._whitespaceCell.fg = 0, this._whitespaceCell.bg = 0, this._whitespaceCell.extended = new rt2), this._whitespaceCell;
  }
  getBlankLine(t, e) {
    return new Ze2(this._bufferService.cols, this.getNullCell(t), e);
  }
  get hasScrollback() {
    return this._hasScrollback && this.lines.maxLength > this._rows;
  }
  get isCursorInViewport() {
    let e = this.ybase + this.y - this.ydisp;
    return e >= 0 && e < this._rows;
  }
  _getCorrectBufferLength(t) {
    if (!this._hasScrollback)
      return t;
    let e = t + this._optionsService.rawOptions.scrollback;
    return e > cl ? cl : e;
  }
  fillViewportRows(t) {
    if (this.lines.length === 0) {
      t === undefined && (t = X2);
      let e = this._rows;
      for (;e--; )
        this.lines.push(this.getBlankLine(t));
    }
  }
  clear() {
    this.ydisp = 0, this.ybase = 0, this.y = 0, this.x = 0, this.lines = new zi2(this._getCorrectBufferLength(this._rows)), this.scrollTop = 0, this.scrollBottom = this._rows - 1, this.setupTabStops();
  }
  resize(t, e) {
    let i = this.getNullCell(X2), r = 0, n = this._getCorrectBufferLength(e);
    if (n > this.lines.maxLength && (this.lines.maxLength = n), this.lines.length > 0) {
      if (this._cols < t)
        for (let l = 0;l < this.lines.length; l++)
          r += +this.lines.get(l).resize(t, i);
      let o = 0;
      if (this._rows < e)
        for (let l = this._rows;l < e; l++)
          this.lines.length < e + this.ybase && (this._optionsService.rawOptions.windowsMode || this._optionsService.rawOptions.windowsPty.backend !== undefined || this._optionsService.rawOptions.windowsPty.buildNumber !== undefined ? this.lines.push(new Ze2(t, i)) : this.ybase > 0 && this.lines.length <= this.ybase + this.y + o + 1 ? (this.ybase--, o++, this.ydisp > 0 && this.ydisp--) : this.lines.push(new Ze2(t, i)));
      else
        for (let l = this._rows;l > e; l--)
          this.lines.length > e + this.ybase && (this.lines.length > this.ybase + this.y + 1 ? this.lines.pop() : (this.ybase++, this.ydisp++));
      if (n < this.lines.maxLength) {
        let l = this.lines.length - n;
        l > 0 && (this.lines.trimStart(l), this.ybase = Math.max(this.ybase - l, 0), this.ydisp = Math.max(this.ydisp - l, 0), this.savedY = Math.max(this.savedY - l, 0)), this.lines.maxLength = n;
      }
      this.x = Math.min(this.x, t - 1), this.y = Math.min(this.y, e - 1), o && (this.y += o), this.savedX = Math.min(this.savedX, t - 1), this.scrollTop = 0;
    }
    if (this.scrollBottom = e - 1, this._isReflowEnabled && (this._reflow(t, e), this._cols > t))
      for (let o = 0;o < this.lines.length; o++)
        r += +this.lines.get(o).resize(t, i);
    this._cols = t, this._rows = e, this._memoryCleanupQueue.clear(), r > 0.1 * this.lines.length && (this._memoryCleanupPosition = 0, this._memoryCleanupQueue.enqueue(() => this._batchedMemoryCleanup()));
  }
  _batchedMemoryCleanup() {
    let t = true;
    this._memoryCleanupPosition >= this.lines.length && (this._memoryCleanupPosition = 0, t = false);
    let e = 0;
    for (;this._memoryCleanupPosition < this.lines.length; )
      if (e += this.lines.get(this._memoryCleanupPosition++).cleanupMemory(), e > 100)
        return true;
    return t;
  }
  get _isReflowEnabled() {
    let t = this._optionsService.rawOptions.windowsPty;
    return t && t.buildNumber ? this._hasScrollback && t.backend === "conpty" && t.buildNumber >= 21376 : this._hasScrollback && !this._optionsService.rawOptions.windowsMode;
  }
  _reflow(t, e) {
    this._cols !== t && (t > this._cols ? this._reflowLarger(t, e) : this._reflowSmaller(t, e));
  }
  _reflowLarger(t, e) {
    let i = this._optionsService.rawOptions.reflowCursorLine, r = sl(this.lines, this._cols, t, this.ybase + this.y, this.getNullCell(X2), i);
    if (r.length > 0) {
      let n = ol(this.lines, r);
      ll(this.lines, n.layout), this._reflowLargerAdjustViewport(t, e, n.countRemoved);
    }
  }
  _reflowLargerAdjustViewport(t, e, i) {
    let r = this.getNullCell(X2), n = i;
    for (;n-- > 0; )
      this.ybase === 0 ? (this.y > 0 && this.y--, this.lines.length < e && this.lines.push(new Ze2(t, r))) : (this.ydisp === this.ybase && this.ydisp--, this.ybase--);
    this.savedY = Math.max(this.savedY - i, 0);
  }
  _reflowSmaller(t, e) {
    let i = this._optionsService.rawOptions.reflowCursorLine, r = this.getNullCell(X2), n = [], o = 0;
    for (let l = this.lines.length - 1;l >= 0; l--) {
      let a = this.lines.get(l);
      if (!a || !a.isWrapped && a.getTrimmedLength() <= t)
        continue;
      let u = [a];
      for (;a.isWrapped && l > 0; )
        a = this.lines.get(--l), u.unshift(a);
      if (!i) {
        let I = this.ybase + this.y;
        if (I >= l && I < l + u.length)
          continue;
      }
      let h = u[u.length - 1].getTrimmedLength(), c = al(u, this._cols, t), d = c.length - u.length, _;
      this.ybase === 0 && this.y !== this.lines.length - 1 ? _ = Math.max(0, this.y - this.lines.maxLength + d) : _ = Math.max(0, this.lines.length - this.lines.maxLength + d);
      let p = [];
      for (let I = 0;I < d; I++) {
        let k = this.getBlankLine(X2, true);
        p.push(k);
      }
      p.length > 0 && (n.push({ start: l + u.length + o, newLines: p }), o += p.length), u.push(...p);
      let m = c.length - 1, f = c[m];
      f === 0 && (m--, f = c[m]);
      let A = u.length - d - 1, R = h;
      for (;A >= 0; ) {
        let I = Math.min(R, f);
        if (u[m] === undefined)
          break;
        if (u[m].copyCellsFrom(u[A], R - I, f - I, I, true), f -= I, f === 0 && (m--, f = c[m]), R -= I, R === 0) {
          A--;
          let k = Math.max(A, 0);
          R = ri2(u, k, this._cols);
        }
      }
      for (let I = 0;I < u.length; I++)
        c[I] < t && u[I].setCell(c[I], r);
      let O = d - _;
      for (;O-- > 0; )
        this.ybase === 0 ? this.y < e - 1 ? (this.y++, this.lines.pop()) : (this.ybase++, this.ydisp++) : this.ybase < Math.min(this.lines.maxLength, this.lines.length + o) - e && (this.ybase === this.ydisp && this.ydisp++, this.ybase++);
      this.savedY = Math.min(this.savedY + d, this.ybase + e - 1);
    }
    if (n.length > 0) {
      let l = [], a = [];
      for (let f = 0;f < this.lines.length; f++)
        a.push(this.lines.get(f));
      let u = this.lines.length, h = u - 1, c = 0, d = n[c];
      this.lines.length = Math.min(this.lines.maxLength, this.lines.length + o);
      let _ = 0;
      for (let f = Math.min(this.lines.maxLength - 1, u + o - 1);f >= 0; f--)
        if (d && d.start > h + _) {
          for (let A = d.newLines.length - 1;A >= 0; A--)
            this.lines.set(f--, d.newLines[A]);
          f++, l.push({ index: h + 1, amount: d.newLines.length }), _ += d.newLines.length, d = n[++c];
        } else
          this.lines.set(f, a[h--]);
      let p = 0;
      for (let f = l.length - 1;f >= 0; f--)
        l[f].index += p, this.lines.onInsertEmitter.fire(l[f]), p += l[f].amount;
      let m = Math.max(0, u + o - this.lines.maxLength);
      m > 0 && this.lines.onTrimEmitter.fire(m);
    }
  }
  translateBufferLineToString(t, e, i = 0, r) {
    let n = this.lines.get(t);
    return n ? n.translateToString(e, i, r) : "";
  }
  getWrappedRangeForLine(t) {
    let e = t, i = t;
    for (;e > 0 && this.lines.get(e).isWrapped; )
      e--;
    for (;i + 1 < this.lines.length && this.lines.get(i + 1).isWrapped; )
      i++;
    return { first: e, last: i };
  }
  setupTabStops(t) {
    for (t != null ? this.tabs[t] || (t = this.prevStop(t)) : (this.tabs = {}, t = 0);t < this._cols; t += this._optionsService.rawOptions.tabStopWidth)
      this.tabs[t] = true;
  }
  prevStop(t) {
    for (t == null && (t = this.x);!this.tabs[--t] && t > 0; )
      ;
    return t >= this._cols ? this._cols - 1 : t < 0 ? 0 : t;
  }
  nextStop(t) {
    for (t == null && (t = this.x);!this.tabs[++t] && t < this._cols; )
      ;
    return t >= this._cols ? this._cols - 1 : t < 0 ? 0 : t;
  }
  clearMarkers(t) {
    this._isClearing = true;
    for (let e = 0;e < this.markers.length; e++)
      this.markers[e].line === t && (this.markers[e].dispose(), this.markers.splice(e--, 1));
    this._isClearing = false;
  }
  clearAllMarkers() {
    this._isClearing = true;
    for (let t = 0;t < this.markers.length; t++)
      this.markers[t].dispose();
    this.markers.length = 0, this._isClearing = false;
  }
  addMarker(t) {
    let e = new cn2(t);
    return this.markers.push(e), e.register(this.lines.onTrim((i) => {
      e.line -= i, e.line < 0 && e.dispose();
    })), e.register(this.lines.onInsert((i) => {
      e.line >= i.index && (e.line += i.amount);
    })), e.register(this.lines.onDelete((i) => {
      e.line >= i.index && e.line < i.index + i.amount && e.dispose(), e.line > i.index && (e.line -= i.amount);
    })), e.register(e.onDispose(() => this._removeMarker(e))), e;
  }
  _removeMarker(t) {
    this._isClearing || this.markers.splice(this.markers.indexOf(t), 1);
  }
};
var hn2 = class extends D2 {
  constructor(e, i) {
    super();
    this._optionsService = e;
    this._bufferService = i;
    this._onBufferActivate = this._register(new v);
    this.onBufferActivate = this._onBufferActivate.event;
    this.reset(), this._register(this._optionsService.onSpecificOptionChange("scrollback", () => this.resize(this._bufferService.cols, this._bufferService.rows))), this._register(this._optionsService.onSpecificOptionChange("tabStopWidth", () => this.setupTabStops()));
  }
  reset() {
    this._normal = new $i2(true, this._optionsService, this._bufferService), this._normal.fillViewportRows(), this._alt = new $i2(false, this._optionsService, this._bufferService), this._activeBuffer = this._normal, this._onBufferActivate.fire({ activeBuffer: this._normal, inactiveBuffer: this._alt }), this.setupTabStops();
  }
  get alt() {
    return this._alt;
  }
  get active() {
    return this._activeBuffer;
  }
  get normal() {
    return this._normal;
  }
  activateNormalBuffer() {
    this._activeBuffer !== this._normal && (this._normal.x = this._alt.x, this._normal.y = this._alt.y, this._alt.clearAllMarkers(), this._alt.clear(), this._activeBuffer = this._normal, this._onBufferActivate.fire({ activeBuffer: this._normal, inactiveBuffer: this._alt }));
  }
  activateAltBuffer(e) {
    this._activeBuffer !== this._alt && (this._alt.fillViewportRows(e), this._alt.x = this._normal.x, this._alt.y = this._normal.y, this._activeBuffer = this._alt, this._onBufferActivate.fire({ activeBuffer: this._alt, inactiveBuffer: this._normal }));
  }
  resize(e, i) {
    this._normal.resize(e, i), this._alt.resize(e, i), this.setupTabStops(e);
  }
  setupTabStops(e) {
    this._normal.setupTabStops(e), this._alt.setupTabStops(e);
  }
};
var ks2 = 2;
var Cs = 1;
var ni2 = class extends D2 {
  constructor(e) {
    super();
    this.isUserScrolling = false;
    this._onResize = this._register(new v);
    this.onResize = this._onResize.event;
    this._onScroll = this._register(new v);
    this.onScroll = this._onScroll.event;
    this.cols = Math.max(e.rawOptions.cols || 0, ks2), this.rows = Math.max(e.rawOptions.rows || 0, Cs), this.buffers = this._register(new hn2(e, this)), this._register(this.buffers.onBufferActivate((i) => {
      this._onScroll.fire(i.activeBuffer.ydisp);
    }));
  }
  get buffer() {
    return this.buffers.active;
  }
  resize(e, i) {
    let r = this.cols !== e, n = this.rows !== i;
    this.cols = e, this.rows = i, this.buffers.resize(e, i), this._onResize.fire({ cols: e, rows: i, colsChanged: r, rowsChanged: n });
  }
  reset() {
    this.buffers.reset(), this.isUserScrolling = false;
  }
  scroll(e, i = false) {
    let r = this.buffer, n;
    n = this._cachedBlankLine, (!n || n.length !== this.cols || n.getFg(0) !== e.fg || n.getBg(0) !== e.bg) && (n = r.getBlankLine(e, i), this._cachedBlankLine = n), n.isWrapped = i;
    let o = r.ybase + r.scrollTop, l = r.ybase + r.scrollBottom;
    if (r.scrollTop === 0) {
      let a = r.lines.isFull;
      l === r.lines.length - 1 ? a ? r.lines.recycle().copyFrom(n) : r.lines.push(n.clone()) : r.lines.splice(l + 1, 0, n.clone()), a ? this.isUserScrolling && (r.ydisp = Math.max(r.ydisp - 1, 0)) : (r.ybase++, this.isUserScrolling || r.ydisp++);
    } else {
      let a = l - o + 1;
      r.lines.shiftElements(o + 1, a - 1, -1), r.lines.set(l, n.clone());
    }
    this.isUserScrolling || (r.ydisp = r.ybase), this._onScroll.fire(r.ydisp);
  }
  scrollLines(e, i) {
    let r = this.buffer;
    if (e < 0) {
      if (r.ydisp === 0)
        return;
      this.isUserScrolling = true;
    } else
      e + r.ydisp >= r.ybase && (this.isUserScrolling = false);
    let n = r.ydisp;
    r.ydisp = Math.max(Math.min(r.ydisp + e, r.ybase), 0), n !== r.ydisp && (i || this._onScroll.fire(r.ydisp));
  }
};
ni2 = M([S(0, H2)], ni2);
var si = { cols: 80, rows: 24, cursorBlink: false, cursorStyle: "block", cursorWidth: 1, cursorInactiveStyle: "outline", customGlyphs: true, drawBoldTextInBrightColors: true, documentOverride: null, fastScrollModifier: "alt", fastScrollSensitivity: 5, fontFamily: "monospace", fontSize: 15, fontWeight: "normal", fontWeightBold: "bold", ignoreBracketedPasteMode: false, lineHeight: 1, letterSpacing: 0, linkHandler: null, logLevel: "info", logger: null, scrollback: 1000, scrollOnEraseInDisplay: false, scrollOnUserInput: true, scrollSensitivity: 1, screenReaderMode: false, smoothScrollDuration: 0, macOptionIsMeta: false, macOptionClickForcesSelection: false, minimumContrastRatio: 1, disableStdin: false, allowProposedApi: false, allowTransparency: false, tabStopWidth: 8, theme: {}, reflowCursorLine: false, rescaleOverlappingGlyphs: false, rightClickSelectsWord: Zt2, windowOptions: {}, windowsMode: false, windowsPty: {}, wordSeparator: " ()[]{}',\"`", altClickMovesCursor: true, convertEol: false, termName: "xterm", cancelEvents: false, overviewRuler: {} };
var nc = ["normal", "bold", "100", "200", "300", "400", "500", "600", "700", "800", "900"];
var dn2 = class extends D2 {
  constructor(e) {
    super();
    this._onOptionChange = this._register(new v);
    this.onOptionChange = this._onOptionChange.event;
    let i = { ...si };
    for (let r in e)
      if (r in i)
        try {
          let n = e[r];
          i[r] = this._sanitizeAndValidateOption(r, n);
        } catch (n) {
          console.error(n);
        }
    this.rawOptions = i, this.options = { ...i }, this._setupOptions(), this._register(C2(() => {
      this.rawOptions.linkHandler = null, this.rawOptions.documentOverride = null;
    }));
  }
  onSpecificOptionChange(e, i) {
    return this.onOptionChange((r) => {
      r === e && i(this.rawOptions[e]);
    });
  }
  onMultipleOptionChange(e, i) {
    return this.onOptionChange((r) => {
      e.indexOf(r) !== -1 && i();
    });
  }
  _setupOptions() {
    let e = (r) => {
      if (!(r in si))
        throw new Error(`No option with key "${r}"`);
      return this.rawOptions[r];
    }, i = (r, n) => {
      if (!(r in si))
        throw new Error(`No option with key "${r}"`);
      n = this._sanitizeAndValidateOption(r, n), this.rawOptions[r] !== n && (this.rawOptions[r] = n, this._onOptionChange.fire(r));
    };
    for (let r in this.rawOptions) {
      let n = { get: e.bind(this, r), set: i.bind(this, r) };
      Object.defineProperty(this.options, r, n);
    }
  }
  _sanitizeAndValidateOption(e, i) {
    switch (e) {
      case "cursorStyle":
        if (i || (i = si[e]), !sc(i))
          throw new Error(`"${i}" is not a valid value for ${e}`);
        break;
      case "wordSeparator":
        i || (i = si[e]);
        break;
      case "fontWeight":
      case "fontWeightBold":
        if (typeof i == "number" && 1 <= i && i <= 1000)
          break;
        i = nc.includes(i) ? i : si[e];
        break;
      case "cursorWidth":
        i = Math.floor(i);
      case "lineHeight":
      case "tabStopWidth":
        if (i < 1)
          throw new Error(`${e} cannot be less than 1, value: ${i}`);
        break;
      case "minimumContrastRatio":
        i = Math.max(1, Math.min(21, Math.round(i * 10) / 10));
        break;
      case "scrollback":
        if (i = Math.min(i, 4294967295), i < 0)
          throw new Error(`${e} cannot be less than 0, value: ${i}`);
        break;
      case "fastScrollSensitivity":
      case "scrollSensitivity":
        if (i <= 0)
          throw new Error(`${e} cannot be less than or equal to 0, value: ${i}`);
        break;
      case "rows":
      case "cols":
        if (!i && i !== 0)
          throw new Error(`${e} must be numeric, value: ${i}`);
        break;
      case "windowsPty":
        i = i ?? {};
        break;
    }
    return i;
  }
};
function sc(s) {
  return s === "block" || s === "underline" || s === "bar";
}
function oi2(s, t = 5) {
  if (typeof s != "object")
    return s;
  let e = Array.isArray(s) ? [] : {};
  for (let i in s)
    e[i] = t <= 1 ? s[i] : s[i] && oi2(s[i], t - 1);
  return e;
}
var ul = Object.freeze({ insertMode: false });
var hl = Object.freeze({ applicationCursorKeys: false, applicationKeypad: false, bracketedPasteMode: false, cursorBlink: undefined, cursorStyle: undefined, origin: false, reverseWraparound: false, sendFocus: false, synchronizedOutput: false, wraparound: true });
var li2 = class extends D2 {
  constructor(e, i, r) {
    super();
    this._bufferService = e;
    this._logService = i;
    this._optionsService = r;
    this.isCursorInitialized = false;
    this.isCursorHidden = false;
    this._onData = this._register(new v);
    this.onData = this._onData.event;
    this._onUserInput = this._register(new v);
    this.onUserInput = this._onUserInput.event;
    this._onBinary = this._register(new v);
    this.onBinary = this._onBinary.event;
    this._onRequestScrollToBottom = this._register(new v);
    this.onRequestScrollToBottom = this._onRequestScrollToBottom.event;
    this.modes = oi2(ul), this.decPrivateModes = oi2(hl);
  }
  reset() {
    this.modes = oi2(ul), this.decPrivateModes = oi2(hl);
  }
  triggerDataEvent(e, i = false) {
    if (this._optionsService.rawOptions.disableStdin)
      return;
    let r = this._bufferService.buffer;
    i && this._optionsService.rawOptions.scrollOnUserInput && r.ybase !== r.ydisp && this._onRequestScrollToBottom.fire(), i && this._onUserInput.fire(), this._logService.debug(`sending data "${e}"`), this._logService.trace("sending data (codes)", () => e.split("").map((n) => n.charCodeAt(0))), this._onData.fire(e);
  }
  triggerBinaryEvent(e) {
    this._optionsService.rawOptions.disableStdin || (this._logService.debug(`sending binary "${e}"`), this._logService.trace("sending binary (codes)", () => e.split("").map((i) => i.charCodeAt(0))), this._onBinary.fire(e));
  }
};
li2 = M([S(0, F2), S(1, nr2), S(2, H2)], li2);
var dl = { NONE: { events: 0, restrict: () => false }, X10: { events: 1, restrict: (s) => s.button === 4 || s.action !== 1 ? false : (s.ctrl = false, s.alt = false, s.shift = false, true) }, VT200: { events: 19, restrict: (s) => s.action !== 32 }, DRAG: { events: 23, restrict: (s) => !(s.action === 32 && s.button === 3) }, ANY: { events: 31, restrict: (s) => true } };
function Ms2(s, t) {
  let e = (s.ctrl ? 16 : 0) | (s.shift ? 4 : 0) | (s.alt ? 8 : 0);
  return s.button === 4 ? (e |= 64, e |= s.action) : (e |= s.button & 3, s.button & 4 && (e |= 64), s.button & 8 && (e |= 128), s.action === 32 ? e |= 32 : s.action === 0 && !t && (e |= 3)), e;
}
var Ps2 = String.fromCharCode;
var fl = { DEFAULT: (s) => {
  let t = [Ms2(s, false) + 32, s.col + 32, s.row + 32];
  return t[0] > 255 || t[1] > 255 || t[2] > 255 ? "" : `\x1B[M${Ps2(t[0])}${Ps2(t[1])}${Ps2(t[2])}`;
}, SGR: (s) => {
  let t = s.action === 0 && s.button !== 4 ? "m" : "M";
  return `\x1B[<${Ms2(s, true)};${s.col};${s.row}${t}`;
}, SGR_PIXELS: (s) => {
  let t = s.action === 0 && s.button !== 4 ? "m" : "M";
  return `\x1B[<${Ms2(s, true)};${s.x};${s.y}${t}`;
} };
var ai2 = class extends D2 {
  constructor(e, i, r) {
    super();
    this._bufferService = e;
    this._coreService = i;
    this._optionsService = r;
    this._protocols = {};
    this._encodings = {};
    this._activeProtocol = "";
    this._activeEncoding = "";
    this._lastEvent = null;
    this._wheelPartialScroll = 0;
    this._onProtocolChange = this._register(new v);
    this.onProtocolChange = this._onProtocolChange.event;
    for (let n of Object.keys(dl))
      this.addProtocol(n, dl[n]);
    for (let n of Object.keys(fl))
      this.addEncoding(n, fl[n]);
    this.reset();
  }
  addProtocol(e, i) {
    this._protocols[e] = i;
  }
  addEncoding(e, i) {
    this._encodings[e] = i;
  }
  get activeProtocol() {
    return this._activeProtocol;
  }
  get areMouseEventsActive() {
    return this._protocols[this._activeProtocol].events !== 0;
  }
  set activeProtocol(e) {
    if (!this._protocols[e])
      throw new Error(`unknown protocol "${e}"`);
    this._activeProtocol = e, this._onProtocolChange.fire(this._protocols[e].events);
  }
  get activeEncoding() {
    return this._activeEncoding;
  }
  set activeEncoding(e) {
    if (!this._encodings[e])
      throw new Error(`unknown encoding "${e}"`);
    this._activeEncoding = e;
  }
  reset() {
    this.activeProtocol = "NONE", this.activeEncoding = "DEFAULT", this._lastEvent = null, this._wheelPartialScroll = 0;
  }
  consumeWheelEvent(e, i, r) {
    if (e.deltaY === 0 || e.shiftKey || i === undefined || r === undefined)
      return 0;
    let n = i / r, o = this._applyScrollModifier(e.deltaY, e);
    return e.deltaMode === WheelEvent.DOM_DELTA_PIXEL ? (o /= n + 0, Math.abs(e.deltaY) < 50 && (o *= 0.3), this._wheelPartialScroll += o, o = Math.floor(Math.abs(this._wheelPartialScroll)) * (this._wheelPartialScroll > 0 ? 1 : -1), this._wheelPartialScroll %= 1) : e.deltaMode === WheelEvent.DOM_DELTA_PAGE && (o *= this._bufferService.rows), o;
  }
  _applyScrollModifier(e, i) {
    return i.altKey || i.ctrlKey || i.shiftKey ? e * this._optionsService.rawOptions.fastScrollSensitivity * this._optionsService.rawOptions.scrollSensitivity : e * this._optionsService.rawOptions.scrollSensitivity;
  }
  triggerMouseEvent(e) {
    if (e.col < 0 || e.col >= this._bufferService.cols || e.row < 0 || e.row >= this._bufferService.rows || e.button === 4 && e.action === 32 || e.button === 3 && e.action !== 32 || e.button !== 4 && (e.action === 2 || e.action === 3) || (e.col++, e.row++, e.action === 32 && this._lastEvent && this._equalEvents(this._lastEvent, e, this._activeEncoding === "SGR_PIXELS")) || !this._protocols[this._activeProtocol].restrict(e))
      return false;
    let i = this._encodings[this._activeEncoding](e);
    return i && (this._activeEncoding === "DEFAULT" ? this._coreService.triggerBinaryEvent(i) : this._coreService.triggerDataEvent(i, true)), this._lastEvent = e, true;
  }
  explainEvents(e) {
    return { down: !!(e & 1), up: !!(e & 2), drag: !!(e & 4), move: !!(e & 8), wheel: !!(e & 16) };
  }
  _equalEvents(e, i, r) {
    if (r) {
      if (e.x !== i.x || e.y !== i.y)
        return false;
    } else if (e.col !== i.col || e.row !== i.row)
      return false;
    return !(e.button !== i.button || e.action !== i.action || e.ctrl !== i.ctrl || e.alt !== i.alt || e.shift !== i.shift);
  }
};
ai2 = M([S(0, F2), S(1, ge2), S(2, H2)], ai2);
var Os2 = [[768, 879], [1155, 1158], [1160, 1161], [1425, 1469], [1471, 1471], [1473, 1474], [1476, 1477], [1479, 1479], [1536, 1539], [1552, 1557], [1611, 1630], [1648, 1648], [1750, 1764], [1767, 1768], [1770, 1773], [1807, 1807], [1809, 1809], [1840, 1866], [1958, 1968], [2027, 2035], [2305, 2306], [2364, 2364], [2369, 2376], [2381, 2381], [2385, 2388], [2402, 2403], [2433, 2433], [2492, 2492], [2497, 2500], [2509, 2509], [2530, 2531], [2561, 2562], [2620, 2620], [2625, 2626], [2631, 2632], [2635, 2637], [2672, 2673], [2689, 2690], [2748, 2748], [2753, 2757], [2759, 2760], [2765, 2765], [2786, 2787], [2817, 2817], [2876, 2876], [2879, 2879], [2881, 2883], [2893, 2893], [2902, 2902], [2946, 2946], [3008, 3008], [3021, 3021], [3134, 3136], [3142, 3144], [3146, 3149], [3157, 3158], [3260, 3260], [3263, 3263], [3270, 3270], [3276, 3277], [3298, 3299], [3393, 3395], [3405, 3405], [3530, 3530], [3538, 3540], [3542, 3542], [3633, 3633], [3636, 3642], [3655, 3662], [3761, 3761], [3764, 3769], [3771, 3772], [3784, 3789], [3864, 3865], [3893, 3893], [3895, 3895], [3897, 3897], [3953, 3966], [3968, 3972], [3974, 3975], [3984, 3991], [3993, 4028], [4038, 4038], [4141, 4144], [4146, 4146], [4150, 4151], [4153, 4153], [4184, 4185], [4448, 4607], [4959, 4959], [5906, 5908], [5938, 5940], [5970, 5971], [6002, 6003], [6068, 6069], [6071, 6077], [6086, 6086], [6089, 6099], [6109, 6109], [6155, 6157], [6313, 6313], [6432, 6434], [6439, 6440], [6450, 6450], [6457, 6459], [6679, 6680], [6912, 6915], [6964, 6964], [6966, 6970], [6972, 6972], [6978, 6978], [7019, 7027], [7616, 7626], [7678, 7679], [8203, 8207], [8234, 8238], [8288, 8291], [8298, 8303], [8400, 8431], [12330, 12335], [12441, 12442], [43014, 43014], [43019, 43019], [43045, 43046], [64286, 64286], [65024, 65039], [65056, 65059], [65279, 65279], [65529, 65531]];
var ac = [[68097, 68099], [68101, 68102], [68108, 68111], [68152, 68154], [68159, 68159], [119143, 119145], [119155, 119170], [119173, 119179], [119210, 119213], [119362, 119364], [917505, 917505], [917536, 917631], [917760, 917999]];
var se;
function cc(s, t) {
  let e = 0, i = t.length - 1, r;
  if (s < t[0][0] || s > t[i][1])
    return false;
  for (;i >= e; )
    if (r = e + i >> 1, s > t[r][1])
      e = r + 1;
    else if (s < t[r][0])
      i = r - 1;
    else
      return true;
  return false;
}
var fn2 = class {
  constructor() {
    this.version = "6";
    if (!se) {
      se = new Uint8Array(65536), se.fill(1), se[0] = 0, se.fill(0, 1, 32), se.fill(0, 127, 160), se.fill(2, 4352, 4448), se[9001] = 2, se[9002] = 2, se.fill(2, 11904, 42192), se[12351] = 1, se.fill(2, 44032, 55204), se.fill(2, 63744, 64256), se.fill(2, 65040, 65050), se.fill(2, 65072, 65136), se.fill(2, 65280, 65377), se.fill(2, 65504, 65511);
      for (let t = 0;t < Os2.length; ++t)
        se.fill(0, Os2[t][0], Os2[t][1] + 1);
    }
  }
  wcwidth(t) {
    return t < 32 ? 0 : t < 127 ? 1 : t < 65536 ? se[t] : cc(t, ac) ? 0 : t >= 131072 && t <= 196605 || t >= 196608 && t <= 262141 ? 2 : 1;
  }
  charProperties(t, e) {
    let i = this.wcwidth(t), r = i === 0 && e !== 0;
    if (r) {
      let n = Ae2.extractWidth(e);
      n === 0 ? r = false : n > i && (i = n);
    }
    return Ae2.createPropertyValue(0, i, r);
  }
};
var Ae2 = class s {
  constructor() {
    this._providers = Object.create(null);
    this._active = "";
    this._onChange = new v;
    this.onChange = this._onChange.event;
    let t = new fn2;
    this.register(t), this._active = t.version, this._activeProvider = t;
  }
  static extractShouldJoin(t) {
    return (t & 1) !== 0;
  }
  static extractWidth(t) {
    return t >> 1 & 3;
  }
  static extractCharKind(t) {
    return t >> 3;
  }
  static createPropertyValue(t, e, i = false) {
    return (t & 16777215) << 3 | (e & 3) << 1 | (i ? 1 : 0);
  }
  dispose() {
    this._onChange.dispose();
  }
  get versions() {
    return Object.keys(this._providers);
  }
  get activeVersion() {
    return this._active;
  }
  set activeVersion(t) {
    if (!this._providers[t])
      throw new Error(`unknown Unicode version "${t}"`);
    this._active = t, this._activeProvider = this._providers[t], this._onChange.fire(t);
  }
  register(t) {
    this._providers[t.version] = t;
  }
  wcwidth(t) {
    return this._activeProvider.wcwidth(t);
  }
  getStringCellWidth(t) {
    let e = 0, i = 0, r = t.length;
    for (let n = 0;n < r; ++n) {
      let o = t.charCodeAt(n);
      if (55296 <= o && o <= 56319) {
        if (++n >= r)
          return e + this.wcwidth(o);
        let u = t.charCodeAt(n);
        56320 <= u && u <= 57343 ? o = (o - 55296) * 1024 + u - 56320 + 65536 : e += this.wcwidth(u);
      }
      let l = this.charProperties(o, i), a = s.extractWidth(l);
      s.extractShouldJoin(l) && (a -= s.extractWidth(i)), e += a, i = l;
    }
    return e;
  }
  charProperties(t, e) {
    return this._activeProvider.charProperties(t, e);
  }
};
var pn2 = class {
  constructor() {
    this.glevel = 0;
    this._charsets = [];
  }
  reset() {
    this.charset = undefined, this._charsets = [], this.glevel = 0;
  }
  setgLevel(t) {
    this.glevel = t, this.charset = this._charsets[t];
  }
  setgCharset(t, e) {
    this._charsets[t] = e, this.glevel === t && (this.charset = e);
  }
};
function Bs2(s) {
  let e = s.buffer.lines.get(s.buffer.ybase + s.buffer.y - 1)?.get(s.cols - 1), i = s.buffer.lines.get(s.buffer.ybase + s.buffer.y);
  i && e && (i.isWrapped = e[3] !== 0 && e[3] !== 32);
}
var Vi2 = 2147483647;
var uc = 256;
var ci2 = class s {
  constructor(t = 32, e = 32) {
    this.maxLength = t;
    this.maxSubParamsLength = e;
    if (e > uc)
      throw new Error("maxSubParamsLength must not be greater than 256");
    this.params = new Int32Array(t), this.length = 0, this._subParams = new Int32Array(e), this._subParamsLength = 0, this._subParamsIdx = new Uint16Array(t), this._rejectDigits = false, this._rejectSubDigits = false, this._digitIsSub = false;
  }
  static fromArray(t) {
    let e = new s;
    if (!t.length)
      return e;
    for (let i = Array.isArray(t[0]) ? 1 : 0;i < t.length; ++i) {
      let r = t[i];
      if (Array.isArray(r))
        for (let n = 0;n < r.length; ++n)
          e.addSubParam(r[n]);
      else
        e.addParam(r);
    }
    return e;
  }
  clone() {
    let t = new s(this.maxLength, this.maxSubParamsLength);
    return t.params.set(this.params), t.length = this.length, t._subParams.set(this._subParams), t._subParamsLength = this._subParamsLength, t._subParamsIdx.set(this._subParamsIdx), t._rejectDigits = this._rejectDigits, t._rejectSubDigits = this._rejectSubDigits, t._digitIsSub = this._digitIsSub, t;
  }
  toArray() {
    let t = [];
    for (let e = 0;e < this.length; ++e) {
      t.push(this.params[e]);
      let i = this._subParamsIdx[e] >> 8, r = this._subParamsIdx[e] & 255;
      r - i > 0 && t.push(Array.prototype.slice.call(this._subParams, i, r));
    }
    return t;
  }
  reset() {
    this.length = 0, this._subParamsLength = 0, this._rejectDigits = false, this._rejectSubDigits = false, this._digitIsSub = false;
  }
  addParam(t) {
    if (this._digitIsSub = false, this.length >= this.maxLength) {
      this._rejectDigits = true;
      return;
    }
    if (t < -1)
      throw new Error("values lesser than -1 are not allowed");
    this._subParamsIdx[this.length] = this._subParamsLength << 8 | this._subParamsLength, this.params[this.length++] = t > Vi2 ? Vi2 : t;
  }
  addSubParam(t) {
    if (this._digitIsSub = true, !!this.length) {
      if (this._rejectDigits || this._subParamsLength >= this.maxSubParamsLength) {
        this._rejectSubDigits = true;
        return;
      }
      if (t < -1)
        throw new Error("values lesser than -1 are not allowed");
      this._subParams[this._subParamsLength++] = t > Vi2 ? Vi2 : t, this._subParamsIdx[this.length - 1]++;
    }
  }
  hasSubParams(t) {
    return (this._subParamsIdx[t] & 255) - (this._subParamsIdx[t] >> 8) > 0;
  }
  getSubParams(t) {
    let e = this._subParamsIdx[t] >> 8, i = this._subParamsIdx[t] & 255;
    return i - e > 0 ? this._subParams.subarray(e, i) : null;
  }
  getSubParamsAll() {
    let t = {};
    for (let e = 0;e < this.length; ++e) {
      let i = this._subParamsIdx[e] >> 8, r = this._subParamsIdx[e] & 255;
      r - i > 0 && (t[e] = this._subParams.slice(i, r));
    }
    return t;
  }
  addDigit(t) {
    let e;
    if (this._rejectDigits || !(e = this._digitIsSub ? this._subParamsLength : this.length) || this._digitIsSub && this._rejectSubDigits)
      return;
    let i = this._digitIsSub ? this._subParams : this.params, r = i[e - 1];
    i[e - 1] = ~r ? Math.min(r * 10 + t, Vi2) : t;
  }
};
var qi = [];
var mn2 = class {
  constructor() {
    this._state = 0;
    this._active = qi;
    this._id = -1;
    this._handlers = Object.create(null);
    this._handlerFb = () => {};
    this._stack = { paused: false, loopPosition: 0, fallThrough: false };
  }
  registerHandler(t, e) {
    this._handlers[t] === undefined && (this._handlers[t] = []);
    let i = this._handlers[t];
    return i.push(e), { dispose: () => {
      let r = i.indexOf(e);
      r !== -1 && i.splice(r, 1);
    } };
  }
  clearHandler(t) {
    this._handlers[t] && delete this._handlers[t];
  }
  setHandlerFallback(t) {
    this._handlerFb = t;
  }
  dispose() {
    this._handlers = Object.create(null), this._handlerFb = () => {}, this._active = qi;
  }
  reset() {
    if (this._state === 2)
      for (let t = this._stack.paused ? this._stack.loopPosition - 1 : this._active.length - 1;t >= 0; --t)
        this._active[t].end(false);
    this._stack.paused = false, this._active = qi, this._id = -1, this._state = 0;
  }
  _start() {
    if (this._active = this._handlers[this._id] || qi, !this._active.length)
      this._handlerFb(this._id, "START");
    else
      for (let t = this._active.length - 1;t >= 0; t--)
        this._active[t].start();
  }
  _put(t, e, i) {
    if (!this._active.length)
      this._handlerFb(this._id, "PUT", It2(t, e, i));
    else
      for (let r = this._active.length - 1;r >= 0; r--)
        this._active[r].put(t, e, i);
  }
  start() {
    this.reset(), this._state = 1;
  }
  put(t, e, i) {
    if (this._state !== 3) {
      if (this._state === 1)
        for (;e < i; ) {
          let r = t[e++];
          if (r === 59) {
            this._state = 2, this._start();
            break;
          }
          if (r < 48 || 57 < r) {
            this._state = 3;
            return;
          }
          this._id === -1 && (this._id = 0), this._id = this._id * 10 + r - 48;
        }
      this._state === 2 && i - e > 0 && this._put(t, e, i);
    }
  }
  end(t, e = true) {
    if (this._state !== 0) {
      if (this._state !== 3)
        if (this._state === 1 && this._start(), !this._active.length)
          this._handlerFb(this._id, "END", t);
        else {
          let i = false, r = this._active.length - 1, n = false;
          if (this._stack.paused && (r = this._stack.loopPosition - 1, i = e, n = this._stack.fallThrough, this._stack.paused = false), !n && i === false) {
            for (;r >= 0 && (i = this._active[r].end(t), i !== true); r--)
              if (i instanceof Promise)
                return this._stack.paused = true, this._stack.loopPosition = r, this._stack.fallThrough = false, i;
            r--;
          }
          for (;r >= 0; r--)
            if (i = this._active[r].end(false), i instanceof Promise)
              return this._stack.paused = true, this._stack.loopPosition = r, this._stack.fallThrough = true, i;
        }
      this._active = qi, this._id = -1, this._state = 0;
    }
  }
};
var pe2 = class {
  constructor(t) {
    this._handler = t;
    this._data = "";
    this._hitLimit = false;
  }
  start() {
    this._data = "", this._hitLimit = false;
  }
  put(t, e, i) {
    this._hitLimit || (this._data += It2(t, e, i), this._data.length > 1e7 && (this._data = "", this._hitLimit = true));
  }
  end(t) {
    let e = false;
    if (this._hitLimit)
      e = false;
    else if (t && (e = this._handler(this._data), e instanceof Promise))
      return e.then((i) => (this._data = "", this._hitLimit = false, i));
    return this._data = "", this._hitLimit = false, e;
  }
};
var Yi2 = [];
var _n2 = class {
  constructor() {
    this._handlers = Object.create(null);
    this._active = Yi2;
    this._ident = 0;
    this._handlerFb = () => {};
    this._stack = { paused: false, loopPosition: 0, fallThrough: false };
  }
  dispose() {
    this._handlers = Object.create(null), this._handlerFb = () => {}, this._active = Yi2;
  }
  registerHandler(t, e) {
    this._handlers[t] === undefined && (this._handlers[t] = []);
    let i = this._handlers[t];
    return i.push(e), { dispose: () => {
      let r = i.indexOf(e);
      r !== -1 && i.splice(r, 1);
    } };
  }
  clearHandler(t) {
    this._handlers[t] && delete this._handlers[t];
  }
  setHandlerFallback(t) {
    this._handlerFb = t;
  }
  reset() {
    if (this._active.length)
      for (let t = this._stack.paused ? this._stack.loopPosition - 1 : this._active.length - 1;t >= 0; --t)
        this._active[t].unhook(false);
    this._stack.paused = false, this._active = Yi2, this._ident = 0;
  }
  hook(t, e) {
    if (this.reset(), this._ident = t, this._active = this._handlers[t] || Yi2, !this._active.length)
      this._handlerFb(this._ident, "HOOK", e);
    else
      for (let i = this._active.length - 1;i >= 0; i--)
        this._active[i].hook(e);
  }
  put(t, e, i) {
    if (!this._active.length)
      this._handlerFb(this._ident, "PUT", It2(t, e, i));
    else
      for (let r = this._active.length - 1;r >= 0; r--)
        this._active[r].put(t, e, i);
  }
  unhook(t, e = true) {
    if (!this._active.length)
      this._handlerFb(this._ident, "UNHOOK", t);
    else {
      let i = false, r = this._active.length - 1, n = false;
      if (this._stack.paused && (r = this._stack.loopPosition - 1, i = e, n = this._stack.fallThrough, this._stack.paused = false), !n && i === false) {
        for (;r >= 0 && (i = this._active[r].unhook(t), i !== true); r--)
          if (i instanceof Promise)
            return this._stack.paused = true, this._stack.loopPosition = r, this._stack.fallThrough = false, i;
        r--;
      }
      for (;r >= 0; r--)
        if (i = this._active[r].unhook(false), i instanceof Promise)
          return this._stack.paused = true, this._stack.loopPosition = r, this._stack.fallThrough = true, i;
    }
    this._active = Yi2, this._ident = 0;
  }
};
var ji = new ci2;
ji.addParam(0);
var Xi = class {
  constructor(t) {
    this._handler = t;
    this._data = "";
    this._params = ji;
    this._hitLimit = false;
  }
  hook(t) {
    this._params = t.length > 1 || t.params[0] ? t.clone() : ji, this._data = "", this._hitLimit = false;
  }
  put(t, e, i) {
    this._hitLimit || (this._data += It2(t, e, i), this._data.length > 1e7 && (this._data = "", this._hitLimit = true));
  }
  unhook(t) {
    let e = false;
    if (this._hitLimit)
      e = false;
    else if (t && (e = this._handler(this._data, this._params), e instanceof Promise))
      return e.then((i) => (this._params = ji, this._data = "", this._hitLimit = false, i));
    return this._params = ji, this._data = "", this._hitLimit = false, e;
  }
};
var Fs2 = class {
  constructor(t) {
    this.table = new Uint8Array(t);
  }
  setDefault(t, e) {
    this.table.fill(t << 4 | e);
  }
  add(t, e, i, r) {
    this.table[e << 8 | t] = i << 4 | r;
  }
  addMany(t, e, i, r) {
    for (let n = 0;n < t.length; n++)
      this.table[e << 8 | t[n]] = i << 4 | r;
  }
};
var ke2 = 160;
var hc = function() {
  let s = new Fs2(4095), e = Array.apply(null, Array(256)).map((a, u) => u), i = (a, u) => e.slice(a, u), r = i(32, 127), n = i(0, 24);
  n.push(25), n.push.apply(n, i(28, 32));
  let o = i(0, 14), l;
  s.setDefault(1, 0), s.addMany(r, 0, 2, 0);
  for (l in o)
    s.addMany([24, 26, 153, 154], l, 3, 0), s.addMany(i(128, 144), l, 3, 0), s.addMany(i(144, 152), l, 3, 0), s.add(156, l, 0, 0), s.add(27, l, 11, 1), s.add(157, l, 4, 8), s.addMany([152, 158, 159], l, 0, 7), s.add(155, l, 11, 3), s.add(144, l, 11, 9);
  return s.addMany(n, 0, 3, 0), s.addMany(n, 1, 3, 1), s.add(127, 1, 0, 1), s.addMany(n, 8, 0, 8), s.addMany(n, 3, 3, 3), s.add(127, 3, 0, 3), s.addMany(n, 4, 3, 4), s.add(127, 4, 0, 4), s.addMany(n, 6, 3, 6), s.addMany(n, 5, 3, 5), s.add(127, 5, 0, 5), s.addMany(n, 2, 3, 2), s.add(127, 2, 0, 2), s.add(93, 1, 4, 8), s.addMany(r, 8, 5, 8), s.add(127, 8, 5, 8), s.addMany([156, 27, 24, 26, 7], 8, 6, 0), s.addMany(i(28, 32), 8, 0, 8), s.addMany([88, 94, 95], 1, 0, 7), s.addMany(r, 7, 0, 7), s.addMany(n, 7, 0, 7), s.add(156, 7, 0, 0), s.add(127, 7, 0, 7), s.add(91, 1, 11, 3), s.addMany(i(64, 127), 3, 7, 0), s.addMany(i(48, 60), 3, 8, 4), s.addMany([60, 61, 62, 63], 3, 9, 4), s.addMany(i(48, 60), 4, 8, 4), s.addMany(i(64, 127), 4, 7, 0), s.addMany([60, 61, 62, 63], 4, 0, 6), s.addMany(i(32, 64), 6, 0, 6), s.add(127, 6, 0, 6), s.addMany(i(64, 127), 6, 0, 0), s.addMany(i(32, 48), 3, 9, 5), s.addMany(i(32, 48), 5, 9, 5), s.addMany(i(48, 64), 5, 0, 6), s.addMany(i(64, 127), 5, 7, 0), s.addMany(i(32, 48), 4, 9, 5), s.addMany(i(32, 48), 1, 9, 2), s.addMany(i(32, 48), 2, 9, 2), s.addMany(i(48, 127), 2, 10, 0), s.addMany(i(48, 80), 1, 10, 0), s.addMany(i(81, 88), 1, 10, 0), s.addMany([89, 90, 92], 1, 10, 0), s.addMany(i(96, 127), 1, 10, 0), s.add(80, 1, 11, 9), s.addMany(n, 9, 0, 9), s.add(127, 9, 0, 9), s.addMany(i(28, 32), 9, 0, 9), s.addMany(i(32, 48), 9, 9, 12), s.addMany(i(48, 60), 9, 8, 10), s.addMany([60, 61, 62, 63], 9, 9, 10), s.addMany(n, 11, 0, 11), s.addMany(i(32, 128), 11, 0, 11), s.addMany(i(28, 32), 11, 0, 11), s.addMany(n, 10, 0, 10), s.add(127, 10, 0, 10), s.addMany(i(28, 32), 10, 0, 10), s.addMany(i(48, 60), 10, 8, 10), s.addMany([60, 61, 62, 63], 10, 0, 11), s.addMany(i(32, 48), 10, 9, 12), s.addMany(n, 12, 0, 12), s.add(127, 12, 0, 12), s.addMany(i(28, 32), 12, 0, 12), s.addMany(i(32, 48), 12, 9, 12), s.addMany(i(48, 64), 12, 0, 11), s.addMany(i(64, 127), 12, 12, 13), s.addMany(i(64, 127), 10, 12, 13), s.addMany(i(64, 127), 9, 12, 13), s.addMany(n, 13, 13, 13), s.addMany(r, 13, 13, 13), s.add(127, 13, 0, 13), s.addMany([27, 156, 24, 26], 13, 14, 0), s.add(ke2, 0, 2, 0), s.add(ke2, 8, 5, 8), s.add(ke2, 6, 0, 6), s.add(ke2, 11, 0, 11), s.add(ke2, 13, 13, 13), s;
}();
var bn2 = class extends D2 {
  constructor(e = hc) {
    super();
    this._transitions = e;
    this._parseStack = { state: 0, handlers: [], handlerPos: 0, transition: 0, chunkPos: 0 };
    this.initialState = 0, this.currentState = this.initialState, this._params = new ci2, this._params.addParam(0), this._collect = 0, this.precedingJoinState = 0, this._printHandlerFb = (i, r, n) => {}, this._executeHandlerFb = (i) => {}, this._csiHandlerFb = (i, r) => {}, this._escHandlerFb = (i) => {}, this._errorHandlerFb = (i) => i, this._printHandler = this._printHandlerFb, this._executeHandlers = Object.create(null), this._csiHandlers = Object.create(null), this._escHandlers = Object.create(null), this._register(C2(() => {
      this._csiHandlers = Object.create(null), this._executeHandlers = Object.create(null), this._escHandlers = Object.create(null);
    })), this._oscParser = this._register(new mn2), this._dcsParser = this._register(new _n2), this._errorHandler = this._errorHandlerFb, this.registerEscHandler({ final: "\\" }, () => true);
  }
  _identifier(e, i = [64, 126]) {
    let r = 0;
    if (e.prefix) {
      if (e.prefix.length > 1)
        throw new Error("only one byte as prefix supported");
      if (r = e.prefix.charCodeAt(0), r && 60 > r || r > 63)
        throw new Error("prefix must be in range 0x3c .. 0x3f");
    }
    if (e.intermediates) {
      if (e.intermediates.length > 2)
        throw new Error("only two bytes as intermediates are supported");
      for (let o = 0;o < e.intermediates.length; ++o) {
        let l = e.intermediates.charCodeAt(o);
        if (32 > l || l > 47)
          throw new Error("intermediate must be in range 0x20 .. 0x2f");
        r <<= 8, r |= l;
      }
    }
    if (e.final.length !== 1)
      throw new Error("final must be a single byte");
    let n = e.final.charCodeAt(0);
    if (i[0] > n || n > i[1])
      throw new Error(`final must be in range ${i[0]} .. ${i[1]}`);
    return r <<= 8, r |= n, r;
  }
  identToString(e) {
    let i = [];
    for (;e; )
      i.push(String.fromCharCode(e & 255)), e >>= 8;
    return i.reverse().join("");
  }
  setPrintHandler(e) {
    this._printHandler = e;
  }
  clearPrintHandler() {
    this._printHandler = this._printHandlerFb;
  }
  registerEscHandler(e, i) {
    let r = this._identifier(e, [48, 126]);
    this._escHandlers[r] === undefined && (this._escHandlers[r] = []);
    let n = this._escHandlers[r];
    return n.push(i), { dispose: () => {
      let o = n.indexOf(i);
      o !== -1 && n.splice(o, 1);
    } };
  }
  clearEscHandler(e) {
    this._escHandlers[this._identifier(e, [48, 126])] && delete this._escHandlers[this._identifier(e, [48, 126])];
  }
  setEscHandlerFallback(e) {
    this._escHandlerFb = e;
  }
  setExecuteHandler(e, i) {
    this._executeHandlers[e.charCodeAt(0)] = i;
  }
  clearExecuteHandler(e) {
    this._executeHandlers[e.charCodeAt(0)] && delete this._executeHandlers[e.charCodeAt(0)];
  }
  setExecuteHandlerFallback(e) {
    this._executeHandlerFb = e;
  }
  registerCsiHandler(e, i) {
    let r = this._identifier(e);
    this._csiHandlers[r] === undefined && (this._csiHandlers[r] = []);
    let n = this._csiHandlers[r];
    return n.push(i), { dispose: () => {
      let o = n.indexOf(i);
      o !== -1 && n.splice(o, 1);
    } };
  }
  clearCsiHandler(e) {
    this._csiHandlers[this._identifier(e)] && delete this._csiHandlers[this._identifier(e)];
  }
  setCsiHandlerFallback(e) {
    this._csiHandlerFb = e;
  }
  registerDcsHandler(e, i) {
    return this._dcsParser.registerHandler(this._identifier(e), i);
  }
  clearDcsHandler(e) {
    this._dcsParser.clearHandler(this._identifier(e));
  }
  setDcsHandlerFallback(e) {
    this._dcsParser.setHandlerFallback(e);
  }
  registerOscHandler(e, i) {
    return this._oscParser.registerHandler(e, i);
  }
  clearOscHandler(e) {
    this._oscParser.clearHandler(e);
  }
  setOscHandlerFallback(e) {
    this._oscParser.setHandlerFallback(e);
  }
  setErrorHandler(e) {
    this._errorHandler = e;
  }
  clearErrorHandler() {
    this._errorHandler = this._errorHandlerFb;
  }
  reset() {
    this.currentState = this.initialState, this._oscParser.reset(), this._dcsParser.reset(), this._params.reset(), this._params.addParam(0), this._collect = 0, this.precedingJoinState = 0, this._parseStack.state !== 0 && (this._parseStack.state = 2, this._parseStack.handlers = []);
  }
  _preserveStack(e, i, r, n, o) {
    this._parseStack.state = e, this._parseStack.handlers = i, this._parseStack.handlerPos = r, this._parseStack.transition = n, this._parseStack.chunkPos = o;
  }
  parse(e, i, r) {
    let n = 0, o = 0, l = 0, a;
    if (this._parseStack.state)
      if (this._parseStack.state === 2)
        this._parseStack.state = 0, l = this._parseStack.chunkPos + 1;
      else {
        if (r === undefined || this._parseStack.state === 1)
          throw this._parseStack.state = 1, new Error("improper continuation due to previous async handler, giving up parsing");
        let u = this._parseStack.handlers, h = this._parseStack.handlerPos - 1;
        switch (this._parseStack.state) {
          case 3:
            if (r === false && h > -1) {
              for (;h >= 0 && (a = u[h](this._params), a !== true); h--)
                if (a instanceof Promise)
                  return this._parseStack.handlerPos = h, a;
            }
            this._parseStack.handlers = [];
            break;
          case 4:
            if (r === false && h > -1) {
              for (;h >= 0 && (a = u[h](), a !== true); h--)
                if (a instanceof Promise)
                  return this._parseStack.handlerPos = h, a;
            }
            this._parseStack.handlers = [];
            break;
          case 6:
            if (n = e[this._parseStack.chunkPos], a = this._dcsParser.unhook(n !== 24 && n !== 26, r), a)
              return a;
            n === 27 && (this._parseStack.transition |= 1), this._params.reset(), this._params.addParam(0), this._collect = 0;
            break;
          case 5:
            if (n = e[this._parseStack.chunkPos], a = this._oscParser.end(n !== 24 && n !== 26, r), a)
              return a;
            n === 27 && (this._parseStack.transition |= 1), this._params.reset(), this._params.addParam(0), this._collect = 0;
            break;
        }
        this._parseStack.state = 0, l = this._parseStack.chunkPos + 1, this.precedingJoinState = 0, this.currentState = this._parseStack.transition & 15;
      }
    for (let u = l;u < i; ++u) {
      switch (n = e[u], o = this._transitions.table[this.currentState << 8 | (n < 160 ? n : ke2)], o >> 4) {
        case 2:
          for (let m = u + 1;; ++m) {
            if (m >= i || (n = e[m]) < 32 || n > 126 && n < ke2) {
              this._printHandler(e, u, m), u = m - 1;
              break;
            }
            if (++m >= i || (n = e[m]) < 32 || n > 126 && n < ke2) {
              this._printHandler(e, u, m), u = m - 1;
              break;
            }
            if (++m >= i || (n = e[m]) < 32 || n > 126 && n < ke2) {
              this._printHandler(e, u, m), u = m - 1;
              break;
            }
            if (++m >= i || (n = e[m]) < 32 || n > 126 && n < ke2) {
              this._printHandler(e, u, m), u = m - 1;
              break;
            }
          }
          break;
        case 3:
          this._executeHandlers[n] ? this._executeHandlers[n]() : this._executeHandlerFb(n), this.precedingJoinState = 0;
          break;
        case 0:
          break;
        case 1:
          if (this._errorHandler({ position: u, code: n, currentState: this.currentState, collect: this._collect, params: this._params, abort: false }).abort)
            return;
          break;
        case 7:
          let c = this._csiHandlers[this._collect << 8 | n], d = c ? c.length - 1 : -1;
          for (;d >= 0 && (a = c[d](this._params), a !== true); d--)
            if (a instanceof Promise)
              return this._preserveStack(3, c, d, o, u), a;
          d < 0 && this._csiHandlerFb(this._collect << 8 | n, this._params), this.precedingJoinState = 0;
          break;
        case 8:
          do
            switch (n) {
              case 59:
                this._params.addParam(0);
                break;
              case 58:
                this._params.addSubParam(-1);
                break;
              default:
                this._params.addDigit(n - 48);
            }
          while (++u < i && (n = e[u]) > 47 && n < 60);
          u--;
          break;
        case 9:
          this._collect <<= 8, this._collect |= n;
          break;
        case 10:
          let _ = this._escHandlers[this._collect << 8 | n], p = _ ? _.length - 1 : -1;
          for (;p >= 0 && (a = _[p](), a !== true); p--)
            if (a instanceof Promise)
              return this._preserveStack(4, _, p, o, u), a;
          p < 0 && this._escHandlerFb(this._collect << 8 | n), this.precedingJoinState = 0;
          break;
        case 11:
          this._params.reset(), this._params.addParam(0), this._collect = 0;
          break;
        case 12:
          this._dcsParser.hook(this._collect << 8 | n, this._params);
          break;
        case 13:
          for (let m = u + 1;; ++m)
            if (m >= i || (n = e[m]) === 24 || n === 26 || n === 27 || n > 127 && n < ke2) {
              this._dcsParser.put(e, u, m), u = m - 1;
              break;
            }
          break;
        case 14:
          if (a = this._dcsParser.unhook(n !== 24 && n !== 26), a)
            return this._preserveStack(6, [], 0, o, u), a;
          n === 27 && (o |= 1), this._params.reset(), this._params.addParam(0), this._collect = 0, this.precedingJoinState = 0;
          break;
        case 4:
          this._oscParser.start();
          break;
        case 5:
          for (let m = u + 1;; m++)
            if (m >= i || (n = e[m]) < 32 || n > 127 && n < ke2) {
              this._oscParser.put(e, u, m), u = m - 1;
              break;
            }
          break;
        case 6:
          if (a = this._oscParser.end(n !== 24 && n !== 26), a)
            return this._preserveStack(5, [], 0, o, u), a;
          n === 27 && (o |= 1), this._params.reset(), this._params.addParam(0), this._collect = 0, this.precedingJoinState = 0;
          break;
      }
      this.currentState = o & 15;
    }
  }
};
var dc = /^([\da-f])\/([\da-f])\/([\da-f])$|^([\da-f]{2})\/([\da-f]{2})\/([\da-f]{2})$|^([\da-f]{3})\/([\da-f]{3})\/([\da-f]{3})$|^([\da-f]{4})\/([\da-f]{4})\/([\da-f]{4})$/;
var fc = /^[\da-f]+$/;
function Ws2(s) {
  if (!s)
    return;
  let t = s.toLowerCase();
  if (t.indexOf("rgb:") === 0) {
    t = t.slice(4);
    let e = dc.exec(t);
    if (e) {
      let i = e[1] ? 15 : e[4] ? 255 : e[7] ? 4095 : 65535;
      return [Math.round(parseInt(e[1] || e[4] || e[7] || e[10], 16) / i * 255), Math.round(parseInt(e[2] || e[5] || e[8] || e[11], 16) / i * 255), Math.round(parseInt(e[3] || e[6] || e[9] || e[12], 16) / i * 255)];
    }
  } else if (t.indexOf("#") === 0 && (t = t.slice(1), fc.exec(t) && [3, 6, 9, 12].includes(t.length))) {
    let e = t.length / 3, i = [0, 0, 0];
    for (let r = 0;r < 3; ++r) {
      let n = parseInt(t.slice(e * r, e * r + e), 16);
      i[r] = e === 1 ? n << 4 : e === 2 ? n : e === 3 ? n >> 4 : n >> 8;
    }
    return i;
  }
}
function Hs2(s, t) {
  let e = s.toString(16), i = e.length < 2 ? "0" + e : e;
  switch (t) {
    case 4:
      return e[0];
    case 8:
      return i;
    case 12:
      return (i + i).slice(0, 3);
    default:
      return i + i;
  }
}
function ml(s, t = 16) {
  let [e, i, r] = s;
  return `rgb:${Hs2(e, t)}/${Hs2(i, t)}/${Hs2(r, t)}`;
}
var mc = { "(": 0, ")": 1, "*": 2, "+": 3, "-": 1, ".": 2 };
var ut2 = 131072;
var _l = 10;
function bl(s, t) {
  if (s > 24)
    return t.setWinLines || false;
  switch (s) {
    case 1:
      return !!t.restoreWin;
    case 2:
      return !!t.minimizeWin;
    case 3:
      return !!t.setWinPosition;
    case 4:
      return !!t.setWinSizePixels;
    case 5:
      return !!t.raiseWin;
    case 6:
      return !!t.lowerWin;
    case 7:
      return !!t.refreshWin;
    case 8:
      return !!t.setWinSizeChars;
    case 9:
      return !!t.maximizeWin;
    case 10:
      return !!t.fullscreenWin;
    case 11:
      return !!t.getWinState;
    case 13:
      return !!t.getWinPosition;
    case 14:
      return !!t.getWinSizePixels;
    case 15:
      return !!t.getScreenSizePixels;
    case 16:
      return !!t.getCellSizePixels;
    case 18:
      return !!t.getWinSizeChars;
    case 19:
      return !!t.getScreenSizeChars;
    case 20:
      return !!t.getIconTitle;
    case 21:
      return !!t.getWinTitle;
    case 22:
      return !!t.pushTitle;
    case 23:
      return !!t.popTitle;
    case 24:
      return !!t.setWinLines;
  }
  return false;
}
var vl = 5000;
var gl = 0;
var vn = class extends D2 {
  constructor(e, i, r, n, o, l, a, u, h = new bn2) {
    super();
    this._bufferService = e;
    this._charsetService = i;
    this._coreService = r;
    this._logService = n;
    this._optionsService = o;
    this._oscLinkService = l;
    this._coreMouseService = a;
    this._unicodeService = u;
    this._parser = h;
    this._parseBuffer = new Uint32Array(4096);
    this._stringDecoder = new er2;
    this._utf8Decoder = new tr2;
    this._windowTitle = "";
    this._iconName = "";
    this._windowTitleStack = [];
    this._iconNameStack = [];
    this._curAttrData = X2.clone();
    this._eraseAttrDataInternal = X2.clone();
    this._onRequestBell = this._register(new v);
    this.onRequestBell = this._onRequestBell.event;
    this._onRequestRefreshRows = this._register(new v);
    this.onRequestRefreshRows = this._onRequestRefreshRows.event;
    this._onRequestReset = this._register(new v);
    this.onRequestReset = this._onRequestReset.event;
    this._onRequestSendFocus = this._register(new v);
    this.onRequestSendFocus = this._onRequestSendFocus.event;
    this._onRequestSyncScrollBar = this._register(new v);
    this.onRequestSyncScrollBar = this._onRequestSyncScrollBar.event;
    this._onRequestWindowsOptionsReport = this._register(new v);
    this.onRequestWindowsOptionsReport = this._onRequestWindowsOptionsReport.event;
    this._onA11yChar = this._register(new v);
    this.onA11yChar = this._onA11yChar.event;
    this._onA11yTab = this._register(new v);
    this.onA11yTab = this._onA11yTab.event;
    this._onCursorMove = this._register(new v);
    this.onCursorMove = this._onCursorMove.event;
    this._onLineFeed = this._register(new v);
    this.onLineFeed = this._onLineFeed.event;
    this._onScroll = this._register(new v);
    this.onScroll = this._onScroll.event;
    this._onTitleChange = this._register(new v);
    this.onTitleChange = this._onTitleChange.event;
    this._onColor = this._register(new v);
    this.onColor = this._onColor.event;
    this._parseStack = { paused: false, cursorStartX: 0, cursorStartY: 0, decodedLength: 0, position: 0 };
    this._specialColors = [256, 257, 258];
    this._register(this._parser), this._dirtyRowTracker = new Zi(this._bufferService), this._activeBuffer = this._bufferService.buffer, this._register(this._bufferService.buffers.onBufferActivate((c) => this._activeBuffer = c.activeBuffer)), this._parser.setCsiHandlerFallback((c, d) => {
      this._logService.debug("Unknown CSI code: ", { identifier: this._parser.identToString(c), params: d.toArray() });
    }), this._parser.setEscHandlerFallback((c) => {
      this._logService.debug("Unknown ESC code: ", { identifier: this._parser.identToString(c) });
    }), this._parser.setExecuteHandlerFallback((c) => {
      this._logService.debug("Unknown EXECUTE code: ", { code: c });
    }), this._parser.setOscHandlerFallback((c, d, _) => {
      this._logService.debug("Unknown OSC code: ", { identifier: c, action: d, data: _ });
    }), this._parser.setDcsHandlerFallback((c, d, _) => {
      d === "HOOK" && (_ = _.toArray()), this._logService.debug("Unknown DCS code: ", { identifier: this._parser.identToString(c), action: d, payload: _ });
    }), this._parser.setPrintHandler((c, d, _) => this.print(c, d, _)), this._parser.registerCsiHandler({ final: "@" }, (c) => this.insertChars(c)), this._parser.registerCsiHandler({ intermediates: " ", final: "@" }, (c) => this.scrollLeft(c)), this._parser.registerCsiHandler({ final: "A" }, (c) => this.cursorUp(c)), this._parser.registerCsiHandler({ intermediates: " ", final: "A" }, (c) => this.scrollRight(c)), this._parser.registerCsiHandler({ final: "B" }, (c) => this.cursorDown(c)), this._parser.registerCsiHandler({ final: "C" }, (c) => this.cursorForward(c)), this._parser.registerCsiHandler({ final: "D" }, (c) => this.cursorBackward(c)), this._parser.registerCsiHandler({ final: "E" }, (c) => this.cursorNextLine(c)), this._parser.registerCsiHandler({ final: "F" }, (c) => this.cursorPrecedingLine(c)), this._parser.registerCsiHandler({ final: "G" }, (c) => this.cursorCharAbsolute(c)), this._parser.registerCsiHandler({ final: "H" }, (c) => this.cursorPosition(c)), this._parser.registerCsiHandler({ final: "I" }, (c) => this.cursorForwardTab(c)), this._parser.registerCsiHandler({ final: "J" }, (c) => this.eraseInDisplay(c, false)), this._parser.registerCsiHandler({ prefix: "?", final: "J" }, (c) => this.eraseInDisplay(c, true)), this._parser.registerCsiHandler({ final: "K" }, (c) => this.eraseInLine(c, false)), this._parser.registerCsiHandler({ prefix: "?", final: "K" }, (c) => this.eraseInLine(c, true)), this._parser.registerCsiHandler({ final: "L" }, (c) => this.insertLines(c)), this._parser.registerCsiHandler({ final: "M" }, (c) => this.deleteLines(c)), this._parser.registerCsiHandler({ final: "P" }, (c) => this.deleteChars(c)), this._parser.registerCsiHandler({ final: "S" }, (c) => this.scrollUp(c)), this._parser.registerCsiHandler({ final: "T" }, (c) => this.scrollDown(c)), this._parser.registerCsiHandler({ final: "X" }, (c) => this.eraseChars(c)), this._parser.registerCsiHandler({ final: "Z" }, (c) => this.cursorBackwardTab(c)), this._parser.registerCsiHandler({ final: "`" }, (c) => this.charPosAbsolute(c)), this._parser.registerCsiHandler({ final: "a" }, (c) => this.hPositionRelative(c)), this._parser.registerCsiHandler({ final: "b" }, (c) => this.repeatPrecedingCharacter(c)), this._parser.registerCsiHandler({ final: "c" }, (c) => this.sendDeviceAttributesPrimary(c)), this._parser.registerCsiHandler({ prefix: ">", final: "c" }, (c) => this.sendDeviceAttributesSecondary(c)), this._parser.registerCsiHandler({ final: "d" }, (c) => this.linePosAbsolute(c)), this._parser.registerCsiHandler({ final: "e" }, (c) => this.vPositionRelative(c)), this._parser.registerCsiHandler({ final: "f" }, (c) => this.hVPosition(c)), this._parser.registerCsiHandler({ final: "g" }, (c) => this.tabClear(c)), this._parser.registerCsiHandler({ final: "h" }, (c) => this.setMode(c)), this._parser.registerCsiHandler({ prefix: "?", final: "h" }, (c) => this.setModePrivate(c)), this._parser.registerCsiHandler({ final: "l" }, (c) => this.resetMode(c)), this._parser.registerCsiHandler({ prefix: "?", final: "l" }, (c) => this.resetModePrivate(c)), this._parser.registerCsiHandler({ final: "m" }, (c) => this.charAttributes(c)), this._parser.registerCsiHandler({ final: "n" }, (c) => this.deviceStatus(c)), this._parser.registerCsiHandler({ prefix: "?", final: "n" }, (c) => this.deviceStatusPrivate(c)), this._parser.registerCsiHandler({ intermediates: "!", final: "p" }, (c) => this.softReset(c)), this._parser.registerCsiHandler({ intermediates: " ", final: "q" }, (c) => this.setCursorStyle(c)), this._parser.registerCsiHandler({ final: "r" }, (c) => this.setScrollRegion(c)), this._parser.registerCsiHandler({ final: "s" }, (c) => this.saveCursor(c)), this._parser.registerCsiHandler({ final: "t" }, (c) => this.windowOptions(c)), this._parser.registerCsiHandler({ final: "u" }, (c) => this.restoreCursor(c)), this._parser.registerCsiHandler({ intermediates: "'", final: "}" }, (c) => this.insertColumns(c)), this._parser.registerCsiHandler({ intermediates: "'", final: "~" }, (c) => this.deleteColumns(c)), this._parser.registerCsiHandler({ intermediates: '"', final: "q" }, (c) => this.selectProtected(c)), this._parser.registerCsiHandler({ intermediates: "$", final: "p" }, (c) => this.requestMode(c, true)), this._parser.registerCsiHandler({ prefix: "?", intermediates: "$", final: "p" }, (c) => this.requestMode(c, false)), this._parser.setExecuteHandler(b.BEL, () => this.bell()), this._parser.setExecuteHandler(b.LF, () => this.lineFeed()), this._parser.setExecuteHandler(b.VT, () => this.lineFeed()), this._parser.setExecuteHandler(b.FF, () => this.lineFeed()), this._parser.setExecuteHandler(b.CR, () => this.carriageReturn()), this._parser.setExecuteHandler(b.BS, () => this.backspace()), this._parser.setExecuteHandler(b.HT, () => this.tab()), this._parser.setExecuteHandler(b.SO, () => this.shiftOut()), this._parser.setExecuteHandler(b.SI, () => this.shiftIn()), this._parser.setExecuteHandler(Ai2.IND, () => this.index()), this._parser.setExecuteHandler(Ai2.NEL, () => this.nextLine()), this._parser.setExecuteHandler(Ai2.HTS, () => this.tabSet()), this._parser.registerOscHandler(0, new pe2((c) => (this.setTitle(c), this.setIconName(c), true))), this._parser.registerOscHandler(1, new pe2((c) => this.setIconName(c))), this._parser.registerOscHandler(2, new pe2((c) => this.setTitle(c))), this._parser.registerOscHandler(4, new pe2((c) => this.setOrReportIndexedColor(c))), this._parser.registerOscHandler(8, new pe2((c) => this.setHyperlink(c))), this._parser.registerOscHandler(10, new pe2((c) => this.setOrReportFgColor(c))), this._parser.registerOscHandler(11, new pe2((c) => this.setOrReportBgColor(c))), this._parser.registerOscHandler(12, new pe2((c) => this.setOrReportCursorColor(c))), this._parser.registerOscHandler(104, new pe2((c) => this.restoreIndexedColor(c))), this._parser.registerOscHandler(110, new pe2((c) => this.restoreFgColor(c))), this._parser.registerOscHandler(111, new pe2((c) => this.restoreBgColor(c))), this._parser.registerOscHandler(112, new pe2((c) => this.restoreCursorColor(c))), this._parser.registerEscHandler({ final: "7" }, () => this.saveCursor()), this._parser.registerEscHandler({ final: "8" }, () => this.restoreCursor()), this._parser.registerEscHandler({ final: "D" }, () => this.index()), this._parser.registerEscHandler({ final: "E" }, () => this.nextLine()), this._parser.registerEscHandler({ final: "H" }, () => this.tabSet()), this._parser.registerEscHandler({ final: "M" }, () => this.reverseIndex()), this._parser.registerEscHandler({ final: "=" }, () => this.keypadApplicationMode()), this._parser.registerEscHandler({ final: ">" }, () => this.keypadNumericMode()), this._parser.registerEscHandler({ final: "c" }, () => this.fullReset()), this._parser.registerEscHandler({ final: "n" }, () => this.setgLevel(2)), this._parser.registerEscHandler({ final: "o" }, () => this.setgLevel(3)), this._parser.registerEscHandler({ final: "|" }, () => this.setgLevel(3)), this._parser.registerEscHandler({ final: "}" }, () => this.setgLevel(2)), this._parser.registerEscHandler({ final: "~" }, () => this.setgLevel(1)), this._parser.registerEscHandler({ intermediates: "%", final: "@" }, () => this.selectDefaultCharset()), this._parser.registerEscHandler({ intermediates: "%", final: "G" }, () => this.selectDefaultCharset());
    for (let c in ne)
      this._parser.registerEscHandler({ intermediates: "(", final: c }, () => this.selectCharset("(" + c)), this._parser.registerEscHandler({ intermediates: ")", final: c }, () => this.selectCharset(")" + c)), this._parser.registerEscHandler({ intermediates: "*", final: c }, () => this.selectCharset("*" + c)), this._parser.registerEscHandler({ intermediates: "+", final: c }, () => this.selectCharset("+" + c)), this._parser.registerEscHandler({ intermediates: "-", final: c }, () => this.selectCharset("-" + c)), this._parser.registerEscHandler({ intermediates: ".", final: c }, () => this.selectCharset("." + c)), this._parser.registerEscHandler({ intermediates: "/", final: c }, () => this.selectCharset("/" + c));
    this._parser.registerEscHandler({ intermediates: "#", final: "8" }, () => this.screenAlignmentPattern()), this._parser.setErrorHandler((c) => (this._logService.error("Parsing error: ", c), c)), this._parser.registerDcsHandler({ intermediates: "$", final: "q" }, new Xi((c, d) => this.requestStatusString(c, d)));
  }
  getAttrData() {
    return this._curAttrData;
  }
  _preserveStack(e, i, r, n) {
    this._parseStack.paused = true, this._parseStack.cursorStartX = e, this._parseStack.cursorStartY = i, this._parseStack.decodedLength = r, this._parseStack.position = n;
  }
  _logSlowResolvingAsync(e) {
    this._logService.logLevel <= 3 && Promise.race([e, new Promise((i, r) => setTimeout(() => r("#SLOW_TIMEOUT"), vl))]).catch((i) => {
      if (i !== "#SLOW_TIMEOUT")
        throw i;
      console.warn(`async parser handler taking longer than ${vl} ms`);
    });
  }
  _getCurrentLinkId() {
    return this._curAttrData.extended.urlId;
  }
  parse(e, i) {
    let r, n = this._activeBuffer.x, o = this._activeBuffer.y, l = 0, a = this._parseStack.paused;
    if (a) {
      if (r = this._parser.parse(this._parseBuffer, this._parseStack.decodedLength, i))
        return this._logSlowResolvingAsync(r), r;
      n = this._parseStack.cursorStartX, o = this._parseStack.cursorStartY, this._parseStack.paused = false, e.length > ut2 && (l = this._parseStack.position + ut2);
    }
    if (this._logService.logLevel <= 1 && this._logService.debug(`parsing data ${typeof e == "string" ? ` "${e}"` : ` "${Array.prototype.map.call(e, (c) => String.fromCharCode(c)).join("")}"`}`), this._logService.logLevel === 0 && this._logService.trace("parsing data (codes)", typeof e == "string" ? e.split("").map((c) => c.charCodeAt(0)) : e), this._parseBuffer.length < e.length && this._parseBuffer.length < ut2 && (this._parseBuffer = new Uint32Array(Math.min(e.length, ut2))), a || this._dirtyRowTracker.clearRange(), e.length > ut2)
      for (let c = l;c < e.length; c += ut2) {
        let d = c + ut2 < e.length ? c + ut2 : e.length, _ = typeof e == "string" ? this._stringDecoder.decode(e.substring(c, d), this._parseBuffer) : this._utf8Decoder.decode(e.subarray(c, d), this._parseBuffer);
        if (r = this._parser.parse(this._parseBuffer, _))
          return this._preserveStack(n, o, _, c), this._logSlowResolvingAsync(r), r;
      }
    else if (!a) {
      let c = typeof e == "string" ? this._stringDecoder.decode(e, this._parseBuffer) : this._utf8Decoder.decode(e, this._parseBuffer);
      if (r = this._parser.parse(this._parseBuffer, c))
        return this._preserveStack(n, o, c, 0), this._logSlowResolvingAsync(r), r;
    }
    (this._activeBuffer.x !== n || this._activeBuffer.y !== o) && this._onCursorMove.fire();
    let u = this._dirtyRowTracker.end + (this._bufferService.buffer.ybase - this._bufferService.buffer.ydisp), h = this._dirtyRowTracker.start + (this._bufferService.buffer.ybase - this._bufferService.buffer.ydisp);
    h < this._bufferService.rows && this._onRequestRefreshRows.fire({ start: Math.min(h, this._bufferService.rows - 1), end: Math.min(u, this._bufferService.rows - 1) });
  }
  print(e, i, r) {
    let n, o, l = this._charsetService.charset, a = this._optionsService.rawOptions.screenReaderMode, u = this._bufferService.cols, h = this._coreService.decPrivateModes.wraparound, c = this._coreService.modes.insertMode, d = this._curAttrData, _ = this._activeBuffer.lines.get(this._activeBuffer.ybase + this._activeBuffer.y);
    this._dirtyRowTracker.markDirty(this._activeBuffer.y), this._activeBuffer.x && r - i > 0 && _.getWidth(this._activeBuffer.x - 1) === 2 && _.setCellFromCodepoint(this._activeBuffer.x - 1, 0, 1, d);
    let p = this._parser.precedingJoinState;
    for (let m = i;m < r; ++m) {
      if (n = e[m], n < 127 && l) {
        let O = l[String.fromCharCode(n)];
        O && (n = O.charCodeAt(0));
      }
      let f = this._unicodeService.charProperties(n, p);
      o = Ae2.extractWidth(f);
      let A = Ae2.extractShouldJoin(f), R = A ? Ae2.extractWidth(p) : 0;
      if (p = f, a && this._onA11yChar.fire(Ce2(n)), this._getCurrentLinkId() && this._oscLinkService.addLineToLink(this._getCurrentLinkId(), this._activeBuffer.ybase + this._activeBuffer.y), this._activeBuffer.x + o - R > u) {
        if (h) {
          let O = _, I = this._activeBuffer.x - R;
          for (this._activeBuffer.x = R, this._activeBuffer.y++, this._activeBuffer.y === this._activeBuffer.scrollBottom + 1 ? (this._activeBuffer.y--, this._bufferService.scroll(this._eraseAttrData(), true)) : (this._activeBuffer.y >= this._bufferService.rows && (this._activeBuffer.y = this._bufferService.rows - 1), this._activeBuffer.lines.get(this._activeBuffer.ybase + this._activeBuffer.y).isWrapped = true), _ = this._activeBuffer.lines.get(this._activeBuffer.ybase + this._activeBuffer.y), R > 0 && _ instanceof Ze2 && _.copyCellsFrom(O, I, 0, R, false);I < u; )
            O.setCellFromCodepoint(I++, 0, 1, d);
        } else if (this._activeBuffer.x = u - 1, o === 2)
          continue;
      }
      if (A && this._activeBuffer.x) {
        let O = _.getWidth(this._activeBuffer.x - 1) ? 1 : 2;
        _.addCodepointToCell(this._activeBuffer.x - O, n, o);
        for (let I = o - R;--I >= 0; )
          _.setCellFromCodepoint(this._activeBuffer.x++, 0, 0, d);
        continue;
      }
      if (c && (_.insertCells(this._activeBuffer.x, o - R, this._activeBuffer.getNullCell(d)), _.getWidth(u - 1) === 2 && _.setCellFromCodepoint(u - 1, 0, 1, d)), _.setCellFromCodepoint(this._activeBuffer.x++, n, o, d), o > 0)
        for (;--o; )
          _.setCellFromCodepoint(this._activeBuffer.x++, 0, 0, d);
    }
    this._parser.precedingJoinState = p, this._activeBuffer.x < u && r - i > 0 && _.getWidth(this._activeBuffer.x) === 0 && !_.hasContent(this._activeBuffer.x) && _.setCellFromCodepoint(this._activeBuffer.x, 0, 1, d), this._dirtyRowTracker.markDirty(this._activeBuffer.y);
  }
  registerCsiHandler(e, i) {
    return e.final === "t" && !e.prefix && !e.intermediates ? this._parser.registerCsiHandler(e, (r) => bl(r.params[0], this._optionsService.rawOptions.windowOptions) ? i(r) : true) : this._parser.registerCsiHandler(e, i);
  }
  registerDcsHandler(e, i) {
    return this._parser.registerDcsHandler(e, new Xi(i));
  }
  registerEscHandler(e, i) {
    return this._parser.registerEscHandler(e, i);
  }
  registerOscHandler(e, i) {
    return this._parser.registerOscHandler(e, new pe2(i));
  }
  bell() {
    return this._onRequestBell.fire(), true;
  }
  lineFeed() {
    return this._dirtyRowTracker.markDirty(this._activeBuffer.y), this._optionsService.rawOptions.convertEol && (this._activeBuffer.x = 0), this._activeBuffer.y++, this._activeBuffer.y === this._activeBuffer.scrollBottom + 1 ? (this._activeBuffer.y--, this._bufferService.scroll(this._eraseAttrData())) : this._activeBuffer.y >= this._bufferService.rows ? this._activeBuffer.y = this._bufferService.rows - 1 : this._activeBuffer.lines.get(this._activeBuffer.ybase + this._activeBuffer.y).isWrapped = false, this._activeBuffer.x >= this._bufferService.cols && this._activeBuffer.x--, this._dirtyRowTracker.markDirty(this._activeBuffer.y), this._onLineFeed.fire(), true;
  }
  carriageReturn() {
    return this._activeBuffer.x = 0, true;
  }
  backspace() {
    if (!this._coreService.decPrivateModes.reverseWraparound)
      return this._restrictCursor(), this._activeBuffer.x > 0 && this._activeBuffer.x--, true;
    if (this._restrictCursor(this._bufferService.cols), this._activeBuffer.x > 0)
      this._activeBuffer.x--;
    else if (this._activeBuffer.x === 0 && this._activeBuffer.y > this._activeBuffer.scrollTop && this._activeBuffer.y <= this._activeBuffer.scrollBottom && this._activeBuffer.lines.get(this._activeBuffer.ybase + this._activeBuffer.y)?.isWrapped) {
      this._activeBuffer.lines.get(this._activeBuffer.ybase + this._activeBuffer.y).isWrapped = false, this._activeBuffer.y--, this._activeBuffer.x = this._bufferService.cols - 1;
      let e = this._activeBuffer.lines.get(this._activeBuffer.ybase + this._activeBuffer.y);
      e.hasWidth(this._activeBuffer.x) && !e.hasContent(this._activeBuffer.x) && this._activeBuffer.x--;
    }
    return this._restrictCursor(), true;
  }
  tab() {
    if (this._activeBuffer.x >= this._bufferService.cols)
      return true;
    let e = this._activeBuffer.x;
    return this._activeBuffer.x = this._activeBuffer.nextStop(), this._optionsService.rawOptions.screenReaderMode && this._onA11yTab.fire(this._activeBuffer.x - e), true;
  }
  shiftOut() {
    return this._charsetService.setgLevel(1), true;
  }
  shiftIn() {
    return this._charsetService.setgLevel(0), true;
  }
  _restrictCursor(e = this._bufferService.cols - 1) {
    this._activeBuffer.x = Math.min(e, Math.max(0, this._activeBuffer.x)), this._activeBuffer.y = this._coreService.decPrivateModes.origin ? Math.min(this._activeBuffer.scrollBottom, Math.max(this._activeBuffer.scrollTop, this._activeBuffer.y)) : Math.min(this._bufferService.rows - 1, Math.max(0, this._activeBuffer.y)), this._dirtyRowTracker.markDirty(this._activeBuffer.y);
  }
  _setCursor(e, i) {
    this._dirtyRowTracker.markDirty(this._activeBuffer.y), this._coreService.decPrivateModes.origin ? (this._activeBuffer.x = e, this._activeBuffer.y = this._activeBuffer.scrollTop + i) : (this._activeBuffer.x = e, this._activeBuffer.y = i), this._restrictCursor(), this._dirtyRowTracker.markDirty(this._activeBuffer.y);
  }
  _moveCursor(e, i) {
    this._restrictCursor(), this._setCursor(this._activeBuffer.x + e, this._activeBuffer.y + i);
  }
  cursorUp(e) {
    let i = this._activeBuffer.y - this._activeBuffer.scrollTop;
    return i >= 0 ? this._moveCursor(0, -Math.min(i, e.params[0] || 1)) : this._moveCursor(0, -(e.params[0] || 1)), true;
  }
  cursorDown(e) {
    let i = this._activeBuffer.scrollBottom - this._activeBuffer.y;
    return i >= 0 ? this._moveCursor(0, Math.min(i, e.params[0] || 1)) : this._moveCursor(0, e.params[0] || 1), true;
  }
  cursorForward(e) {
    return this._moveCursor(e.params[0] || 1, 0), true;
  }
  cursorBackward(e) {
    return this._moveCursor(-(e.params[0] || 1), 0), true;
  }
  cursorNextLine(e) {
    return this.cursorDown(e), this._activeBuffer.x = 0, true;
  }
  cursorPrecedingLine(e) {
    return this.cursorUp(e), this._activeBuffer.x = 0, true;
  }
  cursorCharAbsolute(e) {
    return this._setCursor((e.params[0] || 1) - 1, this._activeBuffer.y), true;
  }
  cursorPosition(e) {
    return this._setCursor(e.length >= 2 ? (e.params[1] || 1) - 1 : 0, (e.params[0] || 1) - 1), true;
  }
  charPosAbsolute(e) {
    return this._setCursor((e.params[0] || 1) - 1, this._activeBuffer.y), true;
  }
  hPositionRelative(e) {
    return this._moveCursor(e.params[0] || 1, 0), true;
  }
  linePosAbsolute(e) {
    return this._setCursor(this._activeBuffer.x, (e.params[0] || 1) - 1), true;
  }
  vPositionRelative(e) {
    return this._moveCursor(0, e.params[0] || 1), true;
  }
  hVPosition(e) {
    return this.cursorPosition(e), true;
  }
  tabClear(e) {
    let i = e.params[0];
    return i === 0 ? delete this._activeBuffer.tabs[this._activeBuffer.x] : i === 3 && (this._activeBuffer.tabs = {}), true;
  }
  cursorForwardTab(e) {
    if (this._activeBuffer.x >= this._bufferService.cols)
      return true;
    let i = e.params[0] || 1;
    for (;i--; )
      this._activeBuffer.x = this._activeBuffer.nextStop();
    return true;
  }
  cursorBackwardTab(e) {
    if (this._activeBuffer.x >= this._bufferService.cols)
      return true;
    let i = e.params[0] || 1;
    for (;i--; )
      this._activeBuffer.x = this._activeBuffer.prevStop();
    return true;
  }
  selectProtected(e) {
    let i = e.params[0];
    return i === 1 && (this._curAttrData.bg |= 536870912), (i === 2 || i === 0) && (this._curAttrData.bg &= -536870913), true;
  }
  _eraseInBufferLine(e, i, r, n = false, o = false) {
    let l = this._activeBuffer.lines.get(this._activeBuffer.ybase + e);
    l.replaceCells(i, r, this._activeBuffer.getNullCell(this._eraseAttrData()), o), n && (l.isWrapped = false);
  }
  _resetBufferLine(e, i = false) {
    let r = this._activeBuffer.lines.get(this._activeBuffer.ybase + e);
    r && (r.fill(this._activeBuffer.getNullCell(this._eraseAttrData()), i), this._bufferService.buffer.clearMarkers(this._activeBuffer.ybase + e), r.isWrapped = false);
  }
  eraseInDisplay(e, i = false) {
    this._restrictCursor(this._bufferService.cols);
    let r;
    switch (e.params[0]) {
      case 0:
        for (r = this._activeBuffer.y, this._dirtyRowTracker.markDirty(r), this._eraseInBufferLine(r++, this._activeBuffer.x, this._bufferService.cols, this._activeBuffer.x === 0, i);r < this._bufferService.rows; r++)
          this._resetBufferLine(r, i);
        this._dirtyRowTracker.markDirty(r);
        break;
      case 1:
        for (r = this._activeBuffer.y, this._dirtyRowTracker.markDirty(r), this._eraseInBufferLine(r, 0, this._activeBuffer.x + 1, true, i), this._activeBuffer.x + 1 >= this._bufferService.cols && (this._activeBuffer.lines.get(r + 1).isWrapped = false);r--; )
          this._resetBufferLine(r, i);
        this._dirtyRowTracker.markDirty(0);
        break;
      case 2:
        if (this._optionsService.rawOptions.scrollOnEraseInDisplay) {
          for (r = this._bufferService.rows, this._dirtyRowTracker.markRangeDirty(0, r - 1);r-- && !this._activeBuffer.lines.get(this._activeBuffer.ybase + r)?.getTrimmedLength(); )
            ;
          for (;r >= 0; r--)
            this._bufferService.scroll(this._eraseAttrData());
        } else {
          for (r = this._bufferService.rows, this._dirtyRowTracker.markDirty(r - 1);r--; )
            this._resetBufferLine(r, i);
          this._dirtyRowTracker.markDirty(0);
        }
        break;
      case 3:
        let n = this._activeBuffer.lines.length - this._bufferService.rows;
        n > 0 && (this._activeBuffer.lines.trimStart(n), this._activeBuffer.ybase = Math.max(this._activeBuffer.ybase - n, 0), this._activeBuffer.ydisp = Math.max(this._activeBuffer.ydisp - n, 0), this._onScroll.fire(0));
        break;
    }
    return true;
  }
  eraseInLine(e, i = false) {
    switch (this._restrictCursor(this._bufferService.cols), e.params[0]) {
      case 0:
        this._eraseInBufferLine(this._activeBuffer.y, this._activeBuffer.x, this._bufferService.cols, this._activeBuffer.x === 0, i);
        break;
      case 1:
        this._eraseInBufferLine(this._activeBuffer.y, 0, this._activeBuffer.x + 1, false, i);
        break;
      case 2:
        this._eraseInBufferLine(this._activeBuffer.y, 0, this._bufferService.cols, true, i);
        break;
    }
    return this._dirtyRowTracker.markDirty(this._activeBuffer.y), true;
  }
  insertLines(e) {
    this._restrictCursor();
    let i = e.params[0] || 1;
    if (this._activeBuffer.y > this._activeBuffer.scrollBottom || this._activeBuffer.y < this._activeBuffer.scrollTop)
      return true;
    let r = this._activeBuffer.ybase + this._activeBuffer.y, n = this._bufferService.rows - 1 - this._activeBuffer.scrollBottom, o = this._bufferService.rows - 1 + this._activeBuffer.ybase - n + 1;
    for (;i--; )
      this._activeBuffer.lines.splice(o - 1, 1), this._activeBuffer.lines.splice(r, 0, this._activeBuffer.getBlankLine(this._eraseAttrData()));
    return this._dirtyRowTracker.markRangeDirty(this._activeBuffer.y, this._activeBuffer.scrollBottom), this._activeBuffer.x = 0, true;
  }
  deleteLines(e) {
    this._restrictCursor();
    let i = e.params[0] || 1;
    if (this._activeBuffer.y > this._activeBuffer.scrollBottom || this._activeBuffer.y < this._activeBuffer.scrollTop)
      return true;
    let r = this._activeBuffer.ybase + this._activeBuffer.y, n;
    for (n = this._bufferService.rows - 1 - this._activeBuffer.scrollBottom, n = this._bufferService.rows - 1 + this._activeBuffer.ybase - n;i--; )
      this._activeBuffer.lines.splice(r, 1), this._activeBuffer.lines.splice(n, 0, this._activeBuffer.getBlankLine(this._eraseAttrData()));
    return this._dirtyRowTracker.markRangeDirty(this._activeBuffer.y, this._activeBuffer.scrollBottom), this._activeBuffer.x = 0, true;
  }
  insertChars(e) {
    this._restrictCursor();
    let i = this._activeBuffer.lines.get(this._activeBuffer.ybase + this._activeBuffer.y);
    return i && (i.insertCells(this._activeBuffer.x, e.params[0] || 1, this._activeBuffer.getNullCell(this._eraseAttrData())), this._dirtyRowTracker.markDirty(this._activeBuffer.y)), true;
  }
  deleteChars(e) {
    this._restrictCursor();
    let i = this._activeBuffer.lines.get(this._activeBuffer.ybase + this._activeBuffer.y);
    return i && (i.deleteCells(this._activeBuffer.x, e.params[0] || 1, this._activeBuffer.getNullCell(this._eraseAttrData())), this._dirtyRowTracker.markDirty(this._activeBuffer.y)), true;
  }
  scrollUp(e) {
    let i = e.params[0] || 1;
    for (;i--; )
      this._activeBuffer.lines.splice(this._activeBuffer.ybase + this._activeBuffer.scrollTop, 1), this._activeBuffer.lines.splice(this._activeBuffer.ybase + this._activeBuffer.scrollBottom, 0, this._activeBuffer.getBlankLine(this._eraseAttrData()));
    return this._dirtyRowTracker.markRangeDirty(this._activeBuffer.scrollTop, this._activeBuffer.scrollBottom), true;
  }
  scrollDown(e) {
    let i = e.params[0] || 1;
    for (;i--; )
      this._activeBuffer.lines.splice(this._activeBuffer.ybase + this._activeBuffer.scrollBottom, 1), this._activeBuffer.lines.splice(this._activeBuffer.ybase + this._activeBuffer.scrollTop, 0, this._activeBuffer.getBlankLine(X2));
    return this._dirtyRowTracker.markRangeDirty(this._activeBuffer.scrollTop, this._activeBuffer.scrollBottom), true;
  }
  scrollLeft(e) {
    if (this._activeBuffer.y > this._activeBuffer.scrollBottom || this._activeBuffer.y < this._activeBuffer.scrollTop)
      return true;
    let i = e.params[0] || 1;
    for (let r = this._activeBuffer.scrollTop;r <= this._activeBuffer.scrollBottom; ++r) {
      let n = this._activeBuffer.lines.get(this._activeBuffer.ybase + r);
      n.deleteCells(0, i, this._activeBuffer.getNullCell(this._eraseAttrData())), n.isWrapped = false;
    }
    return this._dirtyRowTracker.markRangeDirty(this._activeBuffer.scrollTop, this._activeBuffer.scrollBottom), true;
  }
  scrollRight(e) {
    if (this._activeBuffer.y > this._activeBuffer.scrollBottom || this._activeBuffer.y < this._activeBuffer.scrollTop)
      return true;
    let i = e.params[0] || 1;
    for (let r = this._activeBuffer.scrollTop;r <= this._activeBuffer.scrollBottom; ++r) {
      let n = this._activeBuffer.lines.get(this._activeBuffer.ybase + r);
      n.insertCells(0, i, this._activeBuffer.getNullCell(this._eraseAttrData())), n.isWrapped = false;
    }
    return this._dirtyRowTracker.markRangeDirty(this._activeBuffer.scrollTop, this._activeBuffer.scrollBottom), true;
  }
  insertColumns(e) {
    if (this._activeBuffer.y > this._activeBuffer.scrollBottom || this._activeBuffer.y < this._activeBuffer.scrollTop)
      return true;
    let i = e.params[0] || 1;
    for (let r = this._activeBuffer.scrollTop;r <= this._activeBuffer.scrollBottom; ++r) {
      let n = this._activeBuffer.lines.get(this._activeBuffer.ybase + r);
      n.insertCells(this._activeBuffer.x, i, this._activeBuffer.getNullCell(this._eraseAttrData())), n.isWrapped = false;
    }
    return this._dirtyRowTracker.markRangeDirty(this._activeBuffer.scrollTop, this._activeBuffer.scrollBottom), true;
  }
  deleteColumns(e) {
    if (this._activeBuffer.y > this._activeBuffer.scrollBottom || this._activeBuffer.y < this._activeBuffer.scrollTop)
      return true;
    let i = e.params[0] || 1;
    for (let r = this._activeBuffer.scrollTop;r <= this._activeBuffer.scrollBottom; ++r) {
      let n = this._activeBuffer.lines.get(this._activeBuffer.ybase + r);
      n.deleteCells(this._activeBuffer.x, i, this._activeBuffer.getNullCell(this._eraseAttrData())), n.isWrapped = false;
    }
    return this._dirtyRowTracker.markRangeDirty(this._activeBuffer.scrollTop, this._activeBuffer.scrollBottom), true;
  }
  eraseChars(e) {
    this._restrictCursor();
    let i = this._activeBuffer.lines.get(this._activeBuffer.ybase + this._activeBuffer.y);
    return i && (i.replaceCells(this._activeBuffer.x, this._activeBuffer.x + (e.params[0] || 1), this._activeBuffer.getNullCell(this._eraseAttrData())), this._dirtyRowTracker.markDirty(this._activeBuffer.y)), true;
  }
  repeatPrecedingCharacter(e) {
    let i = this._parser.precedingJoinState;
    if (!i)
      return true;
    let r = e.params[0] || 1, n = Ae2.extractWidth(i), o = this._activeBuffer.x - n, a = this._activeBuffer.lines.get(this._activeBuffer.ybase + this._activeBuffer.y).getString(o), u = new Uint32Array(a.length * r), h = 0;
    for (let d = 0;d < a.length; ) {
      let _ = a.codePointAt(d) || 0;
      u[h++] = _, d += _ > 65535 ? 2 : 1;
    }
    let c = h;
    for (let d = 1;d < r; ++d)
      u.copyWithin(c, 0, h), c += h;
    return this.print(u, 0, c), true;
  }
  sendDeviceAttributesPrimary(e) {
    return e.params[0] > 0 || (this._is("xterm") || this._is("rxvt-unicode") || this._is("screen") ? this._coreService.triggerDataEvent(b.ESC + "[?1;2c") : this._is("linux") && this._coreService.triggerDataEvent(b.ESC + "[?6c")), true;
  }
  sendDeviceAttributesSecondary(e) {
    return e.params[0] > 0 || (this._is("xterm") ? this._coreService.triggerDataEvent(b.ESC + "[>0;276;0c") : this._is("rxvt-unicode") ? this._coreService.triggerDataEvent(b.ESC + "[>85;95;0c") : this._is("linux") ? this._coreService.triggerDataEvent(e.params[0] + "c") : this._is("screen") && this._coreService.triggerDataEvent(b.ESC + "[>83;40003;0c")), true;
  }
  _is(e) {
    return (this._optionsService.rawOptions.termName + "").indexOf(e) === 0;
  }
  setMode(e) {
    for (let i = 0;i < e.length; i++)
      switch (e.params[i]) {
        case 4:
          this._coreService.modes.insertMode = true;
          break;
        case 20:
          this._optionsService.options.convertEol = true;
          break;
      }
    return true;
  }
  setModePrivate(e) {
    for (let i = 0;i < e.length; i++)
      switch (e.params[i]) {
        case 1:
          this._coreService.decPrivateModes.applicationCursorKeys = true;
          break;
        case 2:
          this._charsetService.setgCharset(0, Je), this._charsetService.setgCharset(1, Je), this._charsetService.setgCharset(2, Je), this._charsetService.setgCharset(3, Je);
          break;
        case 3:
          this._optionsService.rawOptions.windowOptions.setWinLines && (this._bufferService.resize(132, this._bufferService.rows), this._onRequestReset.fire());
          break;
        case 6:
          this._coreService.decPrivateModes.origin = true, this._setCursor(0, 0);
          break;
        case 7:
          this._coreService.decPrivateModes.wraparound = true;
          break;
        case 12:
          this._optionsService.options.cursorBlink = true;
          break;
        case 45:
          this._coreService.decPrivateModes.reverseWraparound = true;
          break;
        case 66:
          this._logService.debug("Serial port requested application keypad."), this._coreService.decPrivateModes.applicationKeypad = true, this._onRequestSyncScrollBar.fire();
          break;
        case 9:
          this._coreMouseService.activeProtocol = "X10";
          break;
        case 1000:
          this._coreMouseService.activeProtocol = "VT200";
          break;
        case 1002:
          this._coreMouseService.activeProtocol = "DRAG";
          break;
        case 1003:
          this._coreMouseService.activeProtocol = "ANY";
          break;
        case 1004:
          this._coreService.decPrivateModes.sendFocus = true, this._onRequestSendFocus.fire();
          break;
        case 1005:
          this._logService.debug("DECSET 1005 not supported (see #2507)");
          break;
        case 1006:
          this._coreMouseService.activeEncoding = "SGR";
          break;
        case 1015:
          this._logService.debug("DECSET 1015 not supported (see #2507)");
          break;
        case 1016:
          this._coreMouseService.activeEncoding = "SGR_PIXELS";
          break;
        case 25:
          this._coreService.isCursorHidden = false;
          break;
        case 1048:
          this.saveCursor();
          break;
        case 1049:
          this.saveCursor();
        case 47:
        case 1047:
          this._bufferService.buffers.activateAltBuffer(this._eraseAttrData()), this._coreService.isCursorInitialized = true, this._onRequestRefreshRows.fire(undefined), this._onRequestSyncScrollBar.fire();
          break;
        case 2004:
          this._coreService.decPrivateModes.bracketedPasteMode = true;
          break;
        case 2026:
          this._coreService.decPrivateModes.synchronizedOutput = true;
          break;
      }
    return true;
  }
  resetMode(e) {
    for (let i = 0;i < e.length; i++)
      switch (e.params[i]) {
        case 4:
          this._coreService.modes.insertMode = false;
          break;
        case 20:
          this._optionsService.options.convertEol = false;
          break;
      }
    return true;
  }
  resetModePrivate(e) {
    for (let i = 0;i < e.length; i++)
      switch (e.params[i]) {
        case 1:
          this._coreService.decPrivateModes.applicationCursorKeys = false;
          break;
        case 3:
          this._optionsService.rawOptions.windowOptions.setWinLines && (this._bufferService.resize(80, this._bufferService.rows), this._onRequestReset.fire());
          break;
        case 6:
          this._coreService.decPrivateModes.origin = false, this._setCursor(0, 0);
          break;
        case 7:
          this._coreService.decPrivateModes.wraparound = false;
          break;
        case 12:
          this._optionsService.options.cursorBlink = false;
          break;
        case 45:
          this._coreService.decPrivateModes.reverseWraparound = false;
          break;
        case 66:
          this._logService.debug("Switching back to normal keypad."), this._coreService.decPrivateModes.applicationKeypad = false, this._onRequestSyncScrollBar.fire();
          break;
        case 9:
        case 1000:
        case 1002:
        case 1003:
          this._coreMouseService.activeProtocol = "NONE";
          break;
        case 1004:
          this._coreService.decPrivateModes.sendFocus = false;
          break;
        case 1005:
          this._logService.debug("DECRST 1005 not supported (see #2507)");
          break;
        case 1006:
          this._coreMouseService.activeEncoding = "DEFAULT";
          break;
        case 1015:
          this._logService.debug("DECRST 1015 not supported (see #2507)");
          break;
        case 1016:
          this._coreMouseService.activeEncoding = "DEFAULT";
          break;
        case 25:
          this._coreService.isCursorHidden = true;
          break;
        case 1048:
          this.restoreCursor();
          break;
        case 1049:
        case 47:
        case 1047:
          this._bufferService.buffers.activateNormalBuffer(), e.params[i] === 1049 && this.restoreCursor(), this._coreService.isCursorInitialized = true, this._onRequestRefreshRows.fire(undefined), this._onRequestSyncScrollBar.fire();
          break;
        case 2004:
          this._coreService.decPrivateModes.bracketedPasteMode = false;
          break;
        case 2026:
          this._coreService.decPrivateModes.synchronizedOutput = false, this._onRequestRefreshRows.fire(undefined);
          break;
      }
    return true;
  }
  requestMode(e, i) {
    let r;
    ((P) => (P[P.NOT_RECOGNIZED = 0] = "NOT_RECOGNIZED", P[P.SET = 1] = "SET", P[P.RESET = 2] = "RESET", P[P.PERMANENTLY_SET = 3] = "PERMANENTLY_SET", P[P.PERMANENTLY_RESET = 4] = "PERMANENTLY_RESET"))(r ||= {});
    let n = this._coreService.decPrivateModes, { activeProtocol: o, activeEncoding: l } = this._coreMouseService, a = this._coreService, { buffers: u, cols: h } = this._bufferService, { active: c, alt: d } = u, _ = this._optionsService.rawOptions, p = (A, R) => (a.triggerDataEvent(`${b.ESC}[${i ? "" : "?"}${A};${R}$y`), true), m = (A) => A ? 1 : 2, f = e.params[0];
    return i ? f === 2 ? p(f, 4) : f === 4 ? p(f, m(a.modes.insertMode)) : f === 12 ? p(f, 3) : f === 20 ? p(f, m(_.convertEol)) : p(f, 0) : f === 1 ? p(f, m(n.applicationCursorKeys)) : f === 3 ? p(f, _.windowOptions.setWinLines ? h === 80 ? 2 : h === 132 ? 1 : 0 : 0) : f === 6 ? p(f, m(n.origin)) : f === 7 ? p(f, m(n.wraparound)) : f === 8 ? p(f, 3) : f === 9 ? p(f, m(o === "X10")) : f === 12 ? p(f, m(_.cursorBlink)) : f === 25 ? p(f, m(!a.isCursorHidden)) : f === 45 ? p(f, m(n.reverseWraparound)) : f === 66 ? p(f, m(n.applicationKeypad)) : f === 67 ? p(f, 4) : f === 1000 ? p(f, m(o === "VT200")) : f === 1002 ? p(f, m(o === "DRAG")) : f === 1003 ? p(f, m(o === "ANY")) : f === 1004 ? p(f, m(n.sendFocus)) : f === 1005 ? p(f, 4) : f === 1006 ? p(f, m(l === "SGR")) : f === 1015 ? p(f, 4) : f === 1016 ? p(f, m(l === "SGR_PIXELS")) : f === 1048 ? p(f, 1) : f === 47 || f === 1047 || f === 1049 ? p(f, m(c === d)) : f === 2004 ? p(f, m(n.bracketedPasteMode)) : f === 2026 ? p(f, m(n.synchronizedOutput)) : p(f, 0);
  }
  _updateAttrColor(e, i, r, n, o) {
    return i === 2 ? (e |= 50331648, e &= -16777216, e |= De2.fromColorRGB([r, n, o])) : i === 5 && (e &= -50331904, e |= 33554432 | r & 255), e;
  }
  _extractColor(e, i, r) {
    let n = [0, 0, -1, 0, 0, 0], o = 0, l = 0;
    do {
      if (n[l + o] = e.params[i + l], e.hasSubParams(i + l)) {
        let a = e.getSubParams(i + l), u = 0;
        do
          n[1] === 5 && (o = 1), n[l + u + 1 + o] = a[u];
        while (++u < a.length && u + l + 1 + o < n.length);
        break;
      }
      if (n[1] === 5 && l + o >= 2 || n[1] === 2 && l + o >= 5)
        break;
      n[1] && (o = 1);
    } while (++l + i < e.length && l + o < n.length);
    for (let a = 2;a < n.length; ++a)
      n[a] === -1 && (n[a] = 0);
    switch (n[0]) {
      case 38:
        r.fg = this._updateAttrColor(r.fg, n[1], n[3], n[4], n[5]);
        break;
      case 48:
        r.bg = this._updateAttrColor(r.bg, n[1], n[3], n[4], n[5]);
        break;
      case 58:
        r.extended = r.extended.clone(), r.extended.underlineColor = this._updateAttrColor(r.extended.underlineColor, n[1], n[3], n[4], n[5]);
    }
    return l;
  }
  _processUnderline(e, i) {
    i.extended = i.extended.clone(), (!~e || e > 5) && (e = 1), i.extended.underlineStyle = e, i.fg |= 268435456, e === 0 && (i.fg &= -268435457), i.updateExtended();
  }
  _processSGR0(e) {
    e.fg = X2.fg, e.bg = X2.bg, e.extended = e.extended.clone(), e.extended.underlineStyle = 0, e.extended.underlineColor &= -67108864, e.updateExtended();
  }
  charAttributes(e) {
    if (e.length === 1 && e.params[0] === 0)
      return this._processSGR0(this._curAttrData), true;
    let i = e.length, r, n = this._curAttrData;
    for (let o = 0;o < i; o++)
      r = e.params[o], r >= 30 && r <= 37 ? (n.fg &= -50331904, n.fg |= 16777216 | r - 30) : r >= 40 && r <= 47 ? (n.bg &= -50331904, n.bg |= 16777216 | r - 40) : r >= 90 && r <= 97 ? (n.fg &= -50331904, n.fg |= 16777216 | r - 90 | 8) : r >= 100 && r <= 107 ? (n.bg &= -50331904, n.bg |= 16777216 | r - 100 | 8) : r === 0 ? this._processSGR0(n) : r === 1 ? n.fg |= 134217728 : r === 3 ? n.bg |= 67108864 : r === 4 ? (n.fg |= 268435456, this._processUnderline(e.hasSubParams(o) ? e.getSubParams(o)[0] : 1, n)) : r === 5 ? n.fg |= 536870912 : r === 7 ? n.fg |= 67108864 : r === 8 ? n.fg |= 1073741824 : r === 9 ? n.fg |= 2147483648 : r === 2 ? n.bg |= 134217728 : r === 21 ? this._processUnderline(2, n) : r === 22 ? (n.fg &= -134217729, n.bg &= -134217729) : r === 23 ? n.bg &= -67108865 : r === 24 ? (n.fg &= -268435457, this._processUnderline(0, n)) : r === 25 ? n.fg &= -536870913 : r === 27 ? n.fg &= -67108865 : r === 28 ? n.fg &= -1073741825 : r === 29 ? n.fg &= 2147483647 : r === 39 ? (n.fg &= -67108864, n.fg |= X2.fg & 16777215) : r === 49 ? (n.bg &= -67108864, n.bg |= X2.bg & 16777215) : r === 38 || r === 48 || r === 58 ? o += this._extractColor(e, o, n) : r === 53 ? n.bg |= 1073741824 : r === 55 ? n.bg &= -1073741825 : r === 59 ? (n.extended = n.extended.clone(), n.extended.underlineColor = -1, n.updateExtended()) : r === 100 ? (n.fg &= -67108864, n.fg |= X2.fg & 16777215, n.bg &= -67108864, n.bg |= X2.bg & 16777215) : this._logService.debug("Unknown SGR attribute: %d.", r);
    return true;
  }
  deviceStatus(e) {
    switch (e.params[0]) {
      case 5:
        this._coreService.triggerDataEvent(`${b.ESC}[0n`);
        break;
      case 6:
        let i = this._activeBuffer.y + 1, r = this._activeBuffer.x + 1;
        this._coreService.triggerDataEvent(`${b.ESC}[${i};${r}R`);
        break;
    }
    return true;
  }
  deviceStatusPrivate(e) {
    switch (e.params[0]) {
      case 6:
        let i = this._activeBuffer.y + 1, r = this._activeBuffer.x + 1;
        this._coreService.triggerDataEvent(`${b.ESC}[?${i};${r}R`);
        break;
      case 15:
        break;
      case 25:
        break;
      case 26:
        break;
      case 53:
        break;
    }
    return true;
  }
  softReset(e) {
    return this._coreService.isCursorHidden = false, this._onRequestSyncScrollBar.fire(), this._activeBuffer.scrollTop = 0, this._activeBuffer.scrollBottom = this._bufferService.rows - 1, this._curAttrData = X2.clone(), this._coreService.reset(), this._charsetService.reset(), this._activeBuffer.savedX = 0, this._activeBuffer.savedY = this._activeBuffer.ybase, this._activeBuffer.savedCurAttrData.fg = this._curAttrData.fg, this._activeBuffer.savedCurAttrData.bg = this._curAttrData.bg, this._activeBuffer.savedCharset = this._charsetService.charset, this._coreService.decPrivateModes.origin = false, true;
  }
  setCursorStyle(e) {
    let i = e.length === 0 ? 1 : e.params[0];
    if (i === 0)
      this._coreService.decPrivateModes.cursorStyle = undefined, this._coreService.decPrivateModes.cursorBlink = undefined;
    else {
      switch (i) {
        case 1:
        case 2:
          this._coreService.decPrivateModes.cursorStyle = "block";
          break;
        case 3:
        case 4:
          this._coreService.decPrivateModes.cursorStyle = "underline";
          break;
        case 5:
        case 6:
          this._coreService.decPrivateModes.cursorStyle = "bar";
          break;
      }
      let r = i % 2 === 1;
      this._coreService.decPrivateModes.cursorBlink = r;
    }
    return true;
  }
  setScrollRegion(e) {
    let i = e.params[0] || 1, r;
    return (e.length < 2 || (r = e.params[1]) > this._bufferService.rows || r === 0) && (r = this._bufferService.rows), r > i && (this._activeBuffer.scrollTop = i - 1, this._activeBuffer.scrollBottom = r - 1, this._setCursor(0, 0)), true;
  }
  windowOptions(e) {
    if (!bl(e.params[0], this._optionsService.rawOptions.windowOptions))
      return true;
    let i = e.length > 1 ? e.params[1] : 0;
    switch (e.params[0]) {
      case 14:
        i !== 2 && this._onRequestWindowsOptionsReport.fire(0);
        break;
      case 16:
        this._onRequestWindowsOptionsReport.fire(1);
        break;
      case 18:
        this._bufferService && this._coreService.triggerDataEvent(`${b.ESC}[8;${this._bufferService.rows};${this._bufferService.cols}t`);
        break;
      case 22:
        (i === 0 || i === 2) && (this._windowTitleStack.push(this._windowTitle), this._windowTitleStack.length > _l && this._windowTitleStack.shift()), (i === 0 || i === 1) && (this._iconNameStack.push(this._iconName), this._iconNameStack.length > _l && this._iconNameStack.shift());
        break;
      case 23:
        (i === 0 || i === 2) && this._windowTitleStack.length && this.setTitle(this._windowTitleStack.pop()), (i === 0 || i === 1) && this._iconNameStack.length && this.setIconName(this._iconNameStack.pop());
        break;
    }
    return true;
  }
  saveCursor(e) {
    return this._activeBuffer.savedX = this._activeBuffer.x, this._activeBuffer.savedY = this._activeBuffer.ybase + this._activeBuffer.y, this._activeBuffer.savedCurAttrData.fg = this._curAttrData.fg, this._activeBuffer.savedCurAttrData.bg = this._curAttrData.bg, this._activeBuffer.savedCharset = this._charsetService.charset, true;
  }
  restoreCursor(e) {
    return this._activeBuffer.x = this._activeBuffer.savedX || 0, this._activeBuffer.y = Math.max(this._activeBuffer.savedY - this._activeBuffer.ybase, 0), this._curAttrData.fg = this._activeBuffer.savedCurAttrData.fg, this._curAttrData.bg = this._activeBuffer.savedCurAttrData.bg, this._charsetService.charset = this._savedCharset, this._activeBuffer.savedCharset && (this._charsetService.charset = this._activeBuffer.savedCharset), this._restrictCursor(), true;
  }
  setTitle(e) {
    return this._windowTitle = e, this._onTitleChange.fire(e), true;
  }
  setIconName(e) {
    return this._iconName = e, true;
  }
  setOrReportIndexedColor(e) {
    let i = [], r = e.split(";");
    for (;r.length > 1; ) {
      let n = r.shift(), o = r.shift();
      if (/^\d+$/.exec(n)) {
        let l = parseInt(n);
        if (Sl(l))
          if (o === "?")
            i.push({ type: 0, index: l });
          else {
            let a = Ws2(o);
            a && i.push({ type: 1, index: l, color: a });
          }
      }
    }
    return i.length && this._onColor.fire(i), true;
  }
  setHyperlink(e) {
    let i = e.indexOf(";");
    if (i === -1)
      return true;
    let r = e.slice(0, i).trim(), n = e.slice(i + 1);
    return n ? this._createHyperlink(r, n) : r.trim() ? false : this._finishHyperlink();
  }
  _createHyperlink(e, i) {
    this._getCurrentLinkId() && this._finishHyperlink();
    let r = e.split(":"), n, o = r.findIndex((l) => l.startsWith("id="));
    return o !== -1 && (n = r[o].slice(3) || undefined), this._curAttrData.extended = this._curAttrData.extended.clone(), this._curAttrData.extended.urlId = this._oscLinkService.registerLink({ id: n, uri: i }), this._curAttrData.updateExtended(), true;
  }
  _finishHyperlink() {
    return this._curAttrData.extended = this._curAttrData.extended.clone(), this._curAttrData.extended.urlId = 0, this._curAttrData.updateExtended(), true;
  }
  _setOrReportSpecialColor(e, i) {
    let r = e.split(";");
    for (let n = 0;n < r.length && !(i >= this._specialColors.length); ++n, ++i)
      if (r[n] === "?")
        this._onColor.fire([{ type: 0, index: this._specialColors[i] }]);
      else {
        let o = Ws2(r[n]);
        o && this._onColor.fire([{ type: 1, index: this._specialColors[i], color: o }]);
      }
    return true;
  }
  setOrReportFgColor(e) {
    return this._setOrReportSpecialColor(e, 0);
  }
  setOrReportBgColor(e) {
    return this._setOrReportSpecialColor(e, 1);
  }
  setOrReportCursorColor(e) {
    return this._setOrReportSpecialColor(e, 2);
  }
  restoreIndexedColor(e) {
    if (!e)
      return this._onColor.fire([{ type: 2 }]), true;
    let i = [], r = e.split(";");
    for (let n = 0;n < r.length; ++n)
      if (/^\d+$/.exec(r[n])) {
        let o = parseInt(r[n]);
        Sl(o) && i.push({ type: 2, index: o });
      }
    return i.length && this._onColor.fire(i), true;
  }
  restoreFgColor(e) {
    return this._onColor.fire([{ type: 2, index: 256 }]), true;
  }
  restoreBgColor(e) {
    return this._onColor.fire([{ type: 2, index: 257 }]), true;
  }
  restoreCursorColor(e) {
    return this._onColor.fire([{ type: 2, index: 258 }]), true;
  }
  nextLine() {
    return this._activeBuffer.x = 0, this.index(), true;
  }
  keypadApplicationMode() {
    return this._logService.debug("Serial port requested application keypad."), this._coreService.decPrivateModes.applicationKeypad = true, this._onRequestSyncScrollBar.fire(), true;
  }
  keypadNumericMode() {
    return this._logService.debug("Switching back to normal keypad."), this._coreService.decPrivateModes.applicationKeypad = false, this._onRequestSyncScrollBar.fire(), true;
  }
  selectDefaultCharset() {
    return this._charsetService.setgLevel(0), this._charsetService.setgCharset(0, Je), true;
  }
  selectCharset(e) {
    return e.length !== 2 ? (this.selectDefaultCharset(), true) : (e[0] === "/" || this._charsetService.setgCharset(mc[e[0]], ne[e[1]] || Je), true);
  }
  index() {
    return this._restrictCursor(), this._activeBuffer.y++, this._activeBuffer.y === this._activeBuffer.scrollBottom + 1 ? (this._activeBuffer.y--, this._bufferService.scroll(this._eraseAttrData())) : this._activeBuffer.y >= this._bufferService.rows && (this._activeBuffer.y = this._bufferService.rows - 1), this._restrictCursor(), true;
  }
  tabSet() {
    return this._activeBuffer.tabs[this._activeBuffer.x] = true, true;
  }
  reverseIndex() {
    if (this._restrictCursor(), this._activeBuffer.y === this._activeBuffer.scrollTop) {
      let e = this._activeBuffer.scrollBottom - this._activeBuffer.scrollTop;
      this._activeBuffer.lines.shiftElements(this._activeBuffer.ybase + this._activeBuffer.y, e, 1), this._activeBuffer.lines.set(this._activeBuffer.ybase + this._activeBuffer.y, this._activeBuffer.getBlankLine(this._eraseAttrData())), this._dirtyRowTracker.markRangeDirty(this._activeBuffer.scrollTop, this._activeBuffer.scrollBottom);
    } else
      this._activeBuffer.y--, this._restrictCursor();
    return true;
  }
  fullReset() {
    return this._parser.reset(), this._onRequestReset.fire(), true;
  }
  reset() {
    this._curAttrData = X2.clone(), this._eraseAttrDataInternal = X2.clone();
  }
  _eraseAttrData() {
    return this._eraseAttrDataInternal.bg &= -67108864, this._eraseAttrDataInternal.bg |= this._curAttrData.bg & 67108863, this._eraseAttrDataInternal;
  }
  setgLevel(e) {
    return this._charsetService.setgLevel(e), true;
  }
  screenAlignmentPattern() {
    let e = new q;
    e.content = 1 << 22 | 69, e.fg = this._curAttrData.fg, e.bg = this._curAttrData.bg, this._setCursor(0, 0);
    for (let i = 0;i < this._bufferService.rows; ++i) {
      let r = this._activeBuffer.ybase + this._activeBuffer.y + i, n = this._activeBuffer.lines.get(r);
      n && (n.fill(e), n.isWrapped = false);
    }
    return this._dirtyRowTracker.markAllDirty(), this._setCursor(0, 0), true;
  }
  requestStatusString(e, i) {
    let r = (a) => (this._coreService.triggerDataEvent(`${b.ESC}${a}${b.ESC}\\`), true), n = this._bufferService.buffer, o = this._optionsService.rawOptions, l = { block: 2, underline: 4, bar: 6 };
    return r(e === '"q' ? `P1$r${this._curAttrData.isProtected() ? 1 : 0}"q` : e === '"p' ? 'P1$r61;1"p' : e === "r" ? `P1$r${n.scrollTop + 1};${n.scrollBottom + 1}r` : e === "m" ? "P1$r0m" : e === " q" ? `P1$r${l[o.cursorStyle] - (o.cursorBlink ? 1 : 0)} q` : "P0$r");
  }
  markRangeDirty(e, i) {
    this._dirtyRowTracker.markRangeDirty(e, i);
  }
};
var Zi = class {
  constructor(t) {
    this._bufferService = t;
    this.clearRange();
  }
  clearRange() {
    this.start = this._bufferService.buffer.y, this.end = this._bufferService.buffer.y;
  }
  markDirty(t) {
    t < this.start ? this.start = t : t > this.end && (this.end = t);
  }
  markRangeDirty(t, e) {
    t > e && (gl = t, t = e, e = gl), t < this.start && (this.start = t), e > this.end && (this.end = e);
  }
  markAllDirty() {
    this.markRangeDirty(0, this._bufferService.rows - 1);
  }
};
Zi = M([S(0, F2)], Zi);
function Sl(s) {
  return 0 <= s && s < 256;
}
var _c = 50000000;
var El = 12;
var bc = 50;
var gn2 = class extends D2 {
  constructor(e) {
    super();
    this._action = e;
    this._writeBuffer = [];
    this._callbacks = [];
    this._pendingData = 0;
    this._bufferOffset = 0;
    this._isSyncWriting = false;
    this._syncCalls = 0;
    this._didUserInput = false;
    this._onWriteParsed = this._register(new v);
    this.onWriteParsed = this._onWriteParsed.event;
  }
  handleUserInput() {
    this._didUserInput = true;
  }
  writeSync(e, i) {
    if (i !== undefined && this._syncCalls > i) {
      this._syncCalls = 0;
      return;
    }
    if (this._pendingData += e.length, this._writeBuffer.push(e), this._callbacks.push(undefined), this._syncCalls++, this._isSyncWriting)
      return;
    this._isSyncWriting = true;
    let r;
    for (;r = this._writeBuffer.shift(); ) {
      this._action(r);
      let n = this._callbacks.shift();
      n && n();
    }
    this._pendingData = 0, this._bufferOffset = 2147483647, this._isSyncWriting = false, this._syncCalls = 0;
  }
  write(e, i) {
    if (this._pendingData > _c)
      throw new Error("write data discarded, use flow control to avoid losing data");
    if (!this._writeBuffer.length) {
      if (this._bufferOffset = 0, this._didUserInput) {
        this._didUserInput = false, this._pendingData += e.length, this._writeBuffer.push(e), this._callbacks.push(i), this._innerWrite();
        return;
      }
      setTimeout(() => this._innerWrite());
    }
    this._pendingData += e.length, this._writeBuffer.push(e), this._callbacks.push(i);
  }
  _innerWrite(e = 0, i = true) {
    let r = e || performance.now();
    for (;this._writeBuffer.length > this._bufferOffset; ) {
      let n = this._writeBuffer[this._bufferOffset], o = this._action(n, i);
      if (o) {
        let a = (u) => performance.now() - r >= El ? setTimeout(() => this._innerWrite(0, u)) : this._innerWrite(r, u);
        o.catch((u) => (queueMicrotask(() => {
          throw u;
        }), Promise.resolve(false))).then(a);
        return;
      }
      let l = this._callbacks[this._bufferOffset];
      if (l && l(), this._bufferOffset++, this._pendingData -= n.length, performance.now() - r >= El)
        break;
    }
    this._writeBuffer.length > this._bufferOffset ? (this._bufferOffset > bc && (this._writeBuffer = this._writeBuffer.slice(this._bufferOffset), this._callbacks = this._callbacks.slice(this._bufferOffset), this._bufferOffset = 0), setTimeout(() => this._innerWrite())) : (this._writeBuffer.length = 0, this._callbacks.length = 0, this._pendingData = 0, this._bufferOffset = 0), this._onWriteParsed.fire();
  }
};
var ui2 = class {
  constructor(t) {
    this._bufferService = t;
    this._nextId = 1;
    this._entriesWithId = new Map;
    this._dataByLinkId = new Map;
  }
  registerLink(t) {
    let e = this._bufferService.buffer;
    if (t.id === undefined) {
      let a = e.addMarker(e.ybase + e.y), u = { data: t, id: this._nextId++, lines: [a] };
      return a.onDispose(() => this._removeMarkerFromLink(u, a)), this._dataByLinkId.set(u.id, u), u.id;
    }
    let i = t, r = this._getEntryIdKey(i), n = this._entriesWithId.get(r);
    if (n)
      return this.addLineToLink(n.id, e.ybase + e.y), n.id;
    let o = e.addMarker(e.ybase + e.y), l = { id: this._nextId++, key: this._getEntryIdKey(i), data: i, lines: [o] };
    return o.onDispose(() => this._removeMarkerFromLink(l, o)), this._entriesWithId.set(l.key, l), this._dataByLinkId.set(l.id, l), l.id;
  }
  addLineToLink(t, e) {
    let i = this._dataByLinkId.get(t);
    if (i && i.lines.every((r) => r.line !== e)) {
      let r = this._bufferService.buffer.addMarker(e);
      i.lines.push(r), r.onDispose(() => this._removeMarkerFromLink(i, r));
    }
  }
  getLinkData(t) {
    return this._dataByLinkId.get(t)?.data;
  }
  _getEntryIdKey(t) {
    return `${t.id};;${t.uri}`;
  }
  _removeMarkerFromLink(t, e) {
    let i = t.lines.indexOf(e);
    i !== -1 && (t.lines.splice(i, 1), t.lines.length === 0 && (t.data.id !== undefined && this._entriesWithId.delete(t.key), this._dataByLinkId.delete(t.id)));
  }
};
ui2 = M([S(0, F2)], ui2);
var Tl = false;
var Sn2 = class extends D2 {
  constructor(e) {
    super();
    this._windowsWrappingHeuristics = this._register(new ye2);
    this._onBinary = this._register(new v);
    this.onBinary = this._onBinary.event;
    this._onData = this._register(new v);
    this.onData = this._onData.event;
    this._onLineFeed = this._register(new v);
    this.onLineFeed = this._onLineFeed.event;
    this._onResize = this._register(new v);
    this.onResize = this._onResize.event;
    this._onWriteParsed = this._register(new v);
    this.onWriteParsed = this._onWriteParsed.event;
    this._onScroll = this._register(new v);
    this._instantiationService = new ln, this.optionsService = this._register(new dn2(e)), this._instantiationService.setService(H2, this.optionsService), this._bufferService = this._register(this._instantiationService.createInstance(ni2)), this._instantiationService.setService(F2, this._bufferService), this._logService = this._register(this._instantiationService.createInstance(ii2)), this._instantiationService.setService(nr2, this._logService), this.coreService = this._register(this._instantiationService.createInstance(li2)), this._instantiationService.setService(ge2, this.coreService), this.coreMouseService = this._register(this._instantiationService.createInstance(ai2)), this._instantiationService.setService(rr, this.coreMouseService), this.unicodeService = this._register(this._instantiationService.createInstance(Ae2)), this._instantiationService.setService(Js, this.unicodeService), this._charsetService = this._instantiationService.createInstance(pn2), this._instantiationService.setService(Zs, this._charsetService), this._oscLinkService = this._instantiationService.createInstance(ui2), this._instantiationService.setService(sr2, this._oscLinkService), this._inputHandler = this._register(new vn(this._bufferService, this._charsetService, this.coreService, this._logService, this.optionsService, this._oscLinkService, this.coreMouseService, this.unicodeService)), this._register($.forward(this._inputHandler.onLineFeed, this._onLineFeed)), this._register(this._inputHandler), this._register($.forward(this._bufferService.onResize, this._onResize)), this._register($.forward(this.coreService.onData, this._onData)), this._register($.forward(this.coreService.onBinary, this._onBinary)), this._register(this.coreService.onRequestScrollToBottom(() => this.scrollToBottom(true))), this._register(this.coreService.onUserInput(() => this._writeBuffer.handleUserInput())), this._register(this.optionsService.onMultipleOptionChange(["windowsMode", "windowsPty"], () => this._handleWindowsPtyOptionChange())), this._register(this._bufferService.onScroll(() => {
      this._onScroll.fire({ position: this._bufferService.buffer.ydisp }), this._inputHandler.markRangeDirty(this._bufferService.buffer.scrollTop, this._bufferService.buffer.scrollBottom);
    })), this._writeBuffer = this._register(new gn2((i, r) => this._inputHandler.parse(i, r))), this._register($.forward(this._writeBuffer.onWriteParsed, this._onWriteParsed));
  }
  get onScroll() {
    return this._onScrollApi || (this._onScrollApi = this._register(new v), this._onScroll.event((e) => {
      this._onScrollApi?.fire(e.position);
    })), this._onScrollApi.event;
  }
  get cols() {
    return this._bufferService.cols;
  }
  get rows() {
    return this._bufferService.rows;
  }
  get buffers() {
    return this._bufferService.buffers;
  }
  get options() {
    return this.optionsService.options;
  }
  set options(e) {
    for (let i in e)
      this.optionsService.options[i] = e[i];
  }
  write(e, i) {
    this._writeBuffer.write(e, i);
  }
  writeSync(e, i) {
    this._logService.logLevel <= 3 && !Tl && (this._logService.warn("writeSync is unreliable and will be removed soon."), Tl = true), this._writeBuffer.writeSync(e, i);
  }
  input(e, i = true) {
    this.coreService.triggerDataEvent(e, i);
  }
  resize(e, i) {
    isNaN(e) || isNaN(i) || (e = Math.max(e, ks2), i = Math.max(i, Cs), this._bufferService.resize(e, i));
  }
  scroll(e, i = false) {
    this._bufferService.scroll(e, i);
  }
  scrollLines(e, i) {
    this._bufferService.scrollLines(e, i);
  }
  scrollPages(e) {
    this.scrollLines(e * (this.rows - 1));
  }
  scrollToTop() {
    this.scrollLines(-this._bufferService.buffer.ydisp);
  }
  scrollToBottom(e) {
    this.scrollLines(this._bufferService.buffer.ybase - this._bufferService.buffer.ydisp);
  }
  scrollToLine(e) {
    let i = e - this._bufferService.buffer.ydisp;
    i !== 0 && this.scrollLines(i);
  }
  registerEscHandler(e, i) {
    return this._inputHandler.registerEscHandler(e, i);
  }
  registerDcsHandler(e, i) {
    return this._inputHandler.registerDcsHandler(e, i);
  }
  registerCsiHandler(e, i) {
    return this._inputHandler.registerCsiHandler(e, i);
  }
  registerOscHandler(e, i) {
    return this._inputHandler.registerOscHandler(e, i);
  }
  _setup() {
    this._handleWindowsPtyOptionChange();
  }
  reset() {
    this._inputHandler.reset(), this._bufferService.reset(), this._charsetService.reset(), this.coreService.reset(), this.coreMouseService.reset();
  }
  _handleWindowsPtyOptionChange() {
    let e = false, i = this.optionsService.rawOptions.windowsPty;
    i && i.buildNumber !== undefined && i.buildNumber !== undefined ? e = i.backend === "conpty" && i.buildNumber < 21376 : this.optionsService.rawOptions.windowsMode && (e = true), e ? this._enableWindowsWrappingHeuristics() : this._windowsWrappingHeuristics.clear();
  }
  _enableWindowsWrappingHeuristics() {
    if (!this._windowsWrappingHeuristics.value) {
      let e = [];
      e.push(this.onLineFeed(Bs2.bind(null, this._bufferService))), e.push(this.registerCsiHandler({ final: "H" }, () => (Bs2(this._bufferService), false))), this._windowsWrappingHeuristics.value = C2(() => {
        for (let i of e)
          i.dispose();
      });
    }
  }
};
var gc = { 48: ["0", ")"], 49: ["1", "!"], 50: ["2", "@"], 51: ["3", "#"], 52: ["4", "$"], 53: ["5", "%"], 54: ["6", "^"], 55: ["7", "&"], 56: ["8", "*"], 57: ["9", "("], 186: [";", ":"], 187: ["=", "+"], 188: [",", "<"], 189: ["-", "_"], 190: [".", ">"], 191: ["/", "?"], 192: ["`", "~"], 219: ["[", "{"], 220: ["\\", "|"], 221: ["]", "}"], 222: ["'", '"'] };
function Il(s, t, e, i) {
  let r = { type: 0, cancel: false, key: undefined }, n = (s.shiftKey ? 1 : 0) | (s.altKey ? 2 : 0) | (s.ctrlKey ? 4 : 0) | (s.metaKey ? 8 : 0);
  switch (s.keyCode) {
    case 0:
      s.key === "UIKeyInputUpArrow" ? t ? r.key = b.ESC + "OA" : r.key = b.ESC + "[A" : s.key === "UIKeyInputLeftArrow" ? t ? r.key = b.ESC + "OD" : r.key = b.ESC + "[D" : s.key === "UIKeyInputRightArrow" ? t ? r.key = b.ESC + "OC" : r.key = b.ESC + "[C" : s.key === "UIKeyInputDownArrow" && (t ? r.key = b.ESC + "OB" : r.key = b.ESC + "[B");
      break;
    case 8:
      r.key = s.ctrlKey ? "\b" : b.DEL, s.altKey && (r.key = b.ESC + r.key);
      break;
    case 9:
      if (s.shiftKey) {
        r.key = b.ESC + "[Z";
        break;
      }
      r.key = b.HT, r.cancel = true;
      break;
    case 13:
      r.key = s.altKey ? b.ESC + b.CR : b.CR, r.cancel = true;
      break;
    case 27:
      r.key = b.ESC, s.altKey && (r.key = b.ESC + b.ESC), r.cancel = true;
      break;
    case 37:
      if (s.metaKey)
        break;
      n ? r.key = b.ESC + "[1;" + (n + 1) + "D" : t ? r.key = b.ESC + "OD" : r.key = b.ESC + "[D";
      break;
    case 39:
      if (s.metaKey)
        break;
      n ? r.key = b.ESC + "[1;" + (n + 1) + "C" : t ? r.key = b.ESC + "OC" : r.key = b.ESC + "[C";
      break;
    case 38:
      if (s.metaKey)
        break;
      n ? r.key = b.ESC + "[1;" + (n + 1) + "A" : t ? r.key = b.ESC + "OA" : r.key = b.ESC + "[A";
      break;
    case 40:
      if (s.metaKey)
        break;
      n ? r.key = b.ESC + "[1;" + (n + 1) + "B" : t ? r.key = b.ESC + "OB" : r.key = b.ESC + "[B";
      break;
    case 45:
      !s.shiftKey && !s.ctrlKey && (r.key = b.ESC + "[2~");
      break;
    case 46:
      n ? r.key = b.ESC + "[3;" + (n + 1) + "~" : r.key = b.ESC + "[3~";
      break;
    case 36:
      n ? r.key = b.ESC + "[1;" + (n + 1) + "H" : t ? r.key = b.ESC + "OH" : r.key = b.ESC + "[H";
      break;
    case 35:
      n ? r.key = b.ESC + "[1;" + (n + 1) + "F" : t ? r.key = b.ESC + "OF" : r.key = b.ESC + "[F";
      break;
    case 33:
      s.shiftKey ? r.type = 2 : s.ctrlKey ? r.key = b.ESC + "[5;" + (n + 1) + "~" : r.key = b.ESC + "[5~";
      break;
    case 34:
      s.shiftKey ? r.type = 3 : s.ctrlKey ? r.key = b.ESC + "[6;" + (n + 1) + "~" : r.key = b.ESC + "[6~";
      break;
    case 112:
      n ? r.key = b.ESC + "[1;" + (n + 1) + "P" : r.key = b.ESC + "OP";
      break;
    case 113:
      n ? r.key = b.ESC + "[1;" + (n + 1) + "Q" : r.key = b.ESC + "OQ";
      break;
    case 114:
      n ? r.key = b.ESC + "[1;" + (n + 1) + "R" : r.key = b.ESC + "OR";
      break;
    case 115:
      n ? r.key = b.ESC + "[1;" + (n + 1) + "S" : r.key = b.ESC + "OS";
      break;
    case 116:
      n ? r.key = b.ESC + "[15;" + (n + 1) + "~" : r.key = b.ESC + "[15~";
      break;
    case 117:
      n ? r.key = b.ESC + "[17;" + (n + 1) + "~" : r.key = b.ESC + "[17~";
      break;
    case 118:
      n ? r.key = b.ESC + "[18;" + (n + 1) + "~" : r.key = b.ESC + "[18~";
      break;
    case 119:
      n ? r.key = b.ESC + "[19;" + (n + 1) + "~" : r.key = b.ESC + "[19~";
      break;
    case 120:
      n ? r.key = b.ESC + "[20;" + (n + 1) + "~" : r.key = b.ESC + "[20~";
      break;
    case 121:
      n ? r.key = b.ESC + "[21;" + (n + 1) + "~" : r.key = b.ESC + "[21~";
      break;
    case 122:
      n ? r.key = b.ESC + "[23;" + (n + 1) + "~" : r.key = b.ESC + "[23~";
      break;
    case 123:
      n ? r.key = b.ESC + "[24;" + (n + 1) + "~" : r.key = b.ESC + "[24~";
      break;
    default:
      if (s.ctrlKey && !s.shiftKey && !s.altKey && !s.metaKey)
        s.keyCode >= 65 && s.keyCode <= 90 ? r.key = String.fromCharCode(s.keyCode - 64) : s.keyCode === 32 ? r.key = b.NUL : s.keyCode >= 51 && s.keyCode <= 55 ? r.key = String.fromCharCode(s.keyCode - 51 + 27) : s.keyCode === 56 ? r.key = b.DEL : s.keyCode === 219 ? r.key = b.ESC : s.keyCode === 220 ? r.key = b.FS : s.keyCode === 221 && (r.key = b.GS);
      else if ((!e || i) && s.altKey && !s.metaKey) {
        let l = gc[s.keyCode]?.[s.shiftKey ? 1 : 0];
        if (l)
          r.key = b.ESC + l;
        else if (s.keyCode >= 65 && s.keyCode <= 90) {
          let a = s.ctrlKey ? s.keyCode - 64 : s.keyCode + 32, u = String.fromCharCode(a);
          s.shiftKey && (u = u.toUpperCase()), r.key = b.ESC + u;
        } else if (s.keyCode === 32)
          r.key = b.ESC + (s.ctrlKey ? b.NUL : " ");
        else if (s.key === "Dead" && s.code.startsWith("Key")) {
          let a = s.code.slice(3, 4);
          s.shiftKey || (a = a.toLowerCase()), r.key = b.ESC + a, r.cancel = true;
        }
      } else
        e && !s.altKey && !s.ctrlKey && !s.shiftKey && s.metaKey ? s.keyCode === 65 && (r.type = 1) : s.key && !s.ctrlKey && !s.altKey && !s.metaKey && s.keyCode >= 48 && s.key.length === 1 ? r.key = s.key : s.key && s.ctrlKey && (s.key === "_" && (r.key = b.US), s.key === "@" && (r.key = b.NUL));
      break;
  }
  return r;
}
var ee2 = 0;
var En2 = class {
  constructor(t) {
    this._getKey = t;
    this._array = [];
    this._insertedValues = [];
    this._flushInsertedTask = new Jt;
    this._isFlushingInserted = false;
    this._deletedIndices = [];
    this._flushDeletedTask = new Jt;
    this._isFlushingDeleted = false;
  }
  clear() {
    this._array.length = 0, this._insertedValues.length = 0, this._flushInsertedTask.clear(), this._isFlushingInserted = false, this._deletedIndices.length = 0, this._flushDeletedTask.clear(), this._isFlushingDeleted = false;
  }
  insert(t) {
    this._flushCleanupDeleted(), this._insertedValues.length === 0 && this._flushInsertedTask.enqueue(() => this._flushInserted()), this._insertedValues.push(t);
  }
  _flushInserted() {
    let t = this._insertedValues.sort((n, o) => this._getKey(n) - this._getKey(o)), e = 0, i = 0, r = new Array(this._array.length + this._insertedValues.length);
    for (let n = 0;n < r.length; n++)
      i >= this._array.length || this._getKey(t[e]) <= this._getKey(this._array[i]) ? (r[n] = t[e], e++) : r[n] = this._array[i++];
    this._array = r, this._insertedValues.length = 0;
  }
  _flushCleanupInserted() {
    !this._isFlushingInserted && this._insertedValues.length > 0 && this._flushInsertedTask.flush();
  }
  delete(t) {
    if (this._flushCleanupInserted(), this._array.length === 0)
      return false;
    let e = this._getKey(t);
    if (e === undefined || (ee2 = this._search(e), ee2 === -1) || this._getKey(this._array[ee2]) !== e)
      return false;
    do
      if (this._array[ee2] === t)
        return this._deletedIndices.length === 0 && this._flushDeletedTask.enqueue(() => this._flushDeleted()), this._deletedIndices.push(ee2), true;
    while (++ee2 < this._array.length && this._getKey(this._array[ee2]) === e);
    return false;
  }
  _flushDeleted() {
    this._isFlushingDeleted = true;
    let t = this._deletedIndices.sort((n, o) => n - o), e = 0, i = new Array(this._array.length - t.length), r = 0;
    for (let n = 0;n < this._array.length; n++)
      t[e] === n ? e++ : i[r++] = this._array[n];
    this._array = i, this._deletedIndices.length = 0, this._isFlushingDeleted = false;
  }
  _flushCleanupDeleted() {
    !this._isFlushingDeleted && this._deletedIndices.length > 0 && this._flushDeletedTask.flush();
  }
  *getKeyIterator(t) {
    if (this._flushCleanupInserted(), this._flushCleanupDeleted(), this._array.length !== 0 && (ee2 = this._search(t), !(ee2 < 0 || ee2 >= this._array.length) && this._getKey(this._array[ee2]) === t))
      do
        yield this._array[ee2];
      while (++ee2 < this._array.length && this._getKey(this._array[ee2]) === t);
  }
  forEachByKey(t, e) {
    if (this._flushCleanupInserted(), this._flushCleanupDeleted(), this._array.length !== 0 && (ee2 = this._search(t), !(ee2 < 0 || ee2 >= this._array.length) && this._getKey(this._array[ee2]) === t))
      do
        e(this._array[ee2]);
      while (++ee2 < this._array.length && this._getKey(this._array[ee2]) === t);
  }
  values() {
    return this._flushCleanupInserted(), this._flushCleanupDeleted(), [...this._array].values();
  }
  _search(t) {
    let e = 0, i = this._array.length - 1;
    for (;i >= e; ) {
      let r = e + i >> 1, n = this._getKey(this._array[r]);
      if (n > t)
        i = r - 1;
      else if (n < t)
        e = r + 1;
      else {
        for (;r > 0 && this._getKey(this._array[r - 1]) === t; )
          r--;
        return r;
      }
    }
    return e;
  }
};
var Us2 = 0;
var yl = 0;
var Tn = class extends D2 {
  constructor() {
    super();
    this._decorations = new En2((e) => e?.marker.line);
    this._onDecorationRegistered = this._register(new v);
    this.onDecorationRegistered = this._onDecorationRegistered.event;
    this._onDecorationRemoved = this._register(new v);
    this.onDecorationRemoved = this._onDecorationRemoved.event;
    this._register(C2(() => this.reset()));
  }
  get decorations() {
    return this._decorations.values();
  }
  registerDecoration(e) {
    if (e.marker.isDisposed)
      return;
    let i = new Ks(e);
    if (i) {
      let r = i.marker.onDispose(() => i.dispose()), n = i.onDispose(() => {
        n.dispose(), i && (this._decorations.delete(i) && this._onDecorationRemoved.fire(i), r.dispose());
      });
      this._decorations.insert(i), this._onDecorationRegistered.fire(i);
    }
    return i;
  }
  reset() {
    for (let e of this._decorations.values())
      e.dispose();
    this._decorations.clear();
  }
  *getDecorationsAtCell(e, i, r) {
    let n = 0, o = 0;
    for (let l of this._decorations.getKeyIterator(i))
      n = l.options.x ?? 0, o = n + (l.options.width ?? 1), e >= n && e < o && (!r || (l.options.layer ?? "bottom") === r) && (yield l);
  }
  forEachDecorationAtCell(e, i, r, n) {
    this._decorations.forEachByKey(i, (o) => {
      Us2 = o.options.x ?? 0, yl = Us2 + (o.options.width ?? 1), e >= Us2 && e < yl && (!r || (o.options.layer ?? "bottom") === r) && n(o);
    });
  }
};
var Ks = class extends Ee2 {
  constructor(e) {
    super();
    this.options = e;
    this.onRenderEmitter = this.add(new v);
    this.onRender = this.onRenderEmitter.event;
    this._onDispose = this.add(new v);
    this.onDispose = this._onDispose.event;
    this._cachedBg = null;
    this._cachedFg = null;
    this.marker = e.marker, this.options.overviewRulerOptions && !this.options.overviewRulerOptions.position && (this.options.overviewRulerOptions.position = "full");
  }
  get backgroundColorRGB() {
    return this._cachedBg === null && (this.options.backgroundColor ? this._cachedBg = z2.toColor(this.options.backgroundColor) : this._cachedBg = undefined), this._cachedBg;
  }
  get foregroundColorRGB() {
    return this._cachedFg === null && (this.options.foregroundColor ? this._cachedFg = z2.toColor(this.options.foregroundColor) : this._cachedFg = undefined), this._cachedFg;
  }
  dispose() {
    this._onDispose.fire(), super.dispose();
  }
};
var Sc = 1000;
var In2 = class {
  constructor(t, e = Sc) {
    this._renderCallback = t;
    this._debounceThresholdMS = e;
    this._lastRefreshMs = 0;
    this._additionalRefreshRequested = false;
  }
  dispose() {
    this._refreshTimeoutID && clearTimeout(this._refreshTimeoutID);
  }
  refresh(t, e, i) {
    this._rowCount = i, t = t !== undefined ? t : 0, e = e !== undefined ? e : this._rowCount - 1, this._rowStart = this._rowStart !== undefined ? Math.min(this._rowStart, t) : t, this._rowEnd = this._rowEnd !== undefined ? Math.max(this._rowEnd, e) : e;
    let r = performance.now();
    if (r - this._lastRefreshMs >= this._debounceThresholdMS)
      this._lastRefreshMs = r, this._innerRefresh();
    else if (!this._additionalRefreshRequested) {
      let n = r - this._lastRefreshMs, o = this._debounceThresholdMS - n;
      this._additionalRefreshRequested = true, this._refreshTimeoutID = window.setTimeout(() => {
        this._lastRefreshMs = performance.now(), this._innerRefresh(), this._additionalRefreshRequested = false, this._refreshTimeoutID = undefined;
      }, o);
    }
  }
  _innerRefresh() {
    if (this._rowStart === undefined || this._rowEnd === undefined || this._rowCount === undefined)
      return;
    let t = Math.max(this._rowStart, 0), e = Math.min(this._rowEnd, this._rowCount - 1);
    this._rowStart = undefined, this._rowEnd = undefined, this._renderCallback(t, e);
  }
};
var xl = 20;
var wl = false;
var Tt2 = class extends D2 {
  constructor(e, i, r, n) {
    super();
    this._terminal = e;
    this._coreBrowserService = r;
    this._renderService = n;
    this._rowColumns = new WeakMap;
    this._liveRegionLineCount = 0;
    this._charsToConsume = [];
    this._charsToAnnounce = "";
    let o = this._coreBrowserService.mainDocument;
    this._accessibilityContainer = o.createElement("div"), this._accessibilityContainer.classList.add("xterm-accessibility"), this._rowContainer = o.createElement("div"), this._rowContainer.setAttribute("role", "list"), this._rowContainer.classList.add("xterm-accessibility-tree"), this._rowElements = [];
    for (let l = 0;l < this._terminal.rows; l++)
      this._rowElements[l] = this._createAccessibilityTreeNode(), this._rowContainer.appendChild(this._rowElements[l]);
    if (this._topBoundaryFocusListener = (l) => this._handleBoundaryFocus(l, 0), this._bottomBoundaryFocusListener = (l) => this._handleBoundaryFocus(l, 1), this._rowElements[0].addEventListener("focus", this._topBoundaryFocusListener), this._rowElements[this._rowElements.length - 1].addEventListener("focus", this._bottomBoundaryFocusListener), this._accessibilityContainer.appendChild(this._rowContainer), this._liveRegion = o.createElement("div"), this._liveRegion.classList.add("live-region"), this._liveRegion.setAttribute("aria-live", "assertive"), this._accessibilityContainer.appendChild(this._liveRegion), this._liveRegionDebouncer = this._register(new In2(this._renderRows.bind(this))), !this._terminal.element)
      throw new Error("Cannot enable accessibility before Terminal.open");
    wl ? (this._accessibilityContainer.classList.add("debug"), this._rowContainer.classList.add("debug"), this._debugRootContainer = o.createElement("div"), this._debugRootContainer.classList.add("xterm"), this._debugRootContainer.appendChild(o.createTextNode("------start a11y------")), this._debugRootContainer.appendChild(this._accessibilityContainer), this._debugRootContainer.appendChild(o.createTextNode("------end a11y------")), this._terminal.element.insertAdjacentElement("afterend", this._debugRootContainer)) : this._terminal.element.insertAdjacentElement("afterbegin", this._accessibilityContainer), this._register(this._terminal.onResize((l) => this._handleResize(l.rows))), this._register(this._terminal.onRender((l) => this._refreshRows(l.start, l.end))), this._register(this._terminal.onScroll(() => this._refreshRows())), this._register(this._terminal.onA11yChar((l) => this._handleChar(l))), this._register(this._terminal.onLineFeed(() => this._handleChar(`
`))), this._register(this._terminal.onA11yTab((l) => this._handleTab(l))), this._register(this._terminal.onKey((l) => this._handleKey(l.key))), this._register(this._terminal.onBlur(() => this._clearLiveRegion())), this._register(this._renderService.onDimensionsChange(() => this._refreshRowsDimensions())), this._register(L(o, "selectionchange", () => this._handleSelectionChange())), this._register(this._coreBrowserService.onDprChange(() => this._refreshRowsDimensions())), this._refreshRowsDimensions(), this._refreshRows(), this._register(C2(() => {
      wl ? this._debugRootContainer.remove() : this._accessibilityContainer.remove(), this._rowElements.length = 0;
    }));
  }
  _handleTab(e) {
    for (let i = 0;i < e; i++)
      this._handleChar(" ");
  }
  _handleChar(e) {
    this._liveRegionLineCount < xl + 1 && (this._charsToConsume.length > 0 ? this._charsToConsume.shift() !== e && (this._charsToAnnounce += e) : this._charsToAnnounce += e, e === `
` && (this._liveRegionLineCount++, this._liveRegionLineCount === xl + 1 && (this._liveRegion.textContent += _i2.get())));
  }
  _clearLiveRegion() {
    this._liveRegion.textContent = "", this._liveRegionLineCount = 0;
  }
  _handleKey(e) {
    this._clearLiveRegion(), /\p{Control}/u.test(e) || this._charsToConsume.push(e);
  }
  _refreshRows(e, i) {
    this._liveRegionDebouncer.refresh(e, i, this._terminal.rows);
  }
  _renderRows(e, i) {
    let r = this._terminal.buffer, n = r.lines.length.toString();
    for (let o = e;o <= i; o++) {
      let l = r.lines.get(r.ydisp + o), a = [], u = l?.translateToString(true, undefined, undefined, a) || "", h = (r.ydisp + o + 1).toString(), c = this._rowElements[o];
      c && (u.length === 0 ? (c.textContent = " ", this._rowColumns.set(c, [0, 1])) : (c.textContent = u, this._rowColumns.set(c, a)), c.setAttribute("aria-posinset", h), c.setAttribute("aria-setsize", n), this._alignRowWidth(c));
    }
    this._announceCharacters();
  }
  _announceCharacters() {
    this._charsToAnnounce.length !== 0 && (this._liveRegion.textContent += this._charsToAnnounce, this._charsToAnnounce = "");
  }
  _handleBoundaryFocus(e, i) {
    let r = e.target, n = this._rowElements[i === 0 ? 1 : this._rowElements.length - 2], o = r.getAttribute("aria-posinset"), l = i === 0 ? "1" : `${this._terminal.buffer.lines.length}`;
    if (o === l || e.relatedTarget !== n)
      return;
    let a, u;
    if (i === 0 ? (a = r, u = this._rowElements.pop(), this._rowContainer.removeChild(u)) : (a = this._rowElements.shift(), u = r, this._rowContainer.removeChild(a)), a.removeEventListener("focus", this._topBoundaryFocusListener), u.removeEventListener("focus", this._bottomBoundaryFocusListener), i === 0) {
      let h = this._createAccessibilityTreeNode();
      this._rowElements.unshift(h), this._rowContainer.insertAdjacentElement("afterbegin", h);
    } else {
      let h = this._createAccessibilityTreeNode();
      this._rowElements.push(h), this._rowContainer.appendChild(h);
    }
    this._rowElements[0].addEventListener("focus", this._topBoundaryFocusListener), this._rowElements[this._rowElements.length - 1].addEventListener("focus", this._bottomBoundaryFocusListener), this._terminal.scrollLines(i === 0 ? -1 : 1), this._rowElements[i === 0 ? 1 : this._rowElements.length - 2].focus(), e.preventDefault(), e.stopImmediatePropagation();
  }
  _handleSelectionChange() {
    if (this._rowElements.length === 0)
      return;
    let e = this._coreBrowserService.mainDocument.getSelection();
    if (!e)
      return;
    if (e.isCollapsed) {
      this._rowContainer.contains(e.anchorNode) && this._terminal.clearSelection();
      return;
    }
    if (!e.anchorNode || !e.focusNode) {
      console.error("anchorNode and/or focusNode are null");
      return;
    }
    let i = { node: e.anchorNode, offset: e.anchorOffset }, r = { node: e.focusNode, offset: e.focusOffset };
    if ((i.node.compareDocumentPosition(r.node) & Node.DOCUMENT_POSITION_PRECEDING || i.node === r.node && i.offset > r.offset) && ([i, r] = [r, i]), i.node.compareDocumentPosition(this._rowElements[0]) & (Node.DOCUMENT_POSITION_CONTAINED_BY | Node.DOCUMENT_POSITION_FOLLOWING) && (i = { node: this._rowElements[0].childNodes[0], offset: 0 }), !this._rowContainer.contains(i.node))
      return;
    let n = this._rowElements.slice(-1)[0];
    if (r.node.compareDocumentPosition(n) & (Node.DOCUMENT_POSITION_CONTAINED_BY | Node.DOCUMENT_POSITION_PRECEDING) && (r = { node: n, offset: n.textContent?.length ?? 0 }), !this._rowContainer.contains(r.node))
      return;
    let o = ({ node: u, offset: h }) => {
      let c = u instanceof Text ? u.parentNode : u, d = parseInt(c?.getAttribute("aria-posinset"), 10) - 1;
      if (isNaN(d))
        return console.warn("row is invalid. Race condition?"), null;
      let _ = this._rowColumns.get(c);
      if (!_)
        return console.warn("columns is null. Race condition?"), null;
      let p = h < _.length ? _[h] : _.slice(-1)[0] + 1;
      return p >= this._terminal.cols && (++d, p = 0), { row: d, column: p };
    }, l = o(i), a = o(r);
    if (!(!l || !a)) {
      if (l.row > a.row || l.row === a.row && l.column >= a.column)
        throw new Error("invalid range");
      this._terminal.select(l.column, l.row, (a.row - l.row) * this._terminal.cols - l.column + a.column);
    }
  }
  _handleResize(e) {
    this._rowElements[this._rowElements.length - 1].removeEventListener("focus", this._bottomBoundaryFocusListener);
    for (let i = this._rowContainer.children.length;i < this._terminal.rows; i++)
      this._rowElements[i] = this._createAccessibilityTreeNode(), this._rowContainer.appendChild(this._rowElements[i]);
    for (;this._rowElements.length > e; )
      this._rowContainer.removeChild(this._rowElements.pop());
    this._rowElements[this._rowElements.length - 1].addEventListener("focus", this._bottomBoundaryFocusListener), this._refreshRowsDimensions();
  }
  _createAccessibilityTreeNode() {
    let e = this._coreBrowserService.mainDocument.createElement("div");
    return e.setAttribute("role", "listitem"), e.tabIndex = -1, this._refreshRowDimensions(e), e;
  }
  _refreshRowsDimensions() {
    if (this._renderService.dimensions.css.cell.height) {
      Object.assign(this._accessibilityContainer.style, { width: `${this._renderService.dimensions.css.canvas.width}px`, fontSize: `${this._terminal.options.fontSize}px` }), this._rowElements.length !== this._terminal.rows && this._handleResize(this._terminal.rows);
      for (let e = 0;e < this._terminal.rows; e++)
        this._refreshRowDimensions(this._rowElements[e]), this._alignRowWidth(this._rowElements[e]);
    }
  }
  _refreshRowDimensions(e) {
    e.style.height = `${this._renderService.dimensions.css.cell.height}px`;
  }
  _alignRowWidth(e) {
    e.style.transform = "";
    let i = e.getBoundingClientRect().width, r = this._rowColumns.get(e)?.slice(-1)?.[0];
    if (!r)
      return;
    let n = r * this._renderService.dimensions.css.cell.width;
    e.style.transform = `scaleX(${n / i})`;
  }
};
Tt2 = M([S(1, xt2), S(2, ae2), S(3, ce)], Tt2);
var hi = class extends D2 {
  constructor(e, i, r, n, o) {
    super();
    this._element = e;
    this._mouseService = i;
    this._renderService = r;
    this._bufferService = n;
    this._linkProviderService = o;
    this._linkCacheDisposables = [];
    this._isMouseOut = true;
    this._wasResized = false;
    this._activeLine = -1;
    this._onShowLinkUnderline = this._register(new v);
    this.onShowLinkUnderline = this._onShowLinkUnderline.event;
    this._onHideLinkUnderline = this._register(new v);
    this.onHideLinkUnderline = this._onHideLinkUnderline.event;
    this._register(C2(() => {
      Ne2(this._linkCacheDisposables), this._linkCacheDisposables.length = 0, this._lastMouseEvent = undefined, this._activeProviderReplies?.clear();
    })), this._register(this._bufferService.onResize(() => {
      this._clearCurrentLink(), this._wasResized = true;
    })), this._register(L(this._element, "mouseleave", () => {
      this._isMouseOut = true, this._clearCurrentLink();
    })), this._register(L(this._element, "mousemove", this._handleMouseMove.bind(this))), this._register(L(this._element, "mousedown", this._handleMouseDown.bind(this))), this._register(L(this._element, "mouseup", this._handleMouseUp.bind(this)));
  }
  get currentLink() {
    return this._currentLink;
  }
  _handleMouseMove(e) {
    this._lastMouseEvent = e;
    let i = this._positionFromMouseEvent(e, this._element, this._mouseService);
    if (!i)
      return;
    this._isMouseOut = false;
    let r = e.composedPath();
    for (let n = 0;n < r.length; n++) {
      let o = r[n];
      if (o.classList.contains("xterm"))
        break;
      if (o.classList.contains("xterm-hover"))
        return;
    }
    (!this._lastBufferCell || i.x !== this._lastBufferCell.x || i.y !== this._lastBufferCell.y) && (this._handleHover(i), this._lastBufferCell = i);
  }
  _handleHover(e) {
    if (this._activeLine !== e.y || this._wasResized) {
      this._clearCurrentLink(), this._askForLink(e, false), this._wasResized = false;
      return;
    }
    this._currentLink && this._linkAtPosition(this._currentLink.link, e) || (this._clearCurrentLink(), this._askForLink(e, true));
  }
  _askForLink(e, i) {
    (!this._activeProviderReplies || !i) && (this._activeProviderReplies?.forEach((n) => {
      n?.forEach((o) => {
        o.link.dispose && o.link.dispose();
      });
    }), this._activeProviderReplies = new Map, this._activeLine = e.y);
    let r = false;
    for (let [n, o] of this._linkProviderService.linkProviders.entries())
      i ? this._activeProviderReplies?.get(n) && (r = this._checkLinkProviderResult(n, e, r)) : o.provideLinks(e.y, (l) => {
        if (this._isMouseOut)
          return;
        let a = l?.map((u) => ({ link: u }));
        this._activeProviderReplies?.set(n, a), r = this._checkLinkProviderResult(n, e, r), this._activeProviderReplies?.size === this._linkProviderService.linkProviders.length && this._removeIntersectingLinks(e.y, this._activeProviderReplies);
      });
  }
  _removeIntersectingLinks(e, i) {
    let r = new Set;
    for (let n = 0;n < i.size; n++) {
      let o = i.get(n);
      if (o)
        for (let l = 0;l < o.length; l++) {
          let a = o[l], u = a.link.range.start.y < e ? 0 : a.link.range.start.x, h = a.link.range.end.y > e ? this._bufferService.cols : a.link.range.end.x;
          for (let c = u;c <= h; c++) {
            if (r.has(c)) {
              o.splice(l--, 1);
              break;
            }
            r.add(c);
          }
        }
    }
  }
  _checkLinkProviderResult(e, i, r) {
    if (!this._activeProviderReplies)
      return r;
    let n = this._activeProviderReplies.get(e), o = false;
    for (let l = 0;l < e; l++)
      (!this._activeProviderReplies.has(l) || this._activeProviderReplies.get(l)) && (o = true);
    if (!o && n) {
      let l = n.find((a) => this._linkAtPosition(a.link, i));
      l && (r = true, this._handleNewLink(l));
    }
    if (this._activeProviderReplies.size === this._linkProviderService.linkProviders.length && !r)
      for (let l = 0;l < this._activeProviderReplies.size; l++) {
        let a = this._activeProviderReplies.get(l)?.find((u) => this._linkAtPosition(u.link, i));
        if (a) {
          r = true, this._handleNewLink(a);
          break;
        }
      }
    return r;
  }
  _handleMouseDown() {
    this._mouseDownLink = this._currentLink;
  }
  _handleMouseUp(e) {
    if (!this._currentLink)
      return;
    let i = this._positionFromMouseEvent(e, this._element, this._mouseService);
    i && this._mouseDownLink && Ec(this._mouseDownLink.link, this._currentLink.link) && this._linkAtPosition(this._currentLink.link, i) && this._currentLink.link.activate(e, this._currentLink.link.text);
  }
  _clearCurrentLink(e, i) {
    !this._currentLink || !this._lastMouseEvent || (!e || !i || this._currentLink.link.range.start.y >= e && this._currentLink.link.range.end.y <= i) && (this._linkLeave(this._element, this._currentLink.link, this._lastMouseEvent), this._currentLink = undefined, Ne2(this._linkCacheDisposables), this._linkCacheDisposables.length = 0);
  }
  _handleNewLink(e) {
    if (!this._lastMouseEvent)
      return;
    let i = this._positionFromMouseEvent(this._lastMouseEvent, this._element, this._mouseService);
    i && this._linkAtPosition(e.link, i) && (this._currentLink = e, this._currentLink.state = { decorations: { underline: e.link.decorations === undefined ? true : e.link.decorations.underline, pointerCursor: e.link.decorations === undefined ? true : e.link.decorations.pointerCursor }, isHovered: true }, this._linkHover(this._element, e.link, this._lastMouseEvent), e.link.decorations = {}, Object.defineProperties(e.link.decorations, { pointerCursor: { get: () => this._currentLink?.state?.decorations.pointerCursor, set: (r) => {
      this._currentLink?.state && this._currentLink.state.decorations.pointerCursor !== r && (this._currentLink.state.decorations.pointerCursor = r, this._currentLink.state.isHovered && this._element.classList.toggle("xterm-cursor-pointer", r));
    } }, underline: { get: () => this._currentLink?.state?.decorations.underline, set: (r) => {
      this._currentLink?.state && this._currentLink?.state?.decorations.underline !== r && (this._currentLink.state.decorations.underline = r, this._currentLink.state.isHovered && this._fireUnderlineEvent(e.link, r));
    } } }), this._linkCacheDisposables.push(this._renderService.onRenderedViewportChange((r) => {
      if (!this._currentLink)
        return;
      let n = r.start === 0 ? 0 : r.start + 1 + this._bufferService.buffer.ydisp, o = this._bufferService.buffer.ydisp + 1 + r.end;
      if (this._currentLink.link.range.start.y >= n && this._currentLink.link.range.end.y <= o && (this._clearCurrentLink(n, o), this._lastMouseEvent)) {
        let l = this._positionFromMouseEvent(this._lastMouseEvent, this._element, this._mouseService);
        l && this._askForLink(l, false);
      }
    })));
  }
  _linkHover(e, i, r) {
    this._currentLink?.state && (this._currentLink.state.isHovered = true, this._currentLink.state.decorations.underline && this._fireUnderlineEvent(i, true), this._currentLink.state.decorations.pointerCursor && e.classList.add("xterm-cursor-pointer")), i.hover && i.hover(r, i.text);
  }
  _fireUnderlineEvent(e, i) {
    let r = e.range, n = this._bufferService.buffer.ydisp, o = this._createLinkUnderlineEvent(r.start.x - 1, r.start.y - n - 1, r.end.x, r.end.y - n - 1, undefined);
    (i ? this._onShowLinkUnderline : this._onHideLinkUnderline).fire(o);
  }
  _linkLeave(e, i, r) {
    this._currentLink?.state && (this._currentLink.state.isHovered = false, this._currentLink.state.decorations.underline && this._fireUnderlineEvent(i, false), this._currentLink.state.decorations.pointerCursor && e.classList.remove("xterm-cursor-pointer")), i.leave && i.leave(r, i.text);
  }
  _linkAtPosition(e, i) {
    let r = e.range.start.y * this._bufferService.cols + e.range.start.x, n = e.range.end.y * this._bufferService.cols + e.range.end.x, o = i.y * this._bufferService.cols + i.x;
    return r <= o && o <= n;
  }
  _positionFromMouseEvent(e, i, r) {
    let n = r.getCoords(e, i, this._bufferService.cols, this._bufferService.rows);
    if (n)
      return { x: n[0], y: n[1] + this._bufferService.buffer.ydisp };
  }
  _createLinkUnderlineEvent(e, i, r, n, o) {
    return { x1: e, y1: i, x2: r, y2: n, cols: this._bufferService.cols, fg: o };
  }
};
hi = M([S(1, Dt2), S(2, ce), S(3, F2), S(4, lr)], hi);
function Ec(s, t) {
  return s.text === t.text && s.range.start.x === t.range.start.x && s.range.start.y === t.range.start.y && s.range.end.x === t.range.end.x && s.range.end.y === t.range.end.y;
}
var yn2 = class extends Sn2 {
  constructor(e = {}) {
    super(e);
    this._linkifier = this._register(new ye2);
    this.browser = tn2;
    this._keyDownHandled = false;
    this._keyDownSeen = false;
    this._keyPressHandled = false;
    this._unprocessedDeadKey = false;
    this._accessibilityManager = this._register(new ye2);
    this._onCursorMove = this._register(new v);
    this.onCursorMove = this._onCursorMove.event;
    this._onKey = this._register(new v);
    this.onKey = this._onKey.event;
    this._onRender = this._register(new v);
    this.onRender = this._onRender.event;
    this._onSelectionChange = this._register(new v);
    this.onSelectionChange = this._onSelectionChange.event;
    this._onTitleChange = this._register(new v);
    this.onTitleChange = this._onTitleChange.event;
    this._onBell = this._register(new v);
    this.onBell = this._onBell.event;
    this._onFocus = this._register(new v);
    this._onBlur = this._register(new v);
    this._onA11yCharEmitter = this._register(new v);
    this._onA11yTabEmitter = this._register(new v);
    this._onWillOpen = this._register(new v);
    this._setup(), this._decorationService = this._instantiationService.createInstance(Tn), this._instantiationService.setService(Be2, this._decorationService), this._linkProviderService = this._instantiationService.createInstance(Qr2), this._instantiationService.setService(lr, this._linkProviderService), this._linkProviderService.registerLinkProvider(this._instantiationService.createInstance(wt2)), this._register(this._inputHandler.onRequestBell(() => this._onBell.fire())), this._register(this._inputHandler.onRequestRefreshRows((i) => this.refresh(i?.start ?? 0, i?.end ?? this.rows - 1))), this._register(this._inputHandler.onRequestSendFocus(() => this._reportFocus())), this._register(this._inputHandler.onRequestReset(() => this.reset())), this._register(this._inputHandler.onRequestWindowsOptionsReport((i) => this._reportWindowsOptions(i))), this._register(this._inputHandler.onColor((i) => this._handleColorEvent(i))), this._register($.forward(this._inputHandler.onCursorMove, this._onCursorMove)), this._register($.forward(this._inputHandler.onTitleChange, this._onTitleChange)), this._register($.forward(this._inputHandler.onA11yChar, this._onA11yCharEmitter)), this._register($.forward(this._inputHandler.onA11yTab, this._onA11yTabEmitter)), this._register(this._bufferService.onResize((i) => this._afterResize(i.cols, i.rows))), this._register(C2(() => {
      this._customKeyEventHandler = undefined, this.element?.parentNode?.removeChild(this.element);
    }));
  }
  get linkifier() {
    return this._linkifier.value;
  }
  get onFocus() {
    return this._onFocus.event;
  }
  get onBlur() {
    return this._onBlur.event;
  }
  get onA11yChar() {
    return this._onA11yCharEmitter.event;
  }
  get onA11yTab() {
    return this._onA11yTabEmitter.event;
  }
  get onWillOpen() {
    return this._onWillOpen.event;
  }
  _handleColorEvent(e) {
    if (this._themeService)
      for (let i of e) {
        let r, n = "";
        switch (i.index) {
          case 256:
            r = "foreground", n = "10";
            break;
          case 257:
            r = "background", n = "11";
            break;
          case 258:
            r = "cursor", n = "12";
            break;
          default:
            r = "ansi", n = "4;" + i.index;
        }
        switch (i.type) {
          case 0:
            let o = U2.toColorRGB(r === "ansi" ? this._themeService.colors.ansi[i.index] : this._themeService.colors[r]);
            this.coreService.triggerDataEvent(`${b.ESC}]${n};${ml(o)}${fs2.ST}`);
            break;
          case 1:
            if (r === "ansi")
              this._themeService.modifyColors((l) => l.ansi[i.index] = j2.toColor(...i.color));
            else {
              let l = r;
              this._themeService.modifyColors((a) => a[l] = j2.toColor(...i.color));
            }
            break;
          case 2:
            this._themeService.restoreColor(i.index);
            break;
        }
      }
  }
  _setup() {
    super._setup(), this._customKeyEventHandler = undefined;
  }
  get buffer() {
    return this.buffers.active;
  }
  focus() {
    this.textarea && this.textarea.focus({ preventScroll: true });
  }
  _handleScreenReaderModeOptionChange(e) {
    e ? !this._accessibilityManager.value && this._renderService && (this._accessibilityManager.value = this._instantiationService.createInstance(Tt2, this)) : this._accessibilityManager.clear();
  }
  _handleTextAreaFocus(e) {
    this.coreService.decPrivateModes.sendFocus && this.coreService.triggerDataEvent(b.ESC + "[I"), this.element.classList.add("focus"), this._showCursor(), this._onFocus.fire();
  }
  blur() {
    return this.textarea?.blur();
  }
  _handleTextAreaBlur() {
    this.textarea.value = "", this.refresh(this.buffer.y, this.buffer.y), this.coreService.decPrivateModes.sendFocus && this.coreService.triggerDataEvent(b.ESC + "[O"), this.element.classList.remove("focus"), this._onBlur.fire();
  }
  _syncTextArea() {
    if (!this.textarea || !this.buffer.isCursorInViewport || this._compositionHelper.isComposing || !this._renderService)
      return;
    let e = this.buffer.ybase + this.buffer.y, i = this.buffer.lines.get(e);
    if (!i)
      return;
    let r = Math.min(this.buffer.x, this.cols - 1), n = this._renderService.dimensions.css.cell.height, o = i.getWidth(r), l = this._renderService.dimensions.css.cell.width * o, a = this.buffer.y * this._renderService.dimensions.css.cell.height, u = r * this._renderService.dimensions.css.cell.width;
    this.textarea.style.left = u + "px", this.textarea.style.top = a + "px", this.textarea.style.width = l + "px", this.textarea.style.height = n + "px", this.textarea.style.lineHeight = n + "px", this.textarea.style.zIndex = "-5";
  }
  _initGlobal() {
    this._bindKeys(), this._register(L(this.element, "copy", (i) => {
      this.hasSelection() && Vs(i, this._selectionService);
    }));
    let e = (i) => qs(i, this.textarea, this.coreService, this.optionsService);
    this._register(L(this.textarea, "paste", e)), this._register(L(this.element, "paste", e)), Ss2 ? this._register(L(this.element, "mousedown", (i) => {
      i.button === 2 && Pn2(i, this.textarea, this.screenElement, this._selectionService, this.options.rightClickSelectsWord);
    })) : this._register(L(this.element, "contextmenu", (i) => {
      Pn2(i, this.textarea, this.screenElement, this._selectionService, this.options.rightClickSelectsWord);
    })), Bi2 && this._register(L(this.element, "auxclick", (i) => {
      i.button === 1 && Mn2(i, this.textarea, this.screenElement);
    }));
  }
  _bindKeys() {
    this._register(L(this.textarea, "keyup", (e) => this._keyUp(e), true)), this._register(L(this.textarea, "keydown", (e) => this._keyDown(e), true)), this._register(L(this.textarea, "keypress", (e) => this._keyPress(e), true)), this._register(L(this.textarea, "compositionstart", () => this._compositionHelper.compositionstart())), this._register(L(this.textarea, "compositionupdate", (e) => this._compositionHelper.compositionupdate(e))), this._register(L(this.textarea, "compositionend", () => this._compositionHelper.compositionend())), this._register(L(this.textarea, "input", (e) => this._inputEvent(e), true)), this._register(this.onRender(() => this._compositionHelper.updateCompositionElements()));
  }
  open(e) {
    if (!e)
      throw new Error("Terminal requires a parent element.");
    if (e.isConnected || this._logService.debug("Terminal.open was called on an element that was not attached to the DOM"), this.element?.ownerDocument.defaultView && this._coreBrowserService) {
      this.element.ownerDocument.defaultView !== this._coreBrowserService.window && (this._coreBrowserService.window = this.element.ownerDocument.defaultView);
      return;
    }
    this._document = e.ownerDocument, this.options.documentOverride && this.options.documentOverride instanceof Document && (this._document = this.optionsService.rawOptions.documentOverride), this.element = this._document.createElement("div"), this.element.dir = "ltr", this.element.classList.add("terminal"), this.element.classList.add("xterm"), e.appendChild(this.element);
    let i = this._document.createDocumentFragment();
    this._viewportElement = this._document.createElement("div"), this._viewportElement.classList.add("xterm-viewport"), i.appendChild(this._viewportElement), this.screenElement = this._document.createElement("div"), this.screenElement.classList.add("xterm-screen"), this._register(L(this.screenElement, "mousemove", (o) => this.updateCursorStyle(o))), this._helperContainer = this._document.createElement("div"), this._helperContainer.classList.add("xterm-helpers"), this.screenElement.appendChild(this._helperContainer), i.appendChild(this.screenElement);
    let r = this.textarea = this._document.createElement("textarea");
    this.textarea.classList.add("xterm-helper-textarea"), this.textarea.setAttribute("aria-label", mi2.get()), Ts2 || this.textarea.setAttribute("aria-multiline", "false"), this.textarea.setAttribute("autocorrect", "off"), this.textarea.setAttribute("autocapitalize", "off"), this.textarea.setAttribute("spellcheck", "false"), this.textarea.tabIndex = 0, this._register(this.optionsService.onSpecificOptionChange("disableStdin", () => r.readOnly = this.optionsService.rawOptions.disableStdin)), this.textarea.readOnly = this.optionsService.rawOptions.disableStdin, this._coreBrowserService = this._register(this._instantiationService.createInstance(Jr2, this.textarea, e.ownerDocument.defaultView ?? window, this._document ?? typeof window < "u" ? window.document : null)), this._instantiationService.setService(ae2, this._coreBrowserService), this._register(L(this.textarea, "focus", (o) => this._handleTextAreaFocus(o))), this._register(L(this.textarea, "blur", () => this._handleTextAreaBlur())), this._helperContainer.appendChild(this.textarea), this._charSizeService = this._instantiationService.createInstance(jt2, this._document, this._helperContainer), this._instantiationService.setService(nt2, this._charSizeService), this._themeService = this._instantiationService.createInstance(ti2), this._instantiationService.setService(Re, this._themeService), this._characterJoinerService = this._instantiationService.createInstance(ct), this._instantiationService.setService(or2, this._characterJoinerService), this._renderService = this._register(this._instantiationService.createInstance(Qt2, this.rows, this.screenElement)), this._instantiationService.setService(ce, this._renderService), this._register(this._renderService.onRenderedViewportChange((o) => this._onRender.fire(o))), this.onResize((o) => this._renderService.resize(o.cols, o.rows)), this._compositionView = this._document.createElement("div"), this._compositionView.classList.add("composition-view"), this._compositionHelper = this._instantiationService.createInstance($t2, this.textarea, this._compositionView), this._helperContainer.appendChild(this._compositionView), this._mouseService = this._instantiationService.createInstance(Xt2), this._instantiationService.setService(Dt2, this._mouseService);
    let n = this._linkifier.value = this._register(this._instantiationService.createInstance(hi, this.screenElement));
    this.element.appendChild(i);
    try {
      this._onWillOpen.fire(this.element);
    } catch {}
    this._renderService.hasRenderer() || this._renderService.setRenderer(this._createRenderer()), this._register(this.onCursorMove(() => {
      this._renderService.handleCursorMove(), this._syncTextArea();
    })), this._register(this.onResize(() => this._renderService.handleResize(this.cols, this.rows))), this._register(this.onBlur(() => this._renderService.handleBlur())), this._register(this.onFocus(() => this._renderService.handleFocus())), this._viewport = this._register(this._instantiationService.createInstance(zt2, this.element, this.screenElement)), this._register(this._viewport.onRequestScrollLines((o) => {
      super.scrollLines(o, false), this.refresh(0, this.rows - 1);
    })), this._selectionService = this._register(this._instantiationService.createInstance(ei2, this.element, this.screenElement, n)), this._instantiationService.setService(Qs, this._selectionService), this._register(this._selectionService.onRequestScrollLines((o) => this.scrollLines(o.amount, o.suppressScrollEvent))), this._register(this._selectionService.onSelectionChange(() => this._onSelectionChange.fire())), this._register(this._selectionService.onRequestRedraw((o) => this._renderService.handleSelectionChanged(o.start, o.end, o.columnSelectMode))), this._register(this._selectionService.onLinuxMouseSelection((o) => {
      this.textarea.value = o, this.textarea.focus(), this.textarea.select();
    })), this._register($.any(this._onScroll.event, this._inputHandler.onScroll)(() => {
      this._selectionService.refresh(), this._viewport?.queueSync();
    })), this._register(this._instantiationService.createInstance(Gt2, this.screenElement)), this._register(L(this.element, "mousedown", (o) => this._selectionService.handleMouseDown(o))), this.coreMouseService.areMouseEventsActive ? (this._selectionService.disable(), this.element.classList.add("enable-mouse-events")) : this._selectionService.enable(), this.options.screenReaderMode && (this._accessibilityManager.value = this._instantiationService.createInstance(Tt2, this)), this._register(this.optionsService.onSpecificOptionChange("screenReaderMode", (o) => this._handleScreenReaderModeOptionChange(o))), this.options.overviewRuler.width && (this._overviewRulerRenderer = this._register(this._instantiationService.createInstance(bt2, this._viewportElement, this.screenElement))), this.optionsService.onSpecificOptionChange("overviewRuler", (o) => {
      !this._overviewRulerRenderer && o && this._viewportElement && this.screenElement && (this._overviewRulerRenderer = this._register(this._instantiationService.createInstance(bt2, this._viewportElement, this.screenElement)));
    }), this._charSizeService.measure(), this.refresh(0, this.rows - 1), this._initGlobal(), this.bindMouse();
  }
  _createRenderer() {
    return this._instantiationService.createInstance(Yt2, this, this._document, this.element, this.screenElement, this._viewportElement, this._helperContainer, this.linkifier);
  }
  bindMouse() {
    let e = this, i = this.element;
    function r(l) {
      let a = e._mouseService.getMouseReportCoords(l, e.screenElement);
      if (!a)
        return false;
      let u, h;
      switch (l.overrideType || l.type) {
        case "mousemove":
          h = 32, l.buttons === undefined ? (u = 3, l.button !== undefined && (u = l.button < 3 ? l.button : 3)) : u = l.buttons & 1 ? 0 : l.buttons & 4 ? 1 : l.buttons & 2 ? 2 : 3;
          break;
        case "mouseup":
          h = 0, u = l.button < 3 ? l.button : 3;
          break;
        case "mousedown":
          h = 1, u = l.button < 3 ? l.button : 3;
          break;
        case "wheel":
          if (e._customWheelEventHandler && e._customWheelEventHandler(l) === false)
            return false;
          let c = l.deltaY;
          if (c === 0 || e.coreMouseService.consumeWheelEvent(l, e._renderService?.dimensions?.device?.cell?.height, e._coreBrowserService?.dpr) === 0)
            return false;
          h = c < 0 ? 0 : 1, u = 4;
          break;
        default:
          return false;
      }
      return h === undefined || u === undefined || u > 4 ? false : e.coreMouseService.triggerMouseEvent({ col: a.col, row: a.row, x: a.x, y: a.y, button: u, action: h, ctrl: l.ctrlKey, alt: l.altKey, shift: l.shiftKey });
    }
    let n = { mouseup: null, wheel: null, mousedrag: null, mousemove: null }, o = { mouseup: (l) => (r(l), l.buttons || (this._document.removeEventListener("mouseup", n.mouseup), n.mousedrag && this._document.removeEventListener("mousemove", n.mousedrag)), this.cancel(l)), wheel: (l) => (r(l), this.cancel(l, true)), mousedrag: (l) => {
      l.buttons && r(l);
    }, mousemove: (l) => {
      l.buttons || r(l);
    } };
    this._register(this.coreMouseService.onProtocolChange((l) => {
      l ? (this.optionsService.rawOptions.logLevel === "debug" && this._logService.debug("Binding to mouse events:", this.coreMouseService.explainEvents(l)), this.element.classList.add("enable-mouse-events"), this._selectionService.disable()) : (this._logService.debug("Unbinding from mouse events."), this.element.classList.remove("enable-mouse-events"), this._selectionService.enable()), l & 8 ? n.mousemove || (i.addEventListener("mousemove", o.mousemove), n.mousemove = o.mousemove) : (i.removeEventListener("mousemove", n.mousemove), n.mousemove = null), l & 16 ? n.wheel || (i.addEventListener("wheel", o.wheel, { passive: false }), n.wheel = o.wheel) : (i.removeEventListener("wheel", n.wheel), n.wheel = null), l & 2 ? n.mouseup || (n.mouseup = o.mouseup) : (this._document.removeEventListener("mouseup", n.mouseup), n.mouseup = null), l & 4 ? n.mousedrag || (n.mousedrag = o.mousedrag) : (this._document.removeEventListener("mousemove", n.mousedrag), n.mousedrag = null);
    })), this.coreMouseService.activeProtocol = this.coreMouseService.activeProtocol, this._register(L(i, "mousedown", (l) => {
      if (l.preventDefault(), this.focus(), !(!this.coreMouseService.areMouseEventsActive || this._selectionService.shouldForceSelection(l)))
        return r(l), n.mouseup && this._document.addEventListener("mouseup", n.mouseup), n.mousedrag && this._document.addEventListener("mousemove", n.mousedrag), this.cancel(l);
    })), this._register(L(i, "wheel", (l) => {
      if (!n.wheel) {
        if (this._customWheelEventHandler && this._customWheelEventHandler(l) === false)
          return false;
        if (!this.buffer.hasScrollback) {
          if (l.deltaY === 0)
            return false;
          if (e.coreMouseService.consumeWheelEvent(l, e._renderService?.dimensions?.device?.cell?.height, e._coreBrowserService?.dpr) === 0)
            return this.cancel(l, true);
          let h = b.ESC + (this.coreService.decPrivateModes.applicationCursorKeys ? "O" : "[") + (l.deltaY < 0 ? "A" : "B");
          return this.coreService.triggerDataEvent(h, true), this.cancel(l, true);
        }
      }
    }, { passive: false }));
  }
  refresh(e, i) {
    this._renderService?.refreshRows(e, i);
  }
  updateCursorStyle(e) {
    this._selectionService?.shouldColumnSelect(e) ? this.element.classList.add("column-select") : this.element.classList.remove("column-select");
  }
  _showCursor() {
    this.coreService.isCursorInitialized || (this.coreService.isCursorInitialized = true, this.refresh(this.buffer.y, this.buffer.y));
  }
  scrollLines(e, i) {
    this._viewport ? this._viewport.scrollLines(e) : super.scrollLines(e, i), this.refresh(0, this.rows - 1);
  }
  scrollPages(e) {
    this.scrollLines(e * (this.rows - 1));
  }
  scrollToTop() {
    this.scrollLines(-this._bufferService.buffer.ydisp);
  }
  scrollToBottom(e) {
    e && this._viewport ? this._viewport.scrollToLine(this.buffer.ybase, true) : this.scrollLines(this._bufferService.buffer.ybase - this._bufferService.buffer.ydisp);
  }
  scrollToLine(e) {
    let i = e - this._bufferService.buffer.ydisp;
    i !== 0 && this.scrollLines(i);
  }
  paste(e) {
    Cn2(e, this.textarea, this.coreService, this.optionsService);
  }
  attachCustomKeyEventHandler(e) {
    this._customKeyEventHandler = e;
  }
  attachCustomWheelEventHandler(e) {
    this._customWheelEventHandler = e;
  }
  registerLinkProvider(e) {
    return this._linkProviderService.registerLinkProvider(e);
  }
  registerCharacterJoiner(e) {
    if (!this._characterJoinerService)
      throw new Error("Terminal must be opened first");
    let i = this._characterJoinerService.register(e);
    return this.refresh(0, this.rows - 1), i;
  }
  deregisterCharacterJoiner(e) {
    if (!this._characterJoinerService)
      throw new Error("Terminal must be opened first");
    this._characterJoinerService.deregister(e) && this.refresh(0, this.rows - 1);
  }
  get markers() {
    return this.buffer.markers;
  }
  registerMarker(e) {
    return this.buffer.addMarker(this.buffer.ybase + this.buffer.y + e);
  }
  registerDecoration(e) {
    return this._decorationService.registerDecoration(e);
  }
  hasSelection() {
    return this._selectionService ? this._selectionService.hasSelection : false;
  }
  select(e, i, r) {
    this._selectionService.setSelection(e, i, r);
  }
  getSelection() {
    return this._selectionService ? this._selectionService.selectionText : "";
  }
  getSelectionPosition() {
    if (!(!this._selectionService || !this._selectionService.hasSelection))
      return { start: { x: this._selectionService.selectionStart[0], y: this._selectionService.selectionStart[1] }, end: { x: this._selectionService.selectionEnd[0], y: this._selectionService.selectionEnd[1] } };
  }
  clearSelection() {
    this._selectionService?.clearSelection();
  }
  selectAll() {
    this._selectionService?.selectAll();
  }
  selectLines(e, i) {
    this._selectionService?.selectLines(e, i);
  }
  _keyDown(e) {
    if (this._keyDownHandled = false, this._keyDownSeen = true, this._customKeyEventHandler && this._customKeyEventHandler(e) === false)
      return false;
    let i = this.browser.isMac && this.options.macOptionIsMeta && e.altKey;
    if (!i && !this._compositionHelper.keydown(e))
      return this.options.scrollOnUserInput && this.buffer.ybase !== this.buffer.ydisp && this.scrollToBottom(true), false;
    !i && (e.key === "Dead" || e.key === "AltGraph") && (this._unprocessedDeadKey = true);
    let r = Il(e, this.coreService.decPrivateModes.applicationCursorKeys, this.browser.isMac, this.options.macOptionIsMeta);
    if (this.updateCursorStyle(e), r.type === 3 || r.type === 2) {
      let n = this.rows - 1;
      return this.scrollLines(r.type === 2 ? -n : n), this.cancel(e, true);
    }
    if (r.type === 1 && this.selectAll(), this._isThirdLevelShift(this.browser, e) || (r.cancel && this.cancel(e, true), !r.key) || e.key && !e.ctrlKey && !e.altKey && !e.metaKey && e.key.length === 1 && e.key.charCodeAt(0) >= 65 && e.key.charCodeAt(0) <= 90)
      return true;
    if (this._unprocessedDeadKey)
      return this._unprocessedDeadKey = false, true;
    if ((r.key === b.ETX || r.key === b.CR) && (this.textarea.value = ""), this._onKey.fire({ key: r.key, domEvent: e }), this._showCursor(), this.coreService.triggerDataEvent(r.key, true), !this.optionsService.rawOptions.screenReaderMode || e.altKey || e.ctrlKey)
      return this.cancel(e, true);
    this._keyDownHandled = true;
  }
  _isThirdLevelShift(e, i) {
    let r = e.isMac && !this.options.macOptionIsMeta && i.altKey && !i.ctrlKey && !i.metaKey || e.isWindows && i.altKey && i.ctrlKey && !i.metaKey || e.isWindows && i.getModifierState("AltGraph");
    return i.type === "keypress" ? r : r && (!i.keyCode || i.keyCode > 47);
  }
  _keyUp(e) {
    this._keyDownSeen = false, !(this._customKeyEventHandler && this._customKeyEventHandler(e) === false) && (Tc(e) || this.focus(), this.updateCursorStyle(e), this._keyPressHandled = false);
  }
  _keyPress(e) {
    let i;
    if (this._keyPressHandled = false, this._keyDownHandled || this._customKeyEventHandler && this._customKeyEventHandler(e) === false)
      return false;
    if (this.cancel(e), e.charCode)
      i = e.charCode;
    else if (e.which === null || e.which === undefined)
      i = e.keyCode;
    else if (e.which !== 0 && e.charCode !== 0)
      i = e.which;
    else
      return false;
    return !i || (e.altKey || e.ctrlKey || e.metaKey) && !this._isThirdLevelShift(this.browser, e) ? false : (i = String.fromCharCode(i), this._onKey.fire({ key: i, domEvent: e }), this._showCursor(), this.coreService.triggerDataEvent(i, true), this._keyPressHandled = true, this._unprocessedDeadKey = false, true);
  }
  _inputEvent(e) {
    if (e.data && e.inputType === "insertText" && (!e.composed || !this._keyDownSeen) && !this.optionsService.rawOptions.screenReaderMode) {
      if (this._keyPressHandled)
        return false;
      this._unprocessedDeadKey = false;
      let i = e.data;
      return this.coreService.triggerDataEvent(i, true), this.cancel(e), true;
    }
    return false;
  }
  resize(e, i) {
    if (e === this.cols && i === this.rows) {
      this._charSizeService && !this._charSizeService.hasValidSize && this._charSizeService.measure();
      return;
    }
    super.resize(e, i);
  }
  _afterResize(e, i) {
    this._charSizeService?.measure();
  }
  clear() {
    if (!(this.buffer.ybase === 0 && this.buffer.y === 0)) {
      this.buffer.clearAllMarkers(), this.buffer.lines.set(0, this.buffer.lines.get(this.buffer.ybase + this.buffer.y)), this.buffer.lines.length = 1, this.buffer.ydisp = 0, this.buffer.ybase = 0, this.buffer.y = 0;
      for (let e = 1;e < this.rows; e++)
        this.buffer.lines.push(this.buffer.getBlankLine(X2));
      this._onScroll.fire({ position: this.buffer.ydisp }), this.refresh(0, this.rows - 1);
    }
  }
  reset() {
    this.options.rows = this.rows, this.options.cols = this.cols;
    let e = this._customKeyEventHandler;
    this._setup(), super.reset(), this._selectionService?.reset(), this._decorationService.reset(), this._customKeyEventHandler = e, this.refresh(0, this.rows - 1);
  }
  clearTextureAtlas() {
    this._renderService?.clearTextureAtlas();
  }
  _reportFocus() {
    this.element?.classList.contains("focus") ? this.coreService.triggerDataEvent(b.ESC + "[I") : this.coreService.triggerDataEvent(b.ESC + "[O");
  }
  _reportWindowsOptions(e) {
    if (this._renderService)
      switch (e) {
        case 0:
          let i = this._renderService.dimensions.css.canvas.width.toFixed(0), r = this._renderService.dimensions.css.canvas.height.toFixed(0);
          this.coreService.triggerDataEvent(`${b.ESC}[4;${r};${i}t`);
          break;
        case 1:
          let n = this._renderService.dimensions.css.cell.width.toFixed(0), o = this._renderService.dimensions.css.cell.height.toFixed(0);
          this.coreService.triggerDataEvent(`${b.ESC}[6;${o};${n}t`);
          break;
      }
  }
  cancel(e, i) {
    if (!(!this.options.cancelEvents && !i))
      return e.preventDefault(), e.stopPropagation(), false;
  }
};
function Tc(s) {
  return s.keyCode === 16 || s.keyCode === 17 || s.keyCode === 18;
}
var xn2 = class {
  constructor() {
    this._addons = [];
  }
  dispose() {
    for (let t = this._addons.length - 1;t >= 0; t--)
      this._addons[t].instance.dispose();
  }
  loadAddon(t, e) {
    let i = { instance: e, dispose: e.dispose, isDisposed: false };
    this._addons.push(i), e.dispose = () => this._wrappedAddonDispose(i), e.activate(t);
  }
  _wrappedAddonDispose(t) {
    if (t.isDisposed)
      return;
    let e = -1;
    for (let i = 0;i < this._addons.length; i++)
      if (this._addons[i] === t) {
        e = i;
        break;
      }
    if (e === -1)
      throw new Error("Could not dispose an addon that has not been loaded");
    t.isDisposed = true, t.dispose.apply(t.instance), this._addons.splice(e, 1);
  }
};
var wn2 = class {
  constructor(t) {
    this._line = t;
  }
  get isWrapped() {
    return this._line.isWrapped;
  }
  get length() {
    return this._line.length;
  }
  getCell(t, e) {
    if (!(t < 0 || t >= this._line.length))
      return e ? (this._line.loadCell(t, e), e) : this._line.loadCell(t, new q);
  }
  translateToString(t, e, i) {
    return this._line.translateToString(t, e, i);
  }
};
var Ji = class {
  constructor(t, e) {
    this._buffer = t;
    this.type = e;
  }
  init(t) {
    return this._buffer = t, this;
  }
  get cursorY() {
    return this._buffer.y;
  }
  get cursorX() {
    return this._buffer.x;
  }
  get viewportY() {
    return this._buffer.ydisp;
  }
  get baseY() {
    return this._buffer.ybase;
  }
  get length() {
    return this._buffer.lines.length;
  }
  getLine(t) {
    let e = this._buffer.lines.get(t);
    if (e)
      return new wn2(e);
  }
  getNullCell() {
    return new q;
  }
};
var Dn2 = class extends D2 {
  constructor(e) {
    super();
    this._core = e;
    this._onBufferChange = this._register(new v);
    this.onBufferChange = this._onBufferChange.event;
    this._normal = new Ji(this._core.buffers.normal, "normal"), this._alternate = new Ji(this._core.buffers.alt, "alternate"), this._core.buffers.onBufferActivate(() => this._onBufferChange.fire(this.active));
  }
  get active() {
    if (this._core.buffers.active === this._core.buffers.normal)
      return this.normal;
    if (this._core.buffers.active === this._core.buffers.alt)
      return this.alternate;
    throw new Error("Active buffer is neither normal nor alternate");
  }
  get normal() {
    return this._normal.init(this._core.buffers.normal);
  }
  get alternate() {
    return this._alternate.init(this._core.buffers.alt);
  }
};
var Rn = class {
  constructor(t) {
    this._core = t;
  }
  registerCsiHandler(t, e) {
    return this._core.registerCsiHandler(t, (i) => e(i.toArray()));
  }
  addCsiHandler(t, e) {
    return this.registerCsiHandler(t, e);
  }
  registerDcsHandler(t, e) {
    return this._core.registerDcsHandler(t, (i, r) => e(i, r.toArray()));
  }
  addDcsHandler(t, e) {
    return this.registerDcsHandler(t, e);
  }
  registerEscHandler(t, e) {
    return this._core.registerEscHandler(t, e);
  }
  addEscHandler(t, e) {
    return this.registerEscHandler(t, e);
  }
  registerOscHandler(t, e) {
    return this._core.registerOscHandler(t, e);
  }
  addOscHandler(t, e) {
    return this.registerOscHandler(t, e);
  }
};
var Ln2 = class {
  constructor(t) {
    this._core = t;
  }
  register(t) {
    this._core.unicodeService.register(t);
  }
  get versions() {
    return this._core.unicodeService.versions;
  }
  get activeVersion() {
    return this._core.unicodeService.activeVersion;
  }
  set activeVersion(t) {
    this._core.unicodeService.activeVersion = t;
  }
};
var Ic = ["cols", "rows"];
var Ue2 = 0;
var Dl = class extends D2 {
  constructor(t) {
    super(), this._core = this._register(new yn2(t)), this._addonManager = this._register(new xn2), this._publicOptions = { ...this._core.options };
    let e = (r) => this._core.options[r], i = (r, n) => {
      this._checkReadonlyOptions(r), this._core.options[r] = n;
    };
    for (let r in this._core.options) {
      let n = { get: e.bind(this, r), set: i.bind(this, r) };
      Object.defineProperty(this._publicOptions, r, n);
    }
  }
  _checkReadonlyOptions(t) {
    if (Ic.includes(t))
      throw new Error(`Option "${t}" can only be set in the constructor`);
  }
  _checkProposedApi() {
    if (!this._core.optionsService.rawOptions.allowProposedApi)
      throw new Error("You must set the allowProposedApi option to true to use proposed API");
  }
  get onBell() {
    return this._core.onBell;
  }
  get onBinary() {
    return this._core.onBinary;
  }
  get onCursorMove() {
    return this._core.onCursorMove;
  }
  get onData() {
    return this._core.onData;
  }
  get onKey() {
    return this._core.onKey;
  }
  get onLineFeed() {
    return this._core.onLineFeed;
  }
  get onRender() {
    return this._core.onRender;
  }
  get onResize() {
    return this._core.onResize;
  }
  get onScroll() {
    return this._core.onScroll;
  }
  get onSelectionChange() {
    return this._core.onSelectionChange;
  }
  get onTitleChange() {
    return this._core.onTitleChange;
  }
  get onWriteParsed() {
    return this._core.onWriteParsed;
  }
  get element() {
    return this._core.element;
  }
  get parser() {
    return this._parser || (this._parser = new Rn(this._core)), this._parser;
  }
  get unicode() {
    return this._checkProposedApi(), new Ln2(this._core);
  }
  get textarea() {
    return this._core.textarea;
  }
  get rows() {
    return this._core.rows;
  }
  get cols() {
    return this._core.cols;
  }
  get buffer() {
    return this._buffer || (this._buffer = this._register(new Dn2(this._core))), this._buffer;
  }
  get markers() {
    return this._checkProposedApi(), this._core.markers;
  }
  get modes() {
    let t = this._core.coreService.decPrivateModes, e = "none";
    switch (this._core.coreMouseService.activeProtocol) {
      case "X10":
        e = "x10";
        break;
      case "VT200":
        e = "vt200";
        break;
      case "DRAG":
        e = "drag";
        break;
      case "ANY":
        e = "any";
        break;
    }
    return { applicationCursorKeysMode: t.applicationCursorKeys, applicationKeypadMode: t.applicationKeypad, bracketedPasteMode: t.bracketedPasteMode, insertMode: this._core.coreService.modes.insertMode, mouseTrackingMode: e, originMode: t.origin, reverseWraparoundMode: t.reverseWraparound, sendFocusMode: t.sendFocus, synchronizedOutputMode: t.synchronizedOutput, wraparoundMode: t.wraparound };
  }
  get options() {
    return this._publicOptions;
  }
  set options(t) {
    for (let e in t)
      this._publicOptions[e] = t[e];
  }
  blur() {
    this._core.blur();
  }
  focus() {
    this._core.focus();
  }
  input(t, e = true) {
    this._core.input(t, e);
  }
  resize(t, e) {
    this._verifyIntegers(t, e), this._core.resize(t, e);
  }
  open(t) {
    this._core.open(t);
  }
  attachCustomKeyEventHandler(t) {
    this._core.attachCustomKeyEventHandler(t);
  }
  attachCustomWheelEventHandler(t) {
    this._core.attachCustomWheelEventHandler(t);
  }
  registerLinkProvider(t) {
    return this._core.registerLinkProvider(t);
  }
  registerCharacterJoiner(t) {
    return this._checkProposedApi(), this._core.registerCharacterJoiner(t);
  }
  deregisterCharacterJoiner(t) {
    this._checkProposedApi(), this._core.deregisterCharacterJoiner(t);
  }
  registerMarker(t = 0) {
    return this._verifyIntegers(t), this._core.registerMarker(t);
  }
  registerDecoration(t) {
    return this._checkProposedApi(), this._verifyPositiveIntegers(t.x ?? 0, t.width ?? 0, t.height ?? 0), this._core.registerDecoration(t);
  }
  hasSelection() {
    return this._core.hasSelection();
  }
  select(t, e, i) {
    this._verifyIntegers(t, e, i), this._core.select(t, e, i);
  }
  getSelection() {
    return this._core.getSelection();
  }
  getSelectionPosition() {
    return this._core.getSelectionPosition();
  }
  clearSelection() {
    this._core.clearSelection();
  }
  selectAll() {
    this._core.selectAll();
  }
  selectLines(t, e) {
    this._verifyIntegers(t, e), this._core.selectLines(t, e);
  }
  dispose() {
    super.dispose();
  }
  scrollLines(t) {
    this._verifyIntegers(t), this._core.scrollLines(t);
  }
  scrollPages(t) {
    this._verifyIntegers(t), this._core.scrollPages(t);
  }
  scrollToTop() {
    this._core.scrollToTop();
  }
  scrollToBottom() {
    this._core.scrollToBottom();
  }
  scrollToLine(t) {
    this._verifyIntegers(t), this._core.scrollToLine(t);
  }
  clear() {
    this._core.clear();
  }
  write(t, e) {
    this._core.write(t, e);
  }
  writeln(t, e) {
    this._core.write(t), this._core.write(`\r
`, e);
  }
  paste(t) {
    this._core.paste(t);
  }
  refresh(t, e) {
    this._verifyIntegers(t, e), this._core.refresh(t, e);
  }
  reset() {
    this._core.reset();
  }
  clearTextureAtlas() {
    this._core.clearTextureAtlas();
  }
  loadAddon(t) {
    this._addonManager.loadAddon(this, t);
  }
  static get strings() {
    return { get promptLabel() {
      return mi2.get();
    }, set promptLabel(t) {
      mi2.set(t);
    }, get tooMuchOutput() {
      return _i2.get();
    }, set tooMuchOutput(t) {
      _i2.set(t);
    } };
  }
  _verifyIntegers(...t) {
    for (Ue2 of t)
      if (Ue2 === 1 / 0 || isNaN(Ue2) || Ue2 % 1 !== 0)
        throw new Error("This API only accepts integers");
  }
  _verifyPositiveIntegers(...t) {
    for (Ue2 of t)
      if (Ue2 && (Ue2 === 1 / 0 || isNaN(Ue2) || Ue2 % 1 !== 0 || Ue2 < 0))
        throw new Error("This API only accepts positive integers");
  }
};

// node_modules/@xterm/xterm/css/xterm.css
var xterm_default = `/**
 * Copyright (c) 2014 The xterm.js authors. All rights reserved.
 * Copyright (c) 2012-2013, Christopher Jeffrey (MIT License)
 * https://github.com/chjj/term.js
 * @license MIT
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in
 * all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
 * THE SOFTWARE.
 *
 * Originally forked from (with the author's permission):
 *   Fabrice Bellard's javascript vt100 for jslinux:
 *   http://bellard.org/jslinux/
 *   Copyright (c) 2011 Fabrice Bellard
 *   The original design remains. The terminal itself
 *   has been extended to include xterm CSI codes, among
 *   other features.
 */

/**
 *  Default styles for xterm.js
 */

.xterm {
    cursor: text;
    position: relative;
    user-select: none;
    -ms-user-select: none;
    -webkit-user-select: none;
}

.xterm.focus,
.xterm:focus {
    outline: none;
}

.xterm .xterm-helpers {
    position: absolute;
    top: 0;
    /**
     * The z-index of the helpers must be higher than the canvases in order for
     * IMEs to appear on top.
     */
    z-index: 5;
}

.xterm .xterm-helper-textarea {
    padding: 0;
    border: 0;
    margin: 0;
    /* Move textarea out of the screen to the far left, so that the cursor is not visible */
    position: absolute;
    opacity: 0;
    left: -9999em;
    top: 0;
    width: 0;
    height: 0;
    z-index: -5;
    /** Prevent wrapping so the IME appears against the textarea at the correct position */
    white-space: nowrap;
    overflow: hidden;
    resize: none;
}

.xterm .composition-view {
    /* TODO: Composition position got messed up somewhere */
    background: #000;
    color: #FFF;
    display: none;
    position: absolute;
    white-space: nowrap;
    z-index: 1;
}

.xterm .composition-view.active {
    display: block;
}

.xterm .xterm-viewport {
    /* On OS X this is required in order for the scroll bar to appear fully opaque */
    background-color: #000;
    overflow-y: scroll;
    cursor: default;
    position: absolute;
    right: 0;
    left: 0;
    top: 0;
    bottom: 0;
}

.xterm .xterm-screen {
    position: relative;
}

.xterm .xterm-screen canvas {
    position: absolute;
    left: 0;
    top: 0;
}

.xterm-char-measure-element {
    display: inline-block;
    visibility: hidden;
    position: absolute;
    top: 0;
    left: -9999em;
    line-height: normal;
}

.xterm.enable-mouse-events {
    /* When mouse events are enabled (eg. tmux), revert to the standard pointer cursor */
    cursor: default;
}

.xterm.xterm-cursor-pointer,
.xterm .xterm-cursor-pointer {
    cursor: pointer;
}

.xterm.column-select.focus {
    /* Column selection mode */
    cursor: crosshair;
}

.xterm .xterm-accessibility:not(.debug),
.xterm .xterm-message {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    right: 0;
    z-index: 10;
    color: transparent;
    pointer-events: none;
}

.xterm .xterm-accessibility-tree:not(.debug) *::selection {
  color: transparent;
}

.xterm .xterm-accessibility-tree {
  font-family: monospace;
  user-select: text;
  white-space: pre;
}

.xterm .xterm-accessibility-tree > div {
  transform-origin: left;
  width: fit-content;
}

.xterm .live-region {
    position: absolute;
    left: -9999px;
    width: 1px;
    height: 1px;
    overflow: hidden;
}

.xterm-dim {
    /* Dim should not apply to background, so the opacity of the foreground color is applied
     * explicitly in the generated class and reset to 1 here */
    opacity: 1 !important;
}

.xterm-underline-1 { text-decoration: underline; }
.xterm-underline-2 { text-decoration: double underline; }
.xterm-underline-3 { text-decoration: wavy underline; }
.xterm-underline-4 { text-decoration: dotted underline; }
.xterm-underline-5 { text-decoration: dashed underline; }

.xterm-overline {
    text-decoration: overline;
}

.xterm-overline.xterm-underline-1 { text-decoration: overline underline; }
.xterm-overline.xterm-underline-2 { text-decoration: overline double underline; }
.xterm-overline.xterm-underline-3 { text-decoration: overline wavy underline; }
.xterm-overline.xterm-underline-4 { text-decoration: overline dotted underline; }
.xterm-overline.xterm-underline-5 { text-decoration: overline dashed underline; }

.xterm-strikethrough {
    text-decoration: line-through;
}

.xterm-screen .xterm-decoration-container .xterm-decoration {
	z-index: 6;
	position: absolute;
}

.xterm-screen .xterm-decoration-container .xterm-decoration.xterm-decoration-top-layer {
	z-index: 7;
}

.xterm-decoration-overview-ruler {
    z-index: 8;
    position: absolute;
    top: 0;
    right: 0;
    pointer-events: none;
}

.xterm-decoration-top {
    z-index: 2;
    position: relative;
}



/* Derived from vs/base/browser/ui/scrollbar/media/scrollbar.css */

/* xterm.js customization: Override xterm's cursor style */
.xterm .xterm-scrollable-element > .scrollbar {
    cursor: default;
}

/* Arrows */
.xterm .xterm-scrollable-element > .scrollbar > .scra {
	cursor: pointer;
	font-size: 11px !important;
}

.xterm .xterm-scrollable-element > .visible {
	opacity: 1;

	/* Background rule added for IE9 - to allow clicks on dom node */
	background:rgba(0,0,0,0);

	transition: opacity 100ms linear;
	/* In front of peek view */
	z-index: 11;
}
.xterm .xterm-scrollable-element > .invisible {
	opacity: 0;
	pointer-events: none;
}
.xterm .xterm-scrollable-element > .invisible.fade {
	transition: opacity 800ms linear;
}

/* Scrollable Content Inset Shadow */
.xterm .xterm-scrollable-element > .shadow {
	position: absolute;
	display: none;
}
.xterm .xterm-scrollable-element > .shadow.top {
	display: block;
	top: 0;
	left: 3px;
	height: 3px;
	width: 100%;
	box-shadow: var(--vscode-scrollbar-shadow, #000) 0 6px 6px -6px inset;
}
.xterm .xterm-scrollable-element > .shadow.left {
	display: block;
	top: 3px;
	left: 0;
	height: 100%;
	width: 3px;
	box-shadow: var(--vscode-scrollbar-shadow, #000) 6px 0 6px -6px inset;
}
.xterm .xterm-scrollable-element > .shadow.top-left-corner {
	display: block;
	top: 0;
	left: 0;
	height: 3px;
	width: 3px;
}
.xterm .xterm-scrollable-element > .shadow.top.left {
	box-shadow: var(--vscode-scrollbar-shadow, #000) 6px 0 6px -6px inset;
}
`;

// src/api.ts
async function call(context, route, init = {}) {
  const { headers, ...rest } = init;
  const timeout = !rest.signal && typeof AbortSignal !== "undefined" && "timeout" in AbortSignal ? AbortSignal.timeout(8000) : undefined;
  const response = await context.fetch(`/api/apps/${encodeURIComponent(context.app.id)}${route}`, {
    ...rest,
    ...timeout ? { signal: timeout } : {},
    headers: { accept: "application/json", ...headers ?? {} }
  });
  const body = await response.text();
  let value = null;
  try {
    value = body ? JSON.parse(body) : null;
  } catch {
    value = body;
  }
  if (!response.ok) {
    const message = value && typeof value === "object" && value.error ? `${value.error.code}: ${value.error.message}` : `HTTP ${response.status}`;
    throw new Error(message);
  }
  return value;
}
async function openStream(context, route, signal) {
  const response = await context.fetch(`/api/apps/${encodeURIComponent(context.app.id)}${route}`, { headers: { accept: "application/octet-stream" }, signal });
  if (!response.ok) {
    const body = await response.text();
    let message = `HTTP ${response.status}`;
    try {
      const value = body ? JSON.parse(body) : null;
      if (value && typeof value === "object" && value.error) {
        message = `${value.error.code}: ${value.error.message}`;
      }
    } catch {}
    throw new Error(message);
  }
  if (!response.body)
    throw new Error("the stream has no body");
  return response.body.getReader();
}

// src/gestures.ts
function bindTerminalGestures(screen, handlers) {
  let lastTap = 0;
  let tapReset;
  let hold;
  let startX = 0;
  let startY = 0;
  let lastX = 0;
  let lastY = 0;
  let arrows = false;
  let moved = false;
  let pinching = false;
  let suppressTap = false;
  let pinchStart = 0;
  let pinchFont = 0;
  const clearHold = () => {
    if (hold !== undefined) {
      window.clearTimeout(hold);
      hold = undefined;
    }
  };
  const distance = (touches) => {
    const first = touches[0];
    const second = touches[1];
    if (!first || !second)
      return 0;
    return Math.hypot(first.clientX - second.clientX, first.clientY - second.clientY);
  };
  const onStart = (event) => {
    if (event.touches.length === 2) {
      clearHold();
      arrows = false;
      moved = false;
      pinching = true;
      suppressTap = true;
      pinchStart = distance(event.touches);
      pinchFont = handlers.getFontSize();
      event.preventDefault();
      return;
    }
    if (event.touches.length !== 1)
      return;
    const touch = event.touches[0];
    if (!touch)
      return;
    suppressTap = false;
    startX = lastX = touch.clientX;
    startY = lastY = touch.clientY;
    moved = false;
    arrows = false;
    pinching = false;
    clearHold();
    hold = window.setTimeout(() => {
      hold = undefined;
      arrows = true;
    }, 450);
  };
  const onMove = (event) => {
    if (pinching) {
      if (event.touches.length < 2)
        return;
      event.preventDefault();
      const current = distance(event.touches);
      if (pinchStart > 0) {
        handlers.setFontSize(pinchFont * (current / pinchStart));
      }
      return;
    }
    const touch = event.touches[0];
    if (!touch)
      return;
    if (arrows) {
      event.preventDefault();
      window.getSelection()?.removeAllRanges();
      const deltaX = touch.clientX - lastX;
      const deltaY = touch.clientY - lastY;
      const threshold = 24;
      if (Math.abs(deltaX) >= threshold || Math.abs(deltaY) >= threshold) {
        if (Math.abs(deltaX) >= Math.abs(deltaY)) {
          handlers.sendKey(deltaX > 0 ? "right" : "left");
        } else {
          handlers.sendKey(deltaY > 0 ? "down" : "up");
        }
        lastX = touch.clientX;
        lastY = touch.clientY;
      }
      return;
    }
    if (Math.abs(touch.clientX - startX) > 8 || Math.abs(touch.clientY - startY) > 8) {
      moved = true;
      clearHold();
    }
  };
  const onEnd = (event) => {
    if (pinching) {
      if (event.touches.length >= 2)
        return;
      pinching = false;
      suppressTap = true;
      return;
    }
    if (arrows) {
      arrows = false;
      moved = true;
      return;
    }
    clearHold();
    if (moved) {
      moved = false;
      return;
    }
    if (suppressTap) {
      suppressTap = false;
      return;
    }
    const now = Date.now();
    if (now - lastTap < 320) {
      event.preventDefault();
      lastTap = 0;
      if (tapReset !== undefined) {
        window.clearTimeout(tapReset);
        tapReset = undefined;
      }
      handlers.sendKey("tab");
      handlers.focusInput();
      return;
    }
    lastTap = now;
    event.preventDefault();
    handlers.focusInput();
    if (tapReset !== undefined)
      window.clearTimeout(tapReset);
    tapReset = window.setTimeout(() => {
      lastTap = 0;
      tapReset = undefined;
    }, 320);
  };
  const onCancel = () => {
    clearHold();
    arrows = false;
    moved = false;
    pinching = false;
    suppressTap = false;
  };
  screen.addEventListener("touchstart", onStart, { passive: false });
  screen.addEventListener("touchmove", onMove, { passive: false });
  screen.addEventListener("touchend", onEnd, { passive: false });
  screen.addEventListener("touchcancel", onCancel, { passive: false });
  return () => {
    clearHold();
    if (tapReset !== undefined)
      window.clearTimeout(tapReset);
    screen.removeEventListener("touchstart", onStart);
    screen.removeEventListener("touchmove", onMove);
    screen.removeEventListener("touchend", onEnd);
    screen.removeEventListener("touchcancel", onCancel);
  };
}

// src/keys.ts
var KEY_SEQUENCES = {
  esc: "\x1B",
  tab: "\t",
  up: "\x1B[A",
  down: "\x1B[B",
  right: "\x1B[C",
  left: "\x1B[D",
  home: "\x1B[H",
  end: "\x1B[F",
  pgup: "\x1B[5~",
  pgdn: "\x1B[6~",
  ctrlc: "\x03",
  ctrld: "\x04",
  ctrll: "\f",
  ctrlz: "\x1A"
};
var BAR_SYMBOLS = ["|", "~", "/", "-", "_", "="];
var encoder = new TextEncoder;
function encodeText(value) {
  return Array.from(encoder.encode(value));
}
function applyModifiers(value, ctrl, alt) {
  if (!ctrl && !alt || [...value].length !== 1)
    return value;
  const code = value.codePointAt(0) ?? 0;
  if (code < 32 || code === 127)
    return value;
  if (ctrl) {
    const upper = value.toUpperCase().codePointAt(0) ?? 0;
    if (upper >= 64 && upper <= 95) {
      return String.fromCharCode(upper & 31);
    }
    if (value === "?")
      return "";
    if (value === " ")
      return "\x00";
  }
  return alt ? `\x1B${value}` : value;
}

// src/styles.ts
var styles = `
  .term { position: relative; display: flex; flex-direction: column; gap: 10px; height: 100%; min-height: 0; }
  .term-bar { display: flex; align-items: center; gap: 8px; flex: none; font-size: 12px; color: #667087; }
  .term-bar .who { color: #8f98ab; }
  .term-bar .state { margin-left: auto; }
  .term-controls { display: flex; align-items: center; gap: 4px; }
  .term-bar button {
    padding: 4px 8px; border-radius: 6px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: inherit; font-size: 11px; cursor: pointer;
  }
  .term-bar button:hover { border-color: #3a4560; color: #fff; }
  .term-bar button:disabled { opacity: .4; cursor: default; }
  .term-screen {
    position: relative; flex: 1; min-height: 0; margin: 0; padding: 0; overflow: hidden;
    border: 1px solid #202632; border-radius: 9px; background: #080a0f;
    touch-action: pan-y; cursor: text;
  }
  .term-screen .xterm { height: 100%; padding: 10px 12px; }
  .term-screen .xterm-viewport { background: transparent !important; }
  .term-keys { display: none; }
  .term-keys button {
    flex: none; min-width: 34px; height: 36px; padding: 0 7px;
    border-radius: 6px; border: 1px solid #2a3142; background: #151a24;
    color: #c8d0df; font: 11px ui-monospace, monospace; white-space: nowrap;
    cursor: pointer;
  }
  .term-keys button:hover { border-color: #3a4560; color: #fff; }
  .term-keys button.armed { background: #24406e; border-color: #7aa2ff; color: #fff; }
  .term-keys button:disabled { opacity: .4; cursor: default; }
  .term-keys button svg { display: block; width: 15px; height: 15px; }
  .term-input {
    position: absolute; left: 8px; bottom: 8px; width: 1px; height: 1px;
    padding: 0; border: 0; opacity: 0; pointer-events: none;
    font-size: 16px; caret-color: transparent; outline: none;
    overflow: hidden; white-space: pre;
  }
  .term-status { margin: 0; min-height: 16px; flex: none; font-size: 12px; color: #667087; }
  .term-status.bad { color: #ffb3ba; }
  @media (max-width: 560px) {
    .term { gap: 8px; }
    .term-bar {
      display: grid; grid-template-columns: minmax(0, 1fr) auto;
      align-items: center; gap: 6px 8px;
    }
    .term-bar .who {
      grid-column: 1; grid-row: 1; min-width: 0;
      overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    }
    .term-bar .state {
      grid-column: 2; grid-row: 1; margin-left: 0;
      font-size: 11px; white-space: nowrap;
    }
    .term-controls {
      grid-column: 1 / -1; grid-row: 2;
      display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 6px;
    }
    .term-controls button {
      width: 100%; min-height: 38px; padding: 0 6px;
      font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    }
    .term-keys { display: flex; gap: 6px; flex: none; min-width: 0; }
    .term-modifiers {
      display: flex; gap: 4px; flex: none; padding-right: 6px;
      border-right: 1px solid #202634;
    }
    .term-key-scroll {
      display: flex; gap: 4px; flex: 1; min-width: 0; overflow-x: auto;
      overflow-y: hidden; overscroll-behavior-x: contain; touch-action: pan-x;
      -webkit-overflow-scrolling: touch; scrollbar-width: thin;
    }
    .term-keys button.armed { box-shadow: inset 0 0 0 1px #7aa2ff; }
    .term-screen .xterm { padding: 8px 10px; }
    .term-status { font-size: 11.5px; }
  }
`;

// src/main.ts
function mount(root, context) {
  if (typeof context.fill === "function") {
    context.fill();
    document.documentElement.style.height = "100%";
    document.body.style.height = "100%";
  }
  const style = document.createElement("style");
  style.textContent = styles + xterm_default;
  root.append(style);
  const box = document.createElement("div");
  box.className = "term";
  box.innerHTML = `
    <div class="term-bar">
      <span class="who">terminal</span>
      <div class="term-controls">
        <button type="button" data-signal="INT" data-session-control>INT</button>
        <button type="button" data-signal="TERM" data-session-control>TERM</button>
        <button type="button" data-clear data-session-control>Clear</button>
        <button type="button" data-reconnect>Reconnect</button>
      </div>
      <span class="state">connecting…</span>
    </div>
    <div class="term-screen" data-terminal aria-label="Terminal output"></div>
    <p class="term-status" role="status"></p>
    <div class="term-keys" data-keys aria-label="Terminal special keys">
      <div class="term-modifiers">
        <button type="button" data-keyboard aria-label="Show keyboard" aria-pressed="false" data-session-key>
          <svg viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"><rect x="1.5" y="4" width="13" height="8" rx="1.5"/><path d="M4 6.5h.01M6.5 6.5h.01M9 6.5h.01M11.5 6.5h.01M4 9h.01M11.5 9h.01M6 9h4"/></svg>
        </button>
        <button type="button" data-mod="ctrl" aria-pressed="false" data-session-key>Ctrl</button>
        <button type="button" data-mod="alt" aria-pressed="false" data-session-key>Alt</button>
      </div>
      <div class="term-key-scroll" data-key-scroll></div>
    </div>
    <div class="term-input" role="textbox" aria-label="Terminal input" aria-multiline="false" aria-disabled="true" contenteditable="false" autocapitalize="off" autocomplete="off" autocorrect="off" spellcheck="false" enterkeyhint="send" inputmode="text" data-form-type="other" data-lpignore="true" data-1p-ignore="true" data-bwignore></div>
  `;
  root.append(box);
  const screen = box.querySelector(".term-screen");
  const input = box.querySelector(".term-input");
  const status = box.querySelector(".term-status");
  const state = box.querySelector(".state");
  const who = box.querySelector(".who");
  const sessionControls = [
    ...box.querySelectorAll("[data-session-control]")
  ];
  const keyBar = box.querySelector(".term-keys");
  const keyScroll = box.querySelector("[data-key-scroll]");
  const ctrlButton = box.querySelector('[data-mod="ctrl"]');
  const altButton = box.querySelector('[data-mod="alt"]');
  const keyboardButton = box.querySelector("[data-keyboard]");
  let session = null;
  let streamAbort;
  let socket;
  let writeChain = Promise.resolve();
  let decoder = new TextDecoder;
  let disposed = false;
  let ctrlArmed = false;
  let altArmed = false;
  let composing = false;
  let lastSize = "";
  const term = new Dl({
    cols: 80,
    rows: 24,
    disableStdin: true,
    cursorBlink: true,
    cursorStyle: "block",
    scrollback: window.matchMedia("(max-width: 560px)").matches ? 2500 : 5000,
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
    fontSize: 13,
    theme: {
      background: "#080a0f",
      foreground: "#c8d0df",
      cursor: "#7aa2ff",
      cursorAccent: "#080a0f",
      selectionBackground: "#264f78",
      black: "#0b0e14",
      red: "#ff7d8a",
      green: "#6ee7b7",
      yellow: "#f7c948",
      blue: "#7aa2ff",
      magenta: "#c792ea",
      cyan: "#89ddff",
      white: "#e8ebf2",
      brightBlack: "#667087",
      brightRed: "#ff9aa5",
      brightGreen: "#9af3cf",
      brightYellow: "#ffe08a",
      brightBlue: "#a8c2ff",
      brightMagenta: "#ddb8ff",
      brightCyan: "#b6ecff",
      brightWhite: "#ffffff"
    }
  });
  const fitAddon = new o;
  term.loadAddon(fitAddon);
  term.open(screen);
  try {
    const webgl = new xr;
    webgl.onContextLoss(() => webgl.dispose());
    term.loadAddon(webgl);
  } catch {}
  term.textarea?.setAttribute("tabindex", "-1");
  term.textarea?.setAttribute("readonly", "readonly");
  const fitTerminal = () => {
    if (screen.clientWidth === 0 || screen.clientHeight === 0)
      return;
    try {
      fitAddon.fit();
    } catch {}
  };
  const updateModifiers = () => {
    for (const [button, armed] of [
      [ctrlButton, ctrlArmed],
      [altButton, altArmed]
    ]) {
      button.classList.toggle("armed", armed);
      button.setAttribute("aria-pressed", String(armed));
    }
  };
  const clearModifiers = () => {
    ctrlArmed = false;
    altArmed = false;
    updateModifiers();
  };
  const inputEnabled = () => input.getAttribute("contenteditable") !== "false";
  const setInputEnabled = (enabled) => {
    input.setAttribute("contenteditable", enabled ? "true" : "false");
    input.setAttribute("aria-disabled", String(!enabled));
    if (!enabled) {
      input.blur();
      input.textContent = "";
    }
  };
  const updateKeyboard = () => {
    const visible = document.activeElement === input;
    keyboardButton.classList.toggle("armed", visible);
    keyboardButton.setAttribute("aria-pressed", String(visible));
    const label = visible ? "Hide keyboard" : "Show keyboard";
    keyboardButton.title = label;
    keyboardButton.setAttribute("aria-label", label);
  };
  const focusInput = () => {
    if (inputEnabled())
      input.focus({ preventScroll: true });
  };
  const toggleKeyboard = () => {
    if (document.activeElement === input)
      input.blur();
    else
      focusInput();
    updateKeyboard();
  };
  const getFontSize = () => term.options.fontSize ?? 13;
  const setFontSize = (size) => {
    term.options.fontSize = Math.max(10, Math.min(20, Math.round(size * 10) / 10));
    fitTerminal();
  };
  const keyButton = (label, title) => {
    const node = document.createElement("button");
    node.type = "button";
    node.innerHTML = label;
    node.title = title;
    node.setAttribute("aria-label", title);
    node.dataset.sessionKey = "";
    return node;
  };
  const arrow = (path) => `<svg viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">${path}</svg>`;
  const keyButtons = [
    ["esc", "Esc", "Escape"],
    ["tab", "Tab", "Tab"],
    ["left", arrow('<path d="M12.5 8H3.5M6.5 5l-3 3 3 3"/>'), "Left arrow"],
    ["up", arrow('<path d="M8 12.5v-9M5 6.5l3-3 3 3"/>'), "Up arrow"],
    ["down", arrow('<path d="M8 3.5v9M5 9.5l3 3 3-3"/>'), "Down arrow"],
    ["right", arrow('<path d="M3.5 8h9M9.5 5l3 3-3 3"/>'), "Right arrow"],
    ["home", "Home", "Home"],
    ["end", "End", "End"],
    ["pgup", "PgUp", "Page up"],
    ["pgdn", "PgDn", "Page down"],
    ["ctrlc", "^C", "Interrupt"],
    ["ctrld", "^D", "End of input"],
    ["ctrll", "^L", "Clear screen"],
    ["ctrlz", "^Z", "Suspend"]
  ];
  for (const [key, label, title] of keyButtons) {
    const node = keyButton(label, title);
    node.dataset.key = key;
    keyScroll.append(node);
  }
  for (const symbol of BAR_SYMBOLS) {
    const node = keyButton(symbol, `Send ${symbol}`);
    node.dataset.symbol = symbol;
    keyScroll.append(node);
  }
  const enterKey = keyButton("Enter", "Send the current line");
  enterKey.dataset.enter = "true";
  keyScroll.append(enterKey);
  for (const [label, delta] of [
    ["A-", -1],
    ["A+", 1]
  ]) {
    const node = keyButton(label, delta < 0 ? "Smaller text" : "Larger text");
    delete node.dataset.sessionKey;
    node.dataset.font = String(delta);
    keyScroll.append(node);
  }
  const sessionKeys = [
    ...box.querySelectorAll("[data-session-key]")
  ];
  const setSessionControls = (enabled) => {
    for (const control of sessionControls)
      control.disabled = !enabled;
    for (const control of sessionKeys)
      control.disabled = !enabled;
    setInputEnabled(enabled);
    if (!enabled)
      clearModifiers();
    updateKeyboard();
  };
  setSessionControls(false);
  const setStatus = (message, bad = false) => {
    status.textContent = message ?? "";
    status.className = bad ? "term-status bad" : "term-status";
  };
  async function writeText(value) {
    if (!session)
      return;
    await call(context, "/write", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ id: session.id, data: encodeText(value) })
    });
  }
  const sendRaw = (value) => {
    if (!value)
      return;
    if (socket) {
      socket.send(new Uint8Array(encodeText(value)).buffer);
      return;
    }
    writeText(value).catch((error) => setStatus(String(error), true));
  };
  const sendTyped = (value) => {
    if (!value)
      return;
    let data = value;
    if (ctrlArmed || altArmed) {
      const characters = [...value];
      data = applyModifiers(characters[0], ctrlArmed, altArmed) + characters.slice(1).join("");
      clearModifiers();
    }
    sendRaw(data);
  };
  async function sendSignal(name) {
    if (!session)
      return;
    try {
      await call(context, "/signal", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ id: session.id, signal: name })
      });
      setStatus(`${name} sent to ${session.id}`);
    } catch (error) {
      setStatus(String(error), true);
    }
  }
  async function sendKey(key) {
    if (key === "ctrlc") {
      clearModifiers();
      await sendSignal("INT");
      focusInput();
      return;
    }
    const sequence = KEY_SEQUENCES[key];
    if (!sequence)
      return;
    sendRaw(sequence);
    clearModifiers();
    focusInput();
  }
  async function sendSymbol(symbol) {
    const data = applyModifiers(symbol, ctrlArmed, altArmed);
    sendRaw(data);
    clearModifiers();
    focusInput();
  }
  const sendResize = (cols, rows) => {
    if (!session)
      return;
    const key = `${cols}x${rows}`;
    if (key === lastSize)
      return;
    lastSize = key;
    call(context, "/resize", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ id: session.id, cols, rows })
    }).catch((error) => setStatus(String(error), true));
  };
  term.onResize(({ cols, rows }) => {
    sendResize(cols, rows);
  });
  term.onData((data) => {
    if (data)
      sendRaw(data);
  });
  term.textarea?.addEventListener("focus", () => {
    term.textarea?.blur();
    focusInput();
  });
  screen.addEventListener("focusin", (event) => {
    const target = event.target;
    if (target?.classList.contains("xterm-helper-textarea")) {
      target.blur();
      focusInput();
    }
  });
  const append = (bytes) => new Promise((resolve) => {
    const text = decoder.decode(bytes, { stream: true });
    if (!text) {
      resolve();
      return;
    }
    term.write(text, resolve);
  });
  const endSession = () => {
    const ended = session?.id;
    session = null;
    if (ended) {
      call(context, `/session?id=${encodeURIComponent(ended)}`, {
        method: "DELETE"
      }).catch(() => {});
    }
    setSessionControls(false);
    state.textContent = "ended";
    setStatus("Session ended. Reconnect to start a new one.", true);
    term.write(`\r
[session ended]\r
`);
  };
  const follow = async () => {
    if (disposed || !session)
      return;
    const controller = new AbortController;
    streamAbort = controller;
    try {
      const reader = await openStream(context, `/stream?id=${encodeURIComponent(session.id)}&seq=0`, controller.signal);
      for (;; ) {
        const { done, value } = await reader.read();
        if (disposed)
          return;
        if (done)
          break;
        if (value)
          await append(value);
      }
      if (disposed || controller.signal.aborted)
        return;
      endSession();
    } catch (error) {
      if (disposed || controller.signal.aborted)
        return;
      state.textContent = "degraded";
      setStatus(error instanceof Error ? error.message : String(error), true);
    } finally {
      if (streamAbort === controller)
        streamAbort = undefined;
    }
  };
  const connectSocket = () => {
    if (disposed || !session || !context.socket)
      return;
    const openSocket = context.socket;
    socket = openSocket(`/api/apps/${encodeURIComponent(context.app.id)}/socket?id=${encodeURIComponent(session.id)}&seq=0`, {
      onOpen: () => {
        if (!disposed)
          setStatus("Connected.");
      },
      onMessage: (data) => {
        writeChain = writeChain.then(() => append(new Uint8Array(data))).catch(() => {});
        return writeChain;
      },
      onText: (text) => {
        writeChain = writeChain.then(() => append(new TextEncoder().encode(text))).catch(() => {});
        return writeChain;
      },
      onClose: (error, code) => {
        socket = undefined;
        if (disposed)
          return;
        if (error || code !== undefined && code !== 1000 && code !== 1005) {
          state.textContent = "degraded";
          setStatus(error ?? `socket closed (${code})`, true);
          return;
        }
        endSession();
      }
    });
  };
  async function open(reset = false) {
    if (reset) {
      streamAbort?.abort();
      streamAbort = undefined;
      socket?.close();
      socket = undefined;
      const previous = session?.id;
      session = null;
      if (previous) {
        await call(context, `/session?id=${encodeURIComponent(previous)}`, {
          method: "DELETE"
        }).catch(() => {});
      }
      decoder = new TextDecoder;
      writeChain = Promise.resolve();
      term.reset();
    }
    setInputEnabled(false);
    setSessionControls(false);
    fitTerminal();
    const cols = Math.max(20, term.cols || 80);
    const rows = Math.max(5, term.rows || 24);
    lastSize = "";
    try {
      session = await call(context, "/open", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ shell: "/bin/sh", cols, rows })
      });
      if (disposed)
        return;
      lastSize = `${cols}x${rows}`;
      setInputEnabled(true);
      setSessionControls(true);
      state.textContent = `session ${session.id}`;
      who.textContent = `${session.owner} · ${session.shell}`;
      setStatus("Connected.");
      sendResize(term.cols, term.rows);
      if (context.socket)
        connectSocket();
      else
        follow();
      focusInput();
    } catch (error) {
      session = null;
      setSessionControls(false);
      state.textContent = "unavailable";
      who.textContent = "terminal";
      lastSize = "";
      setStatus(String(error), true);
    }
  }
  const consumeInput = () => {
    const text = input.textContent ?? "";
    input.textContent = "";
    if (text)
      sendTyped(text);
  };
  input.addEventListener("beforeinput", (event) => {
    if (composing)
      return;
    const type = event.inputType;
    if (type === "insertText" || type === "insertFromPaste" || type === "insertReplacementText") {
      const data = event.data;
      if (data) {
        event.preventDefault();
        input.textContent = "";
        sendTyped(data);
      }
      return;
    }
    if (type === "deleteContentBackward") {
      event.preventDefault();
      sendRaw("");
      return;
    }
    if (type === "deleteContentForward") {
      event.preventDefault();
      sendRaw("\x1B[3~");
      return;
    }
    if (type === "insertLineBreak" || type === "insertParagraph") {
      event.preventDefault();
      sendRaw("\r");
    }
  });
  input.addEventListener("input", () => {
    if (!composing)
      consumeInput();
  });
  input.addEventListener("compositionstart", () => {
    composing = true;
  });
  input.addEventListener("compositionend", () => {
    composing = false;
    consumeInput();
  });
  input.addEventListener("focus", () => {
    input.textContent = "";
    updateKeyboard();
  });
  input.addEventListener("blur", () => {
    input.textContent = "";
    updateKeyboard();
  });
  input.addEventListener("keydown", (event) => {
    const sequences = {
      Enter: "\r",
      Backspace: "",
      Delete: "\x1B[3~",
      Tab: "\t",
      Escape: "\x1B",
      ArrowUp: KEY_SEQUENCES.up,
      ArrowDown: KEY_SEQUENCES.down,
      ArrowLeft: KEY_SEQUENCES.left,
      ArrowRight: KEY_SEQUENCES.right,
      Home: KEY_SEQUENCES.home,
      End: KEY_SEQUENCES.end,
      PageUp: KEY_SEQUENCES.pgup,
      PageDown: KEY_SEQUENCES.pgdn
    };
    const sequence = sequences[event.key];
    if (sequence) {
      event.preventDefault();
      sendRaw(sequence);
      return;
    }
    if ((event.ctrlKey || event.altKey || ctrlArmed || altArmed) && event.key.length === 1 && !event.metaKey && !event.isComposing) {
      event.preventDefault();
      const ctrl = event.ctrlKey || ctrlArmed;
      const alt = event.altKey || altArmed;
      clearModifiers();
      sendRaw(applyModifiers(event.key, ctrl, alt));
    }
  });
  box.querySelector("[data-clear]").addEventListener("click", () => {
    term.write("\x1B[2J\x1B[3J\x1B[H");
    setStatus("Screen cleared");
  });
  for (const control of box.querySelectorAll("[data-signal]")) {
    control.addEventListener("click", () => {
      sendSignal(control.dataset.signal ?? "");
    });
  }
  keyBar.addEventListener("pointerdown", (event) => {
    if (event.target.closest("button"))
      event.preventDefault();
  });
  screen.addEventListener("pointerdown", (event) => {
    if (event.pointerType === "mouse")
      focusInput();
  });
  keyBar.addEventListener("click", (event) => {
    const target = event.target.closest("button");
    if (!target || target.disabled)
      return;
    if (target.hasAttribute("data-keyboard")) {
      toggleKeyboard();
      return;
    }
    if (target.dataset.enter === "true") {
      sendRaw("\r");
      focusInput();
      return;
    }
    if (target.dataset.mod === "ctrl") {
      ctrlArmed = !ctrlArmed;
      updateModifiers();
      focusInput();
      return;
    }
    if (target.dataset.mod === "alt") {
      altArmed = !altArmed;
      updateModifiers();
      focusInput();
      return;
    }
    const key = target.dataset.key;
    if (key) {
      sendKey(key);
      return;
    }
    const symbol = target.dataset.symbol;
    if (symbol) {
      sendSymbol(symbol);
      return;
    }
    const delta = Number(target.dataset.font);
    if (delta) {
      setFontSize(getFontSize() + delta);
      focusInput();
    }
  });
  const unbindGestures = bindTerminalGestures(screen, {
    sendKey: (key) => void sendKey(key),
    focusInput,
    getFontSize,
    setFontSize
  });
  const onViewport = () => {
    if (document.activeElement === input) {
      input.scrollIntoView({ block: "nearest" });
    }
  };
  window.visualViewport?.addEventListener("resize", onViewport);
  const onResize = () => {
    fitTerminal();
    updateKeyboard();
  };
  window.addEventListener("resize", onResize);
  box.querySelector("[data-reconnect]").addEventListener("click", () => {
    open(true);
  });
  updateModifiers();
  requestAnimationFrame(fitTerminal);
  open();
  return () => {
    disposed = true;
    streamAbort?.abort();
    streamAbort = undefined;
    socket?.close();
    socket = undefined;
    unbindGestures();
    window.visualViewport?.removeEventListener("resize", onViewport);
    window.removeEventListener("resize", onResize);
    if (session) {
      call(context, `/session?id=${encodeURIComponent(session.id)}`, {
        method: "DELETE"
      }).catch(() => {});
    }
    term.dispose();
    root.replaceChildren();
  };
}
export {
  mount
};
