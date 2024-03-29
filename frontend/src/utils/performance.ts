const PERFORMANCE_LOGGING = false;

function logPerformance(label: string, fn: () => any) : any {
  if (!PERFORMANCE_LOGGING) {
    return fn();
  } else {
    const startedAt = performance.now();
    const ret = fn();
    const duration = performance.now() - startedAt;
    console.log(`[performance] [${label}] ${duration} ms`);
    return ret;
  }
}

export {
  logPerformance,
}
