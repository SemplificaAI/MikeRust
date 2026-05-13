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

    return (
        <div className="flex items-center gap-2">
            <span className="text-sm font-medium">{t("language")}</span>
            <div className="inline-flex rounded-md border border-gray-200 overflow-hidden">
                {locales.map((loc) => (
                    <button
                        key={loc}
                        type="button"
                        disabled={pending}
                        onClick={() => handleChange(loc)}
                        className={
                            "px-3 py-1.5 text-sm transition " +
                            (loc === locale
                                ? "bg-gray-900 text-white"
                                : "bg-white text-gray-700 hover:bg-gray-50") +
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
