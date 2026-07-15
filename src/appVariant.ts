export type AppVariant = 'standard' | 'weekly';

export function resolveAppVariant(value: unknown): AppVariant {
  return value === 'weekly' ? 'weekly' : 'standard';
}

export const APP_VARIANT = resolveAppVariant(import.meta.env.VITE_ORBIT_VARIANT);
