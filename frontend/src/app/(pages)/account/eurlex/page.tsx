"use client";

import { useEffect, useState } from "react";
import { useTranslations } from "next-intl";
import {
    Search,
    AlertCircle,
    Loader2,
    CheckCircle2,
    ExternalLink,
    Trash2,
    RefreshCw,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

const API_BASE =
    process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:3001";

function getToken() {
    return typeof window !== "undefined"
        ? localStorage.getItem("mike_auth_token") ?? ""
        : "";
}

/**
 * Open a URL in the OS default browser. In Tauri, target="_blank"
 * routes the click *back into* the WebView, which makes the in-app
 * shell behave like a browser. We invoke the `open_external_url`
 * Tauri command instead so the URL goes to Edge / Chrome / Safari /
 * the user's actual default.
 *
 * In a plain Next.js dev environment (no Tauri), `invoke` throws —
 * we fall back to `window.open` so the page still works during
 * frontend-only development.
 */
async function openExternal(url: string) {
    try {
        const tauri = await import("@tauri-apps/api/core");
        await tauri.invoke("open_external_url", { url });
    } catch {
        window.open(url, "_blank", "noopener,noreferrer");
    }
}

async function api<T>(path: string, init: RequestInit = {}): Promise<T> {
    const res = await fetch(`${API_BASE}${path}`, {
        ...init,
        headers: {
            "Content-Type": "application/json",
            Authorization: `Bearer ${getToken()}`,
            ...(init.headers ?? {}),
        },
    });
    if (!res.ok) {
        const text = await res.text().catch(() => "");
        throw new Error(text || `HTTP ${res.status}`);
    }
    if (res.status === 204) return undefined as T;
    return (await res.json()) as T;
}

interface ConfigOut {
    enabled: boolean;
    language: string;
    fallback_en: boolean;
}

interface FetchedDoc {
    id: string;
    filename: string;
    corpus_identifier: string;
    corpus_language: string;
    fetched_with_fallback: boolean;
    source_url?: string;
    size_bytes?: number;
    already_indexed?: boolean;
    chunks_indexed?: number;
    indexing_error?: string | null;
    status?: string;
}

interface SearchHit {
    identifier: string;
    title: string;
    date?: string | null;
    url: string;
    languages_available: string[];
}

interface SearchResponse {
    hits: SearchHit[];
    note?: string | null;
}

interface IndexedDoc {
    id: string;
    filename: string;
    corpus_identifier: string | null;
    corpus_language: string | null;
    fetched_with_fallback: boolean;
    size_bytes: number;
    created_at: string;
    source_url?: string | null;
    status: string;
    chunks_indexed?: number;
}

// EU24 official languages — surface-level codes used by EUR-Lex URL
// segments (`/IT/TXT/`, `/EN/TXT/`, ...). The actual fetch happens
// backend-side; this list seeds the dropdown.
const EU_LANGUAGES: { code: string; label: string }[] = [
    { code: "BG", label: "Български" },
    { code: "CS", label: "Čeština" },
    { code: "DA", label: "Dansk" },
    { code: "DE", label: "Deutsch" },
    { code: "EL", label: "Ελληνικά" },
    { code: "EN", label: "English" },
    { code: "ES", label: "Español" },
    { code: "ET", label: "Eesti" },
    { code: "FI", label: "Suomi" },
    { code: "FR", label: "Français" },
    { code: "GA", label: "Gaeilge" },
    { code: "HR", label: "Hrvatski" },
    { code: "HU", label: "Magyar" },
    { code: "IT", label: "Italiano" },
    { code: "LT", label: "Lietuvių" },
    { code: "LV", label: "Latviešu" },
    { code: "MT", label: "Malti" },
    { code: "NL", label: "Nederlands" },
    { code: "PL", label: "Polski" },
    { code: "PT", label: "Português" },
    { code: "RO", label: "Română" },
    { code: "SK", label: "Slovenčina" },
    { code: "SL", label: "Slovenščina" },
    { code: "SV", label: "Svenska" },
];

/**
 * EUR-Lex configuration + search panel.
 *
 * Preview UI — the backend adapter (see docs/CORPORA.md `LegalCorpusAdapter`
 * trait) is not yet implemented. The controls are fully wired to local
 * component state so the user can shape the desired flow; the search
 * button surfaces a "not implemented" notice instead of hitting the API.
 *
 * Persistence will move to /user/settings (the project's preferred
 * server-side store; see memory note "Prefer data/storage over localStorage")
 * once the backend lands. For now state is in-memory.
 */
export default function EurlexPage() {
    const t = useTranslations("Eurlex");

    const [configLoading, setConfigLoading] = useState(true);
    const [enabled, setEnabled] = useState(false);
    const [language, setLanguage] = useState<string>("IT");
    const [fallbackEnglish, setFallbackEnglish] = useState(true);
    const [savingConfig, setSavingConfig] = useState(false);

    const [searchQuery, setSearchQuery] = useState("");
    const [searching, setSearching] = useState(false);
    const [indexedDocs, setIndexedDocs] = useState<IndexedDoc[]>([]);
    const [indexedLoading, setIndexedLoading] = useState(true);
    const [deletingId, setDeletingId] = useState<string | null>(null);
    const [resyncingId, setResyncingId] = useState<string | null>(null);
    // Live progress for the doc currently being embedded. `null`
    // between jobs; populated by the /eurlex/embed-progress poll.
    const [embedProgress, setEmbedProgress] = useState<{
        document_id: string;
        current: number;
        total: number;
        percent: number;
    } | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [searchNote, setSearchNote] = useState<string | null>(null);
    const [hits, setHits] = useState<SearchHit[] | null>(null);
    // Track which CELEX is currently being synced so we can show
    // a per-row spinner without blocking the rest of the list.
    const [syncing, setSyncing] = useState<string | null>(null);
    const [synced, setSynced] = useState<Record<string, FetchedDoc>>({});
    const [lastFetched, setLastFetched] = useState<FetchedDoc | null>(null);

    // Hydrate from /eurlex/config on mount.
    useEffect(() => {
        let cancelled = false;
        api<ConfigOut>("/eurlex/config")
            .then((cfg) => {
                if (cancelled) return;
                setEnabled(cfg.enabled);
                setLanguage((cfg.language || "en").toUpperCase());
                setFallbackEnglish(cfg.fallback_en);
            })
            .catch((e) => {
                if (!cancelled) setError(String(e));
            })
            .finally(() => {
                if (!cancelled) setConfigLoading(false);
            });
        return () => {
            cancelled = true;
        };
    }, []);

    // Refresh the indexed-documents list. Called on mount and after
    // every sync / delete so the visible list stays in step with the
    // backend without polling.
    const refreshIndexed = async () => {
        try {
            const resp = await api<{ documents: IndexedDoc[] }>(
                "/eurlex/documents",
            );
            setIndexedDocs(resp.documents);
        } catch (e) {
            console.warn("[eurlex] refresh indexed failed:", e);
        }
    };

    useEffect(() => {
        let cancelled = false;
        api<{ documents: IndexedDoc[] }>("/eurlex/documents")
            .then((resp) => {
                if (!cancelled) setIndexedDocs(resp.documents);
            })
            .catch((e) => {
                if (!cancelled)
                    console.warn("[eurlex] initial indexed load failed:", e);
            })
            .finally(() => {
                if (!cancelled) setIndexedLoading(false);
            });
        return () => {
            cancelled = true;
        };
    }, []);

    // Poll /eurlex/embed-progress while at least one row is syncing.
    // Stops as soon as the syncing row count drops to zero, so we
    // don't keep hitting the backend during idle periods.
    useEffect(() => {
        const anySyncing = indexedDocs.some((d) => d.status === "syncing");
        if (!anySyncing) {
            setEmbedProgress(null);
            return;
        }
        let cancelled = false;
        const tick = async () => {
            try {
                const p = await api<
                    | {
                          document_id: string;
                          current: number;
                          total: number;
                          percent: number;
                      }
                    | null
                >("/eurlex/embed-progress");
                if (!cancelled) setEmbedProgress(p);
                // If the backend says 100% or no active embed, refresh
                // the documents list so the row flips to "synched".
                if (!cancelled && (p === null || p.percent >= 100)) {
                    await refreshIndexed();
                }
            } catch (e) {
                console.warn("[eurlex] progress poll failed:", e);
            }
        };
        tick();
        const handle = setInterval(tick, 500);
        return () => {
            cancelled = true;
            clearInterval(handle);
        };
    }, [indexedDocs]);

    // Persist config on change (debounced via the savingConfig guard).
    useEffect(() => {
        if (configLoading) return;
        let cancelled = false;
        const handle = setTimeout(async () => {
            setSavingConfig(true);
            try {
                await api("/eurlex/config", {
                    method: "PUT",
                    body: JSON.stringify({
                        enabled,
                        language: language.toLowerCase(),
                        fallback_en: fallbackEnglish,
                    }),
                });
                if (!cancelled) setError(null);
            } catch (e) {
                if (!cancelled) setError(String(e));
            } finally {
                if (!cancelled) setSavingConfig(false);
            }
        }, 350);
        return () => {
            cancelled = true;
            clearTimeout(handle);
        };
    }, [enabled, language, fallbackEnglish, configLoading]);

    const runSearch = async () => {
        setError(null);
        setSearchNote(null);
        setHits(null);
        setLastFetched(null);
        const q = searchQuery.trim();
        if (!q) {
            setError(
                "Inserisci un CELEX (es. 32016R0679), un riferimento (es. 'Direttiva 2014/24/UE'), oppure parole chiave (es. 'trattamento dati personali').",
            );
            return;
        }
        setSearching(true);
        try {
            const resp = await api<SearchResponse>("/eurlex/search", {
                method: "POST",
                body: JSON.stringify({
                    query: q,
                    language: language.toLowerCase(),
                }),
            });
            setHits(resp.hits);
            setSearchNote(resp.note ?? null);
        } catch (e) {
            setError(String(e));
        } finally {
            setSearching(false);
        }
    };

    const syncHit = async (hit: SearchHit) => {
        setError(null);
        setSyncing(hit.identifier);
        // Kick the indexed-list refresh in the background so the
        // 'syncing' row appears immediately and the embed-progress
        // poll has something to attach to. Without this nudge, the
        // backend insert+embed runs to completion before the list
        // even shows the row, so the progress bar never gets a
        // chance to render.
        const indexedRefreshTimer = setInterval(() => {
            void refreshIndexed();
        }, 1000);
        try {
            const doc = await api<FetchedDoc>("/eurlex/fetch", {
                method: "POST",
                body: JSON.stringify({
                    celex: hit.identifier,
                    language: language.toLowerCase(),
                }),
            });
            setSynced((prev) => ({ ...prev, [hit.identifier]: doc }));
            setLastFetched(doc);
            // Surface server-side indexing problems even when the HTTP
            // call returned 200. The /fetch endpoint flips the row to
            // 'interrupted' instead of 'ready' when the chunker
            // produces 0 chunks, but the user still needs to know
            // *why* the new "interrotto" badge appeared.
            if (doc.indexing_error) {
                setError(
                    `Documento sincronizzato con problemi: ${doc.indexing_error}`,
                );
            } else if (
                typeof doc.chunks_indexed === "number" &&
                doc.chunks_indexed === 0 &&
                doc.status !== "ready"
            ) {
                setError(
                    "Documento scaricato ma indicizzazione incompleta. Usa Riavvia.",
                );
            }
            // Refresh the indexed-list so the new doc shows up
            // immediately in the "Documenti sincronizzati" section.
            await refreshIndexed();
        } catch (e) {
            setError(String(e));
        } finally {
            clearInterval(indexedRefreshTimer);
            setSyncing(null);
        }
    };

    const resyncDoc = async (doc: IndexedDoc) => {
        setError(null);
        setResyncingId(doc.id);
        // Optimistic local-state flip so the badge reflects the new
        // syncing state immediately, before the backend round-trip.
        setIndexedDocs((prev) =>
            prev.map((d) =>
                d.id === doc.id ? { ...d, status: "syncing" } : d,
            ),
        );
        try {
            const resp = await api<{
                status: string;
                chunks_indexed: number;
                indexing_error?: string | null;
            }>(`/eurlex/documents/${doc.id}/resync`, { method: "POST" });
            setIndexedDocs((prev) =>
                prev.map((d) =>
                    d.id === doc.id ? { ...d, status: resp.status } : d,
                ),
            );
            if (resp.indexing_error) {
                setError(`Resync fallito: ${resp.indexing_error}`);
            }
        } catch (e) {
            setError(String(e));
            // Bring the row back to whatever it was before — refresh
            // from server is the simplest correct action.
            await refreshIndexed();
        } finally {
            setResyncingId(null);
        }
    };

    const deleteDoc = async (doc: IndexedDoc) => {
        const isInFlight =
            doc.status === "syncing" &&
            embedProgress?.document_id === doc.id;
        const isQueuedDoc =
            doc.status === "syncing" && !isInFlight;
        const message = isInFlight
            ? `Una sincronizzazione è in corso per "${doc.filename}". Interromperla e rimuovere il documento?`
            : isQueuedDoc
              ? `"${doc.filename}" è in coda. Annullare e rimuovere?`
              : `Rimuovere "${doc.filename}" dall'indice?`;
        if (!confirm(message)) return;
        setError(null);
        setDeletingId(doc.id);
        try {
            await api(`/eurlex/documents/${doc.id}`, { method: "DELETE" });
            setIndexedDocs((prev) => prev.filter((d) => d.id !== doc.id));
            if (doc.corpus_identifier) {
                setSynced((prev) => {
                    const c = { ...prev };
                    delete c[doc.corpus_identifier!];
                    return c;
                });
            }
        } catch (e) {
            setError(String(e));
        } finally {
            setDeletingId(null);
        }
    };

    return (
        <div className="space-y-6 max-w-4xl">
            <div>
                <h2 className="text-2xl font-medium font-serif mb-2">
                    {t("title")}
                </h2>
                <p className="text-sm text-gray-500 leading-relaxed">
                    {t("subtitle")}
                </p>
            </div>

            {savingConfig && (
                <div className="text-xs text-gray-500">Salvataggio…</div>
            )}

            {/* Sync toggle */}
            <section className="border border-gray-200 rounded-lg p-4 space-y-3">
                <label className="flex items-start gap-3 cursor-pointer">
                    <input
                        type="checkbox"
                        checked={enabled}
                        onChange={(e) => setEnabled(e.target.checked)}
                        className="mt-1"
                    />
                    <span className="flex-1">
                        <span className="text-sm font-medium block">
                            {t("enabled")}
                        </span>
                        <span className="text-xs text-gray-500 block mt-0.5">
                            {t("enabledHint")}
                        </span>
                    </span>
                </label>
            </section>

            {/* Language picker + EN fallback */}
            <section className="border border-gray-200 rounded-lg p-4 space-y-3">
                <div>
                    <label className="text-sm font-medium block mb-1">
                        {t("language")}
                    </label>
                    <p className="text-xs text-gray-500 mb-2">
                        {t("languageHint")}
                    </p>
                    <select
                        value={language}
                        onChange={(e) => setLanguage(e.target.value)}
                        disabled={!enabled}
                        className="w-full md:w-64 rounded-md border border-gray-200 bg-white px-3 py-2 text-sm hover:border-gray-400 focus:outline-none transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                        {EU_LANGUAGES.map((l) => (
                            <option key={l.code} value={l.code}>
                                {l.label} ({l.code})
                            </option>
                        ))}
                    </select>
                </div>
                <label className="flex items-center gap-2 text-sm text-gray-600">
                    <input
                        type="checkbox"
                        checked={fallbackEnglish}
                        onChange={(e) => setFallbackEnglish(e.target.checked)}
                        disabled={!enabled || language === "EN"}
                    />
                    {t("fallbackEnglish")}
                </label>
            </section>

            {/* Smart search — auto-detects CELEX / natural ref / keyword */}
            <section className="border border-gray-200 rounded-lg p-4 space-y-3">
                <div>
                    <label className="text-xs text-gray-500 block mb-1">
                        {t("smartSearch")}
                    </label>
                    <Input
                        value={searchQuery}
                        onChange={(e) => setSearchQuery(e.target.value)}
                        placeholder={t("smartSearchPlaceholder")}
                        disabled={!enabled}
                        onKeyDown={(e) => {
                            if (
                                e.key === "Enter" &&
                                enabled &&
                                !searching &&
                                searchQuery.trim()
                            ) {
                                e.preventDefault();
                                runSearch();
                            }
                        }}
                    />
                    <p className="text-[11px] text-gray-400 mt-1">
                        {t("smartSearchHint")}
                    </p>
                </div>
                <div className="flex justify-end">
                    <Button
                        onClick={runSearch}
                        disabled={
                            !enabled || searching || !searchQuery.trim()
                        }
                        className="bg-black text-white hover:bg-gray-900"
                    >
                        {searching ? (
                            <Loader2 className="h-3.5 w-3.5 mr-1 animate-spin" />
                        ) : (
                            <Search className="h-3.5 w-3.5 mr-1" />
                        )}
                        {searching ? t("searching") : t("searchButton")}
                    </Button>
                </div>
            </section>

            {error && (
                <div className="text-sm text-red-600 bg-red-50 border border-red-200 rounded-md px-3 py-2 flex items-start gap-2">
                    <AlertCircle className="h-4 w-4 shrink-0 mt-0.5" />
                    <span>{error}</span>
                </div>
            )}

            {searchNote && hits !== null && hits.length === 0 && (
                <div className="text-sm text-gray-500 bg-gray-50 border border-gray-200 rounded-md px-3 py-2">
                    {searchNote}
                </div>
            )}

            {/* Results list — one row per hit, with per-row Sync button */}
            {hits !== null && hits.length > 0 && (
                <section>
                    <h3 className="text-sm font-medium mb-2">
                        Risultati ({hits.length})
                    </h3>
                    <ul className="space-y-2">
                        {hits.map((hit) => {
                            const alreadySynced = !!synced[hit.identifier];
                            const isSyncingThis = syncing === hit.identifier;
                            return (
                                <li
                                    key={hit.identifier}
                                    className="border border-gray-200 rounded-lg p-3 flex items-start justify-between gap-3"
                                >
                                    <div className="min-w-0 flex-1">
                                        <div className="text-sm font-medium truncate">
                                            {hit.title}
                                        </div>
                                        <div className="text-xs text-gray-500 mt-0.5">
                                            CELEX {hit.identifier}
                                            {hit.languages_available.length > 0 && (
                                                <span className="ml-2">
                                                    · servito in{" "}
                                                    {hit.languages_available
                                                        .map((l) => l.toUpperCase())
                                                        .join(", ")}
                                                </span>
                                            )}
                                        </div>
                                        <button
                                            type="button"
                                            onClick={() => openExternal(hit.url)}
                                            className="text-xs inline-flex items-center gap-1 text-gray-500 hover:text-gray-800 mt-1 cursor-pointer"
                                        >
                                            <ExternalLink className="h-3 w-3" />
                                            Apri su EUR-Lex
                                        </button>
                                    </div>
                                    <button
                                        type="button"
                                        onClick={() => syncHit(hit)}
                                        disabled={
                                            isSyncingThis || alreadySynced
                                        }
                                        className={`shrink-0 inline-flex items-center gap-1 rounded-md px-2.5 py-1.5 text-xs transition-colors ${
                                            alreadySynced
                                                ? "bg-green-50 text-green-700 border border-green-200"
                                                : "bg-black text-white hover:bg-gray-900 disabled:opacity-50"
                                        }`}
                                    >
                                        {isSyncingThis ? (
                                            <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                        ) : alreadySynced ? (
                                            <CheckCircle2 className="h-3.5 w-3.5" />
                                        ) : null}
                                        {isSyncingThis
                                            ? t("syncing")
                                            : alreadySynced
                                              ? t("alreadyIndexed")
                                              : t("indexAction")}
                                    </button>
                                </li>
                            );
                        })}
                    </ul>
                </section>
            )}

            {lastFetched && (
                <div className="text-sm text-green-700 bg-green-50 border border-green-200 rounded-md px-3 py-2 flex items-start gap-2">
                    <CheckCircle2 className="h-4 w-4 shrink-0 mt-0.5" />
                    <div className="flex-1 min-w-0">
                        <div className="font-medium truncate">
                            {lastFetched.filename}
                        </div>
                        <div className="text-xs text-green-800/80">
                            CELEX {lastFetched.corpus_identifier} ·{" "}
                            {lastFetched.corpus_language.toUpperCase()}
                            {lastFetched.fetched_with_fallback && (
                                <span className="ml-2 inline-block px-1.5 py-0.5 rounded bg-amber-100 text-amber-900 text-[10px]">
                                    fallback EN
                                </span>
                            )}
                            {lastFetched.already_indexed && (
                                <span className="ml-2 inline-block px-1.5 py-0.5 rounded bg-gray-100 text-gray-700 text-[10px]">
                                    già indicizzato
                                </span>
                            )}
                        </div>
                    </div>
                </div>
            )}

            {/* Indexed documents — always visible, with delete buttons */}
            <section>
                <h3 className="text-sm font-medium mb-2">
                    {t("indexedTitle")} ({indexedDocs.length})
                </h3>
                {indexedLoading ? (
                    <p className="text-sm text-gray-400">Caricamento…</p>
                ) : indexedDocs.length === 0 ? (
                    <p className="text-sm text-gray-400">
                        {t("indexedEmpty")}
                    </p>
                ) : (
                    <ul className="space-y-2">
                        {indexedDocs.map((doc) => {
                            const isDeleting = deletingId === doc.id;
                            const isResyncing = resyncingId === doc.id;
                            const isSyncing =
                                doc.status === "syncing" || isResyncing;
                            const isInterrupted = doc.status === "interrupted";
                            // A row marked 'ready' but with 0 chunks behaves
                            // like 'interrupted' from the user's perspective
                            // — RAG can't reach it. We treat it as a soft
                            // failure so the Riavvia button shows up.
                            const isEmptyReady =
                                doc.status === "ready" &&
                                doc.chunks_indexed === 0;
                            const isReady =
                                doc.status === "ready" && !isEmptyReady;
                            // Only the doc the embedding service is actively
                            // working on right now is "in flight". Other
                            // syncing rows are queued behind the model
                            // mutex and surface as "in attesa" — different
                            // user expectation: they will start when the
                            // active one finishes.
                            const isActiveEmbed =
                                isSyncing &&
                                embedProgress?.document_id === doc.id;
                            const isQueued = isSyncing && !isActiveEmbed;
                            return (
                                <li
                                    key={doc.id}
                                    className="border border-gray-200 rounded-lg p-3 flex items-start justify-between gap-3"
                                >
                                    <div className="min-w-0 flex-1">
                                        <div className="text-sm font-medium truncate flex items-center gap-2">
                                            {doc.filename}
                                            {isReady && (
                                                <span className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded bg-green-50 text-green-700 border border-green-200 text-[10px] font-normal">
                                                    <CheckCircle2 className="h-3 w-3" />
                                                    indicizzato
                                                </span>
                                            )}
                                            {isActiveEmbed && (
                                                <span className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded bg-blue-50 text-blue-700 border border-blue-200 text-[10px] font-normal">
                                                    <Loader2 className="h-3 w-3 animate-spin" />
                                                    in sync
                                                    {embedProgress && (
                                                        <span className="ml-1 tabular-nums">
                                                            {embedProgress.percent}%
                                                        </span>
                                                    )}
                                                </span>
                                            )}
                                            {isQueued && (
                                                <span
                                                    className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded bg-gray-50 text-gray-600 border border-gray-200 text-[10px] font-normal"
                                                    title="Le sync sono sequenziali — questa partirà al termine di quella in corso."
                                                >
                                                    <Loader2 className="h-3 w-3" />
                                                    in attesa
                                                </span>
                                            )}
                                            {isInterrupted && (
                                                <span className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded bg-red-50 text-red-700 border border-red-200 text-[10px] font-normal">
                                                    <AlertCircle className="h-3 w-3" />
                                                    interrotto
                                                </span>
                                            )}
                                            {isEmptyReady && (
                                                <span
                                                    className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded bg-amber-50 text-amber-700 border border-amber-200 text-[10px] font-normal"
                                                    title="0 chunk: il RAG non può recuperare questo documento. Usa Riavvia."
                                                >
                                                    <AlertCircle className="h-3 w-3" />
                                                    senza chunk
                                                </span>
                                            )}
                                        </div>
                                        <div className="text-xs text-gray-500 mt-0.5">
                                            {doc.corpus_identifier && (
                                                <>CELEX {doc.corpus_identifier} · </>
                                            )}
                                            {doc.corpus_language?.toUpperCase()}
                                            {doc.fetched_with_fallback && (
                                                <span className="ml-2 inline-block px-1.5 py-0.5 rounded bg-amber-100 text-amber-900 text-[10px]">
                                                    fallback EN
                                                </span>
                                            )}
                                            <span className="ml-2 text-gray-400">
                                                {(doc.size_bytes / 1024).toFixed(0)} KB
                                            </span>
                                            {typeof doc.chunks_indexed === "number" && (
                                                <span
                                                    className={`ml-2 ${
                                                        isReady && doc.chunks_indexed === 0
                                                            ? "text-amber-700"
                                                            : "text-gray-400"
                                                    }`}
                                                    title={
                                                        isReady &&
                                                        doc.chunks_indexed === 0
                                                            ? "Zero chunk indicizzati: il documento risulta 'ready' ma non è raggiungibile dal RAG. Usa Riavvia."
                                                            : undefined
                                                    }
                                                >
                                                    · {doc.chunks_indexed} chunk
                                                </span>
                                            )}
                                        </div>
                                        {doc.source_url && (
                                            <button
                                                type="button"
                                                onClick={() =>
                                                    openExternal(doc.source_url!)
                                                }
                                                className="text-xs inline-flex items-center gap-1 text-gray-500 hover:text-gray-800 mt-1 cursor-pointer"
                                            >
                                                <ExternalLink className="h-3 w-3" />
                                                Apri su EUR-Lex
                                            </button>
                                        )}
                                        {isActiveEmbed && embedProgress && (
                                            <div className="mt-2">
                                                <div className="h-1.5 bg-gray-100 rounded overflow-hidden">
                                                    <div
                                                        className="h-full bg-blue-500 transition-all"
                                                        style={{
                                                            width: `${Math.min(100, embedProgress.percent)}%`,
                                                        }}
                                                    />
                                                </div>
                                                <div className="text-[10px] text-gray-500 mt-0.5 tabular-nums">
                                                    {embedProgress.current}/
                                                    {embedProgress.total} chunk
                                                    embed
                                                </div>
                                            </div>
                                        )}
                                        {isQueued && (
                                            <div className="mt-1 text-[11px] text-gray-500">
                                                In coda — partirà al termine
                                                della sync attiva.
                                            </div>
                                        )}
                                    </div>
                                    <div className="flex items-center gap-1 shrink-0">
                                        {(isInterrupted || isEmptyReady) && (
                                            <button
                                                type="button"
                                                onClick={() => resyncDoc(doc)}
                                                disabled={isResyncing}
                                                className="rounded-md px-2 py-1 text-xs text-blue-700 hover:bg-blue-50 transition-colors disabled:opacity-40 inline-flex items-center gap-1"
                                                aria-label="Riavvia sync"
                                                title="Riavvia sincronizzazione"
                                            >
                                                <RefreshCw
                                                    className={`h-3.5 w-3.5 ${isResyncing ? "animate-spin" : ""}`}
                                                />
                                                Riavvia
                                            </button>
                                        )}
                                        <button
                                            type="button"
                                            onClick={() => deleteDoc(doc)}
                                            disabled={isDeleting}
                                            className="rounded-md p-1.5 text-gray-400 hover:text-red-600 hover:bg-red-50 transition-colors disabled:opacity-40"
                                            aria-label={t("remove")}
                                            title={
                                                isQueued
                                                    ? "Annulla questa sync in coda"
                                                    : isActiveEmbed
                                                      ? "Interrompi e rimuovi"
                                                      : t("remove")
                                            }
                                        >
                                            {isDeleting ? (
                                                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                            ) : (
                                                <Trash2 className="h-3.5 w-3.5" />
                                            )}
                                        </button>
                                    </div>
                                </li>
                            );
                        })}
                    </ul>
                )}
            </section>
        </div>
    );
}
