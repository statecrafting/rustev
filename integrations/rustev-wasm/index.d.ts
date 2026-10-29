export * from './rustev_wasm.js';
export { Engine } from './rustev_wasm.js';
export class RustevError extends Error {
  readonly code: string;
  readonly detail: unknown;
  constructor(code: string, detail?: unknown, message?: string);
}
export class InvalidInput extends RustevError {}
