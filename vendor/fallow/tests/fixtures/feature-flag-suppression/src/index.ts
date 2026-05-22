// fallow-ignore-next-line feature-flag
const suppressed = process.env.FEATURE_DARK_MODE;

const unsuppressed = process.env.FEATURE_NEW_CHECKOUT;

export function boot() {
  if (suppressed) console.log("dark mode");
  if (unsuppressed) console.log("new checkout");
}
