const PERFORMANCE_LOGGING = true;

function logPerformance(label, fn) {
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
