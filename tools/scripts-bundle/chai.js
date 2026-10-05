import * as chai from 'chai';

// Assertions ajoutées par Bruno à chai : `.json`, `.jsonSchema()` et `.jsonBody()` (parité Postman).
const { Assertion } = chai;

const SCHEMA_VERSIONS = ['http://json-schema.org/draft-07/schema#', 'http://json-schema.org/draft-07/schema'];

Assertion.addProperty('json', function () {
  const obj = this._obj;
  const isJson =
    typeof obj === 'object' && obj !== null && (Array.isArray(obj) || Object.prototype.toString.call(obj) === '[object Object]');
  this.assert(isJson, 'expected #{this} to be JSON', 'expected #{this} not to be JSON');
});

Assertion.addMethod('jsonSchema', function (schema, ajvOptions) {
  if (schema && schema.$schema && !SCHEMA_VERSIONS.includes(schema.$schema)) {
    this.assert(
      false,
      `Unsupported JSON Schema version: "${schema.$schema}". Bruno currently only supports Draft-07 (http://json-schema.org/draft-07/schema#). Please update your schema to be Draft-07 compatible and remove the $schema property.`,
      `Unsupported JSON Schema version: "${schema.$schema}".`,
    );
  }
  const { Ajv, addFormats } = globalThis.__ajv();
  const ajv = new Ajv({ allErrors: true, ...(ajvOptions || {}) });
  addFormats(ajv);
  let validate;
  try {
    validate = ajv.compile(schema);
  } catch (e) {
    this.assert(false, `JSON schema compile error: ${e.message}`, `JSON schema compile error: ${e.message}`);
  }
  const data = this._obj;
  const isValid = validate(data);
  let shown;
  try {
    shown = JSON.stringify(data);
  } catch {
    shown = '[unserializable value]';
  }
  this.assert(
    isValid,
    `expected ${shown} to match JSON schema, validation errors: ${validate.errors ? JSON.stringify(validate.errors) : 'none'}`,
    `expected ${shown} to not match JSON schema`,
  );
});

// `a.b`, `a[0]`, `a["b.c"]`, `a['k']` -> clés
const parsePath = (path) => {
  const keys = [];
  let i = 0;
  while (i < path.length) {
    if (path[i] === '.') {
      i++;
    } else if (path[i] === '[') {
      i++;
      let key = '';
      if (i < path.length && (path[i] === "'" || path[i] === '"')) {
        const quote = path[i++];
        while (i < path.length && path[i] !== quote) {
          if (path[i] === '\\' && i + 1 < path.length && path[i + 1] === quote) {
            key += quote;
            i += 2;
          } else {
            key += path[i++];
          }
        }
        i += 2;
      } else {
        while (i < path.length && path[i] !== ']') key += path[i++];
        i++;
      }
      keys.push(key);
    } else {
      let key = '';
      while (i < path.length && path[i] !== '.' && path[i] !== '[') key += path[i++];
      keys.push(key);
    }
  }
  return keys;
};

const nested = (obj, path) => {
  let current = obj;
  for (const key of parsePath(path)) {
    if (current === null || current === undefined || !Object.prototype.hasOwnProperty.call(Object(current), key)) {
      return { found: false };
    }
    current = current[key];
  }
  return { found: true, value: current };
};

const deepEqual = (a, b) => {
  if (a === b) return true;
  if (a === null || b === null || typeof a !== 'object' || typeof b !== 'object') return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  const keys = Object.keys(a);
  if (keys.length !== Object.keys(b).length) return false;
  return keys.every((k) => Object.prototype.hasOwnProperty.call(b, k) && deepEqual(a[k], b[k]));
};

Assertion.addMethod('jsonBody', function (...args) {
  const obj = this._obj;
  if (args.length === 0) {
    this.assert(
      typeof obj === 'object' && obj !== null,
      'expected value to be a JSON body (object or array)',
      'expected value not to be a JSON body',
    );
  } else if (args.length === 1 && typeof args[0] === 'object' && args[0] !== null) {
    this.assert(deepEqual(obj, args[0]), 'expected body to deeply equal given object', 'expected body to not deeply equal given object');
  } else if (args.length === 1) {
    const { found } = nested(obj, String(args[0]));
    this.assert(
      found,
      `expected body to have nested property '${args[0]}'`,
      `expected body to not have nested property '${args[0]}'`,
    );
  } else {
    const { found, value } = nested(obj, String(args[0]));
    this.assert(
      found && deepEqual(value, args[1]),
      `expected body to have nested property '${args[0]}' equal to given value`,
      `expected body to not have nested property '${args[0]}' equal to given value`,
    );
  }
});

globalThis.__lib = { expect: chai.expect, assert: chai.assert };
