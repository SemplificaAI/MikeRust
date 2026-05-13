"use client";

import { useEffect, useMemo, useState } from "react";
import { usePathname, useRouter } from "next/navigation";
import { useLocale, useTranslations } from "next-intl";
import { Loader2 } from "lucide-react";
import { useAuth } from "@/contexts/AuthContext";
import { listCorpora, type CorpusItem } from "@/app/lib/mikeApi";

interface TabDef {
    id: string;
    label: string;
    /**
     * When `href` is null the tab renders dimmed and non-clickable.
     * Used for corpora declared in a manifest but not yet wired
     * (e.g. CNIL while the `http-fetch-per-id` strategy is in
     * roadmap, or any corpus the backend marked `runnable: false`).
     */
    href: string | null;
    /** Optional tooltip text — manifest description for corpora. */
    title?: string;
}

interface TabGroup {
    heading: string;
    tabs: TabDef[];
}

/**
 * Map a corpus id (from the JSON manifest registry) to its existing
 * settings page route. Today this is a small hardcoded table: only
 * corpora that already have a hand-written React page are
 * navigable. New corpora declared in JSON show up in the sidebar but
 * are dimmed until a page exists (or until we land the generic
 * `/account/corpora/:id` panel).
 *
 * Kept here on purpose — not in the manifest — because the routing
 * decision is a frontend concern, not a corpus-config concern.
 */
const CORPUS_ROUTE_BY_ID: Record<string, string> = {
    eurlex: "/account/eurlex",
    "italian-legal": "/account/italia-legale",
};

export default function AccountLayout({
    children,
}: {
    children: React.ReactNode;
}) {
    const router = useRouter();
    const pathname = usePathname();
    const { isAuthenticated, authLoading } = useAuth();
    const tAccount = useTranslations("Account");
    const tCommon = useTranslations("Common");
    const locale = useLocale();

    // Corpora come from the backend registry (/corpora) at mount time.
    // Three observable states:
    //   - corpora === null  → still loading
    //   - corpora !== null && loadError === null → loaded (possibly empty)
    //   - loadError !== null → fetch failed; we render the static fallback
    //                          (Documenti locali only) so the sidebar
    //                          isn't completely broken.
    const [corpora, setCorpora] = useState<CorpusItem[] | null>(null);
    const [loadError, setLoadError] = useState<string | null>(null);

    useEffect(() => {
        if (authLoading || !isAuthenticated) return;
        let cancelled = false;
        listCorpora()
            .then((items) => {
                if (!cancelled) setCorpora(items);
            })
            .catch((err) => {
                console.error("[settings] /corpora load failed", err);
                if (!cancelled) {
                    setCorpora([]);
                    setLoadError(tAccount("corporaLoadFailed"));
                }
            });
        return () => {
            cancelled = true;
        };
    }, [authLoading, isAuthenticated, tAccount]);

    // Two semantic groups:
    //  1. Configurazione — hardcoded (account profile, LLM provider
    //     keys, MCP servers). These are intrinsically not "data
    //     sources" so they don't belong in the registry.
    //  2. Documenti & fonti — data-driven from /corpora. The static
    //     "Documenti locali" entry stays first (it's local-folder
    //     sync, not a corpus); every corpus the backend declared
    //     appears below it. Runnable corpora are clickable;
    //     not-yet-wired ones render dimmed with the manifest
    //     description as tooltip.
    const groups: TabGroup[] = useMemo(() => {
        const sourcesTabs: TabDef[] = [
            {
                id: "local-docs",
                label: tAccount("localDocsLink"),
                href: "/account/sync",
            },
        ];
        for (const c of corpora ?? []) {
            const route = CORPUS_ROUTE_BY_ID[c.id] ?? null;
            const runnable = c.runnable && route !== null;
            // Per-locale name resolution at the consumer (the
            // manifest sends the full map but the API projection
            // already collapsed it to `display_name` at the
            // user's locale via /corpora — for now we just trust
            // it). When we add locale-aware resolution server-side
            // this `c.display_name` reads will pick up the right
            // language automatically.
            const baseLabel = c.display_name;
            const label = runnable
                ? baseLabel
                : `${baseLabel}${tAccount("comingSoonSuffix")}`;
            sourcesTabs.push({
                id: `corpus-${c.id}`,
                label,
                href: runnable ? route : null,
                title: c.description ?? undefined,
            });
        }
        return [
            {
                heading: tAccount("groupConfig"),
                tabs: [
                    { id: "general", label: tAccount("generalLink"), href: "/account" },
                    { id: "models",  label: tAccount("modelsLink"),  href: "/account/models" },
                    { id: "mcp",     label: tAccount("mcpLink"),     href: "/account/mcp" },
                ],
            },
            {
                heading: tAccount("groupSources"),
                tabs: sourcesTabs,
            },
        ];
    }, [corpora, tAccount, locale]);

    useEffect(() => {
        if (!authLoading && !isAuthenticated) {
            router.push("/");
        }
    }, [isAuthenticated, authLoading, router]);

    if (authLoading) {
        return (
            <div className="h-dvh bg-white flex items-center justify-center">
                <Loader2 className="h-8 w-8 animate-spin text-blue-600" />
            </div>
        );
    }

    if (!isAuthenticated) {
        return null;
    }

    return (
        <div className="flex flex-col h-full md:overflow-y-auto px-6 py-6 md:py-10">
            <div className="max-w-5xl w-full mx-auto">
                <h1 className="text-4xl font-medium mb-8 font-eb-garamond">
                    {tCommon("settings")}
                </h1>

                <div className="flex flex-col md:flex-row gap-6 md:gap-10">
                    <nav
                        aria-label={tCommon("settings")}
                        className="md:w-60 shrink-0 flex md:flex-col gap-6 md:gap-7 overflow-x-auto"
                    >
                        {groups.map((group, groupIdx) => (
                            <div
                                key={group.heading}
                                className="flex md:flex-col gap-1 min-w-0"
                            >
                                <h2 className="hidden md:block text-[11px] font-semibold uppercase tracking-wider text-gray-400 px-3 mb-1 select-none">
                                    {group.heading}
                                </h2>
                                {/* Mobile: thin divider between groups. */}
                                {groupIdx > 0 && (
                                    <div className="md:hidden self-center w-px h-6 bg-gray-200 mx-2" />
                                )}
                                {group.tabs.map((tab) => {
                                    if (tab.href === null) {
                                        return (
                                            <span
                                                key={tab.id}
                                                title={tab.title}
                                                className="text-left whitespace-nowrap px-3 py-2 rounded-md text-sm font-medium text-gray-300 select-none cursor-not-allowed"
                                            >
                                                {tab.label}
                                            </span>
                                        );
                                    }
                                    const active = pathname === tab.href;
                                    return (
                                        <button
                                            key={tab.id}
                                            onClick={() => router.push(tab.href!)}
                                            title={tab.title}
                                            className={`text-left whitespace-nowrap px-3 py-2 rounded-md text-sm font-medium transition-colors ${
                                                active
                                                    ? "bg-gray-100 text-gray-900"
                                                    : "text-gray-500 hover:text-gray-900 hover:bg-gray-50"
                                            }`}
                                        >
                                            {tab.label}
                                        </button>
                                    );
                                })}
                                {/* Inline load-error for the sources group. */}
                                {groupIdx === 1 && loadError && (
                                    <span
                                        className="hidden md:block text-[10px] text-red-500 px-3"
                                        role="status"
                                    >
                                        {loadError}
                                    </span>
                                )}
                            </div>
                        ))}
                    </nav>

                    <div className="flex-1 min-w-0">{children}</div>
                </div>
            </div>
        </div>
    );
}
