export function initializeWasm(): Promise<any>  {
  return import('./wasm/pkg').then(wasm => {
    (window as any)._WritingWasm = wasm;
  });
};

export function importWasm(): any {
  return (window as any)._WritingWasm;
}
