export * from './rustev_wasm';
export { Engine } from './rustev_wasm';
export class RustevError extends Error {
  readonly code: string;
  readonly detail: unknown;
  constructor(code: string, detail?: unknown, message?: string);
}
export class InvalidInput extends RustevError {}
