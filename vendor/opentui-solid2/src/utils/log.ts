export const log = (...args: unknown[]) => {
  if (process.env.DEBUG) {
    console.log("[Reconciler]", ...args);
  }
};
