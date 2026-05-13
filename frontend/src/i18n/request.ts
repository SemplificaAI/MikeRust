import { cookies } from "next/headers";
import { getRequestConfig } from "next-intl/server";
import {
    defaultLocale,
    FALLBACK_LOCALE,
    isLocale,
    LOCALE_COOKIE,
} from "./config";

// Deep-merge two message catalogues so the active locale wins where it
// has a key and the fallback locale fills in the gaps. Used to give every
// non-English catalogue an English safety net for any string that hasn't
// been translated yet — otherwise next-intl would render the literal key
// (`Models.previewGlobalOnly`) in the UI.
function deepMerge<T>(base: T, override: T): T {
    if (
        typeof base !== "object" ||
        base === null ||
        Array.isArray(base) ||
        typeof override !== "object" ||
        override === null ||
        Array.isArray(override)
    ) {
        return override ?? base;
    }
    const out: Record<string, unknown> = { ...(base as Record<string, unknown>) };
    for (const [k, v] of Object.entries(override as Record<string, unknown>)) {
        const existing = (base as Record<string, unknown>)[k];
        if (
            typeof v === "object" &&
            v !== null &&
            !Array.isArray(v) &&
            typeof existing === "object" &&
            existing !== null &&
            !Array.isArray(existing)
        ) {
            out[k] = deepMerge(existing, v);
        } else {
            out[k] = v;
        }
    }
    return out as T;
}

// next-intl entry point: invoked once per request to resolve the active
// locale and load the corresponding messages. We don't use locale-prefixed
// routes (the app's URL structure stays as-is); instead the user's choice
// is persisted in a cookie and read here.
export default getRequestConfig(async () => {
    const cookieStore = await cookies();
    const cookieLocale = cookieStore.get(LOCALE_COOKIE)?.value;
    const locale = isLocale(cookieLocale) ? cookieLocale : defaultLocale;

    // Always load the fallback (English) catalogue. When the active
    // locale is English we skip the merge entirely.
    const fallbackMessages = (await import(`../../messages/${FALLBACK_LOCALE}.json`)).default;
    const activeMessages =
        locale === FALLBACK_LOCALE
            ? fallbackMessages
            : (await import(`../../messages/${locale}.json`)).default;

    const messages =
        locale === FALLBACK_LOCALE
            ? activeMessages
            : deepMerge(fallbackMessages, activeMessages);

    return {
        locale,
        messages,
    };
});
