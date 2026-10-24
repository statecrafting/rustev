// Spec 027 3.4.4: turn the glue's explanation refusals into public classes.
// This self-contained factory is emitted for both ESM and CommonJS targets.
export function wrapBindings(glue) {
  class RustevError extends Error {
    constructor(code, detail, message) {
      super(message ?? `${code}${detail === undefined || detail === null ? '' : `: ${JSON.stringify(detail)}`}`);
      this.name = new.target.name;
      this.code = code;
      this.detail = detail;
    }
  }
  class InvalidInput extends RustevError {}
  class Engine extends glue.Engine {
    async explain(evidence, query, options = {}) {
      const invalid = (detail) => new InvalidInput('invalid-options', { detail });
      const inputs = new glue.Inputs();
      let result;
      try {
        const put = (name, value) => {
          if (!(value instanceof Uint8Array)) throw invalid('evidence and redaction must be bytes');
          inputs.add(name, value);
          return name;
        };
        const object = (value) => {
          if (!value || typeof value !== 'object' || Array.isArray(value)) throw invalid('expected an object');
          return value;
        };
        const pack = (value, prefix) => Object.fromEntries(
          Object.entries(object(value)).map(([k, v]) => [k, put(`${prefix}.${k}`, v)]),
        );
        const packed = pack(evidence, 'a');
        const q = query && typeof query === 'object' && Object.hasOwn(query, 'diff')
          ? { ...query, diff: { ...query.diff, other: pack(query.diff?.other, 'b') } } : query;
        const { resolve, ...opts } = object(options);
        if (Object.hasOwn(options, 'resolve') && typeof resolve !== 'function') throw invalid('resolve must be a function');
        for (const key of Object.keys(opts)) if (opts[key] === undefined) opts[key] = null;
        if (Object.hasOwn(opts, 'redaction')) opts.redaction = put('redaction', opts.redaction);
        let args;
        try {
          args = JSON.stringify({ evidence: packed, query: q, options: opts });
        } catch {
          throw invalid('expected JSON');
        }
        result = super.explain(inputs, args, resolve);
        const decoder = new TextDecoder();
        if (result.code !== 0) {
          const error = JSON.parse(decoder.decode(result.out));
          throw new InvalidInput(error.status, error.detail);
        }
        return { explanation: result.artifact('explanation'), markdown: decoder.decode(result.artifact('markdown')) };
      } finally {
        result?.free();
        inputs.free();
      }
    }
  }
  return { Engine, RustevError, InvalidInput };
}
