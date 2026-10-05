// API de scripts compatible Bruno (`bru`, `req`, `res`, `test`, `console`, `require`) pour le sandbox QuickJS.
// Les variables vivent côté Rust (fonctions de `__h`) ; la requête et la réponse vivent ici et ressortent par `collect`.
(function () {
  'use strict';

  const h = globalThis.__h;
  const init = JSON.parse(globalThis.__init);
  delete globalThis.__h;
  delete globalThis.__init;

  const NAME = /^[\w-.]*$/;
  const state = {
    request: init.request,
    response: init.response,
    results: [],
    nextRequest: undefined,
    skipRequest: false,
    stopExecution: false,
    headersToDelete: [],
    maxRedirects: undefined,
    disableJsonParsing: false,
    bodyChanged: false,
  };

  const clone = (v) => (v === undefined ? undefined : JSON.parse(JSON.stringify(v)));
  const enc = (v) => JSON.stringify(v === undefined ? null : v);
  const dec = (s) => (s === undefined ? undefined : JSON.parse(s));

  // ---------- interpolation et variables ----------

  const interpolate = (x) => {
    if (!x) return x;
    if (typeof x === 'object') return JSON.parse(h.interpolate(JSON.stringify(x)));
    return typeof x === 'string' ? h.interpolate(x) : x;
  };

  const checkName = (key) => {
    if (!NAME.test(key)) {
      throw new Error(
        `Variable name: "${key}" contains invalid characters! Names must only contain alpha-numeric characters, "-", "_", "."`,
      );
    }
  };

  const raw = (scope, key) => dec(h.get(scope, key));

  const writable = (scope, noun) => ({
    has: (key) => h.has(scope, key),
    get: (key) => interpolate(raw(scope, key)),
    set: (key, value) => {
      if (!key) throw new Error(`Creating ${noun} without specifying a name is not allowed.`);
      checkName(key);
      h.set(scope, key, enc(value));
    },
    delete: (key) => h.remove(scope, key),
    deleteAll: () => h.clear(scope),
    all: () => JSON.parse(h.all(scope)),
  });

  const env = writable('env', 'a env variable');
  const runtime = writable('runtime', 'a variable');
  const global = writable('global', 'a env variable');
  const collection = writable('collection', 'a variable');
  const withoutName = (map) => {
    delete map.__name__;
    return map;
  };

  // ---------- bru ----------

  const results = () => ({
    summary: {
      total: state.results.length,
      passed: state.results.filter((r) => r.status === 'pass').length,
      failed: state.results.filter((r) => r.status === 'fail').length,
      skipped: state.results.filter((r) => r.status !== 'pass' && r.status !== 'fail').length,
    },
    results: clone(state.results),
  });

  const sleep = async (ms) => {
    h.sleep(Number(ms) || 0);
    return 'slept';
  };

  const bru = {
    getEnvName: () => raw('env', '__name__'),
    hasEnvVar: env.has,
    getEnvVar: env.get,
    setEnvVar: (key, value) => env.set(key, value),
    deleteEnvVar: (key) => {
      if (key !== '__name__') env.delete(key);
    },
    getAllEnvVars: () => withoutName(env.all()),
    deleteAllEnvVars: env.deleteAll,

    hasGlobalEnvVar: global.has,
    getGlobalEnvVar: global.get,
    setGlobalEnvVar: (key, value) => global.set(key, value),
    deleteGlobalEnvVar: global.delete,
    getAllGlobalEnvVars: global.all,
    deleteAllGlobalEnvVars: global.deleteAll,

    hasVar: runtime.has,
    getVar: (key) => {
      checkName(key);
      return runtime.get(key);
    },
    setVar: (key, value) => runtime.set(key, value),
    deleteVar: runtime.delete,
    deleteAllVars: runtime.deleteAll,
    getAllVars: runtime.all,

    hasCollectionVar: collection.has,
    getCollectionVar: collection.get,
    setCollectionVar: (key, value) => collection.set(key, value),
    deleteCollectionVar: collection.delete,
    deleteAllCollectionVars: collection.deleteAll,
    getAllCollectionVars: collection.all,

    getFolderVar: (key) => interpolate(raw('folder', key)),
    getRequestVar: (key) => interpolate(raw('request', key)),
    getProcessEnv: (key) => raw('process', key),
    getOauth2CredentialVar: (key) => interpolate(raw('oauth2', key)),

    interpolate,
    setNextRequest: (name) => {
      state.nextRequest = name;
    },
    runner: {
      skipRequest: () => {
        state.skipRequest = true;
      },
      stopExecution: () => {
        state.stopExecution = true;
      },
      setNextRequest: (name) => {
        state.nextRequest = name;
      },
    },
    sleep,
    cwd: () => init.meta.collectionPath,
    getCollectionName: () => init.meta.collectionName,
    isSafeMode: () => true,
    getTestResults: async () => results(),
    getAssertionResults: async () => ({
      summary: { total: 0, passed: 0, failed: 0, skipped: 0 },
      results: [],
    }),
  };

  // ---------- req ----------

  const isJsonType = (headers) => {
    const type = headers['Content-Type'] || headers['content-type'];
    return typeof type === 'string' && type.includes('json');
  };

  const parseJsonOr = (text) => {
    try {
      return JSON.parse(text);
    } catch {
      return text;
    }
  };

  const splitUrl = (url) => {
    const withoutScheme = String(url || '').replace(/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//, '');
    const rest = withoutScheme.split('#')[0];
    const [beforeQuery, ...query] = rest.split('?');
    const slash = beforeQuery.indexOf('/');
    const authority = slash < 0 ? beforeQuery : beforeQuery.slice(0, slash);
    return {
      host: authority.slice(authority.lastIndexOf('@') + 1),
      path: slash < 0 ? '' : beforeQuery.slice(slash),
      query: query.join('?'),
    };
  };

  const request = state.request;
  const req = {
    url: request.url,
    method: request.method,
    headers: clone(request.headers),
    timeout: request.timeout,
    name: request.name,
    tags: clone(request.tags),
    pathParams: clone(request.pathParams),
    body: isJsonType(request.headers) && typeof request.data === 'string' ? parseJsonOr(request.data) : undefined,

    getUrl: () => request.url,
    setUrl: (url) => {
      request.url = url;
    },
    getHost: () => splitUrl(request.url).host,
    getPath: () => {
      let { path } = splitUrl(request.url);
      for (const p of request.pathParams || []) {
        if (p.type === 'path' && p.value) path = path.replace(`:${p.name}`, p.value);
      }
      return path;
    },
    getQueryString: () => splitUrl(request.url).query,
    getMethod: () => request.method,
    setMethod: (method) => {
      request.method = method;
    },
    getHeaders: () => clone(request.headers),
    setHeaders: (headers) => {
      request.headers = clone(headers) || {};
    },
    getHeader: (name) => request.headers[name],
    setHeader: (name, value) => {
      request.headers[name] = value;
    },
    deleteHeader: (name) => {
      delete request.headers[name];
      state.headersToDelete.push(name);
    },
    deleteHeaders: (names) => {
      for (const name of names) req.deleteHeader(name);
    },
    getBody: (options) => {
      if (options && options.raw) return clone(request.data);
      if (isJsonType(request.headers) && typeof request.data === 'string') return parseJsonOr(request.data);
      return clone(request.data);
    },
    setBody: (data, options) => {
      const structured = data !== null && typeof data === 'object';
      request.data = !(options && options.raw) && isJsonType(request.headers) && structured ? JSON.stringify(data) : data;
    },
    getName: () => request.name,
    getTags: () => clone(request.tags),
    getPathParams: () => clone(request.pathParams),
    getAuthMode: () => request.authMode || 'none',
    getTimeout: () => request.timeout,
    setTimeout: (ms) => {
      request.timeout = ms;
    },
    setMaxRedirects: (n) => {
      state.maxRedirects = n;
    },
    disableParsingResponseJson: () => {
      state.disableJsonParsing = true;
    },
    getExecutionMode: () => init.meta.executionMode,
  };

  // ---------- res ----------

  // Port de `get` de @usebruno/query : navigation, `..` profond, `[n]`, filtres `[?]`.
  const normalize = (value) => {
    if (!Array.isArray(value)) return value;
    const values = [];
    value.forEach((item) => {
      const v = normalize(item);
      if (v != null) values.push(...(Array.isArray(v) ? v : [v]));
    });
    return values.length ? values : undefined;
  };

  const getValue = (source, prop, deep) => {
    if (typeof source !== 'object') return undefined;
    let value;
    if (Array.isArray(source)) {
      value = source.map((item) => getValue(item, prop, deep));
    } else {
      value = source[prop];
      if (deep) {
        value = [value];
        for (const [key, item] of Object.entries(source)) {
          if (key !== prop && typeof item === 'object') value.push(getValue(source[key], prop, deep));
        }
      }
    }
    return normalize(value);
  };

  const objectPredicate = (obj) => (item) => {
    for (const [key, value] of Object.entries(obj)) {
      if (item[key] !== value) return false;
    }
    return true;
  };

  const filterOrMap = (source, funOrObj) => {
    const fun = typeof funOrObj === 'object' ? objectPredicate(funOrObj) : funOrObj;
    const isArray = Array.isArray(source);
    const result = [];
    for (const item of isArray ? source : [source]) {
      if (item == null) continue;
      const value = fun(item);
      if (value === true) result.push(item);
      else if (value != null && value !== false) result.push(value);
    }
    return normalize(isArray ? result : result[0]);
  };

  const query = (source, path, ...fns) => {
    const tokens = path
      .replace(/\s+/g, '')
      .split(/(\.{1,2}|\[\?\]|\[\d+\])/g)
      .filter((s) => s.length > 0)
      .map((str) => {
        const text = str.replace(/\[|\]/g, '');
        const index = parseInt(text);
        return isNaN(index) ? text : index;
      });
    let index = 0;
    let lookbehind = '';
    let funIndex = 0;
    while (source != null && index < tokens.length) {
      const token = tokens[index++];
      if (token === '..' || token === '.') {
        // séparateur
      } else if (token === '?') {
        const fun = fns[funIndex++];
        if (fun == null) throw new Error(`missing function for ${lookbehind}`);
        source = filterOrMap(source, fun);
      } else if (typeof token === 'number') {
        source = normalize(source[token]);
      } else {
        source = getValue(source, token, lookbehind === '..');
      }
      lookbehind = token;
    }
    return source;
  };

  const sizeOf = () => {
    const r = state.response;
    return r.size || { header: 0, body: 0, total: 0 };
  };

  let res;
  if (state.response) {
    const response = state.response;
    res = (expr, ...fns) => query(response.data, expr, ...fns);
    Object.assign(res, {
      status: response.status,
      statusText: response.statusText,
      headers: clone(response.headers),
      body: clone(response.data),
      responseTime: response.responseTime,
      url: response.url,
      getStatus: () => response.status,
      getStatusText: () => response.statusText,
      getHeader: (name) => (typeof name === 'string' ? response.headers[name.toLowerCase()] : null),
      getHeaders: () => clone(response.headers),
      getBody: () => clone(response.data),
      getResponseTime: () => response.responseTime,
      getUrl: () => response.url,
      getSize: sizeOf,
      setBody: (data) => {
        response.data = clone(data);
        state.bodyChanged = true;
      },
    });
  }

  // ---------- console ----------

  const plain = (arg) => {
    if (typeof arg === 'function') return `function ${arg.name || 'anonymous'}() {\n    [native code]\n}`;
    if (arg === undefined) return null;
    if (arg === null || typeof arg !== 'object') return arg;
    const kind = arg.constructor && arg.constructor.name;
    if (kind === 'Date' || kind === 'RegExp' || (kind && kind.endsWith('Error'))) return String(arg);
    if (kind === 'Set' || kind === 'Map') return { __brunoType: kind, __brunoValue: Array.from(arg) };
    return arg;
  };

  const logger = (level) => (...args) => {
    const seen = new WeakSet();
    const circular = (_key, value) => {
      if (value !== null && typeof value === 'object') {
        if (seen.has(value)) return '[Circular Reference]';
        seen.add(value);
      }
      return value;
    };
    h.log(level, JSON.stringify(args.map(plain), circular));
  };

  const consoleShim = {
    log: logger('log'),
    debug: logger('debug'),
    info: logger('info'),
    warn: logger('warn'),
    error: logger('error'),
  };

  // ---------- test, expect, assert, require ----------

  const loaded = {};
  const lib = (name) => {
    if (!(name in loaded)) loaded[name] = h.lib(name);
    return loaded[name];
  };

  const addResult = (result) => state.results.push(result);

  const test = async (description, callback) => {
    try {
      await callback();
      addResult({ description, status: 'pass' });
    } catch (error) {
      const failure = { description, status: 'fail', error: (error && error.message) || 'An unexpected error occurred.' };
      if (error && error.name === 'AssertionError' && 'actual' in error) {
        failure.actual = error.actual;
        failure.expected = error.expected;
      }
      addResult(failure);
    }
  };

  // ---------- bibliothèques et require ----------

  const hex = (bytes) => bytes.match(/../g).map((byte) => parseInt(byte, 16));
  const cryptoShim = {
    randomBytes: (size) => {
      const bytes = Uint8Array.from(hex(h.random(size)));
      return lib('buffer').Buffer.from(bytes);
    },
    getRandomValues: (array) => {
      const bytes = hex(h.random(array.byteLength));
      new Uint8Array(array.buffer, array.byteOffset, array.byteLength).set(bytes);
      return array;
    },
  };

  const BASE64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  const btoa = (input) => {
    const text = String(input);
    let out = '';
    for (let i = 0; i < text.length; i += 3) {
      const [a, b, c] = [text.charCodeAt(i), text.charCodeAt(i + 1), text.charCodeAt(i + 2)];
      if (a > 255 || b > 255 || c > 255) throw new Error('Invalid character');
      const n = (a << 16) | ((b || 0) << 8) | (c || 0);
      out += BASE64[(n >> 18) & 63] + BASE64[(n >> 12) & 63];
      out += i + 1 < text.length ? BASE64[(n >> 6) & 63] : '=';
      out += i + 2 < text.length ? BASE64[n & 63] : '=';
    }
    return out;
  };
  const atob = (input) => {
    const text = String(input).replace(/[\t\n\f\r ]+/g, '').replace(/=+$/, '');
    if (text.length % 4 === 1 || /[^A-Za-z0-9+/]/.test(text)) throw new Error('The string to be decoded is not correctly encoded.');
    let out = '';
    let bits = 0;
    let acc = 0;
    for (const ch of text) {
      acc = (acc << 6) | BASE64.indexOf(ch);
      bits += 6;
      if (bits >= 8) {
        bits -= 8;
        out += String.fromCharCode((acc >> bits) & 255);
      }
    }
    return out;
  };

  const path = { resolve: (...parts) => h.resolve(JSON.stringify(parts)) };

  const modules = {
    chai: () => lib('chai'),
    moment: () => lib('moment'),
    buffer: () => lib('buffer'),
    btoa: () => btoa,
    atob: () => atob,
    'crypto-js': () => lib('crypto-js'),
    tv4: () => lib('tv4'),
    ajv: () => lib('ajv').Ajv,
    'ajv-formats': () => lib('ajv').addFormats,
    nanoid: () => lib('nanoid'),
    uuid: () => lib('uuid'),
    path: () => path,
  };

  const isLocal = (mod) => typeof mod === 'string' && (mod.startsWith('.') || mod.startsWith(init.meta.collectionPath));
  const localModules = {};

  const requireShim = (mod) => {
    if (Object.prototype.hasOwnProperty.call(modules, mod)) return modules[mod]();
    if (!isLocal(mod)) throw new Error(`Cannot find module ${mod}`);
    if (!Object.prototype.hasOwnProperty.call(localModules, mod)) {
      const module = { exports: {} };
      const nested = (sub) => requireShim(isLocal(sub) ? path.resolve(init.meta.collectionPath, mod, '..', sub) : sub);
      new Function('module', 'exports', 'require', h.module(mod))(module, module.exports, nested);
      localModules[mod] = module.exports;
    }
    return localModules[mod];
  };

  const define = (name, value) =>
    Object.defineProperty(globalThis, name, { value, writable: true, configurable: true, enumerable: false });
  const lazy = (name, make) =>
    Object.defineProperty(globalThis, name, {
      get: () => {
        const value = make();
        define(name, value);
        return value;
      },
      set: (value) => define(name, value),
      configurable: true,
      enumerable: false,
    });

  define('bru', bru);
  define('req', req);
  if (res) define('res', res);
  define('test', test);
  define('console', consoleShim);
  define('require', requireShim);
  define('crypto', cryptoShim);
  define('btoa', btoa);
  define('atob', atob);
  define('path', path);
  lazy('moment', () => lib('moment'));
  lazy('Buffer', () => lib('buffer').Buffer);
  lazy('nanoid', () => lib('nanoid'));
  lazy('uuid', () => lib('uuid'));
  lazy('tv4', () => lib('tv4'));
  lazy('Ajv', () => lib('ajv').Ajv);
  lazy('addFormats', () => lib('ajv').addFormats);
  lazy('expect', () => lib('chai').expect);
  lazy('assert', () => lib('chai').assert);
  Object.defineProperty(globalThis, '__ajv', { value: () => lib('ajv'), enumerable: false });

  return {
    collect: () =>
      JSON.stringify({
        request: {
          url: request.url,
          method: request.method,
          headers: request.headers,
          data: request.data === undefined ? null : request.data,
          timeout: request.timeout === undefined ? null : request.timeout,
          maxRedirects: state.maxRedirects === undefined ? null : state.maxRedirects,
          headersToDelete: state.headersToDelete,
          disableJsonParsing: state.disableJsonParsing,
        },
        response: state.response && state.bodyChanged ? { data: state.response.data } : null,
        results: state.results,
        nextRequest: state.nextRequest === undefined ? { unset: true } : { name: state.nextRequest },
        skipRequest: state.skipRequest,
        stopExecution: state.stopExecution,
      }),
  };
})()
