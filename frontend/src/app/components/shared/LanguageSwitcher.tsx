"use client";

import { useTransition } from "react";
import { useLocale, useTranslations } from "next-intl";
import { useRouter } from "next/navigation";
import { setLocaleCookie } from "@/i18n/actions";
import { locales, type Locale } from "@/i18n/config";
import { apiBase } from "@/lib/apiBase";

// Persist the choice on the backend (data\storage\, via /user/locale) so it
// follows the data folder, then mirror to the cookie that next-intl uses
// for SSR. Backend write is best-effort: if the user is offline or the
// session is expired, the cookie still works for the local session.
async function persistLocaleOnBackend(locale: Locale) {
    if (typeof window === "undefined") return;
    const token = localStorage.getItem("mike_auth_token");
    if (!token) return;
    try {
        await fetch(`${apiBase()}/user/locale`, {
            method: "PUT",
            headers: {
                "Content-Type": "application/json",
                Authorization: `Bearer ${token}`,
            },
            body: JSON.stringify({ locale }),
        });
    } catch {
        // Backend offline; cookie is still set so the local session reflects
        // the choice. The next successful login will re-sync.
    }
}

// Language picker. Persists the choice on the backend (primary, portable
// store) and in a cookie used by next-intl SSR, then refreshes the route
// so the next render reads from the new catalog. Used in the Account page;
// can be reused elsewhere if needed.
export function LanguageSwitcher() {
    const t = useTranslations("Account");
    const locale = useLocale() as Locale;
    const router = useRouter();
    const [pending, startTransition] = useTransition();

    const handleChange = (next: Locale) => {
        if (next === locale || pending) return;
        startTransition(async () => {
            await persistLocaleOnBackend(next);
            await setLocaleCookie(next);
            router.refresh();
        });
    };

    const labels: Record<Locale, string> = {
        it: t("languageItalian"),
        en: t("languageEnglish"),
        fr: t("languageFrench"),
        de: t("languageGerman"),
        es: t("languageSpanish"),
        pt: t("languagePortuguese"),
    };

    // Label sits above the pill row (rather than next to it) so the
    // six-language list has the full panel width to wrap into when the
    // viewport is narrow. Each language is an individually-rounded
    // pill so wrapping to a second line still looks clean — the old
    // fused segmented-control look stopped working as soon as we went
    // past four languages.
    return (
        <div className="flex flex-col gap-2">
            <span className="text-sm font-medium">{t("language")}</span>
            <div className="flex flex-wrap gap-1.5">
                {locales.map((loc) => (
                    <button
                        key={loc}
                        type="button"
                        disabled={pending}
                        onClick={() => handleChange(loc)}
                        className={
                            "px-3 py-1.5 text-sm rounded-md border transition " +
                            (loc === locale
                                ? "bg-gray-900 text-white border-gray-900"
                                : "bg-white text-gray-700 border-gray-200 hover:bg-gray-50 hover:border-gray-400") +
                            (pending ? " opacity-60 cursor-not-allowed" : "")
                        }
                        aria-pressed={loc === locale}
                    >
                        {labels[loc]}
                    </button>
                ))}
            </div>
        </div>
    );
}
