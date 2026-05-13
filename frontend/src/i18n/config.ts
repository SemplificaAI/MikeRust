// Single source of truth for the i18n configuration. Imported by both the
// next-intl request handler (server) and the client-side language switcher
// so the locale list and default stay in sync across the app.

export const locales = ["it", "en", "fr", "de", "es", "pt"] as const;
export type Locale = (typeof locales)[number];

export const defaultLocale: Locale = "it";

// English is the universal fallback: any key missing from the active
// locale catalogue is resolved from `en.json` before next-intl falls
// back to printing the raw key. Keep in sync with `request.ts`.
export const FALLBACK_LOCALE: Locale = "en";

// Cookie name where the user's language preference is persisted. Read by the
// server in `request.ts` (via `cookies()`) and written by the client when the
// user picks a language from the switcher in the account page.
export const LOCALE_COOKIE = "mike_locale";

export function isLocale(value: unknown): value is Locale {
    return (
        typeof value === "string" && (locales as readonly string[]).includes(value)
    );
}
