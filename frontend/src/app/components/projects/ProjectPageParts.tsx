"use client";

// Shared building blocks for the project page UI. Extracted from
// ProjectPage.tsx (was 1860+ lines inline) so the orchestrator stays
// focused on state + tab switching, and the tabs / helpers can be
// imported anywhere they're useful.
//
// Pattern lifted from upstream willchen96/mike commit f39f175 (PR #64).
// Two deliberate divergences from upstream:
//   - DocVersionHistory uses next-intl (useTranslations("DocVersions"))
//     instead of hardcoded English. MikeRust ships in IT + EN and every
//     UI string goes through the i18n layer (see feedback_ui_i18n).
//   - DocVersionHistory does NOT take a `depth` prop for tree-indented
//     rendering. Upstream needed it because their Documents tab can
//     show subfolders as tree rows with versions nested under them at
//     depth > 0. MikeRust renders versions flat (the parent doc is the
//     only anchor), so the indentation helpers stay out of this file.
// If MikeRust later grows tree-indented version history, port the
// treeControlCellStyle / treeNameCellStyle helpers from upstream Parts.

import { useState } from "react";
import { useTranslations } from "next-intl";
import { Download, File, FileText, Loader2, Pencil } from "lucide-react";
import { type MikeDocumentVersion } from "@/app/lib/mikeApi";

// ---------------------------------------------------------------------------
// Layout primitives shared across the tabs
// ---------------------------------------------------------------------------

export const CHECK_W = "w-8 shrink-0";
export const NAME_COL_W = "w-[300px] shrink-0";

// ---------------------------------------------------------------------------
// Formatters
// ---------------------------------------------------------------------------

export function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function formatDate(iso: string) {
    return new Date(iso).toLocaleDateString(undefined, {
        day: "numeric",
        month: "short",
        year: "numeric",
    });
}

// ---------------------------------------------------------------------------
// Tiny shared widgets
// ---------------------------------------------------------------------------

export function DocIcon({ fileType }: { fileType: string | null }) {
    if (fileType === "pdf")
        return <FileText className="h-4 w-4 text-red-600 shrink-0" />;
    if (fileType === "docx" || fileType === "doc")
        return <File className="h-4 w-4 text-blue-600 shrink-0" />;
    return <File className="h-4 w-4 text-gray-500 shrink-0" />;
}

/**
 * Stacked rows rendered beneath a doc row when its Version column is
 * expanded. Each row shows a past (or current) version with its number,
 * source, date, and a download button that fetches that specific version.
 */
export function DocVersionHistory({
    docId,
    filename,
    loading,
    versions,
    onDownloadVersion,
    onOpenVersion,
    onRenameVersion,
}: {
    docId: string;
    filename: string;
    loading: boolean;
    versions: MikeDocumentVersion[];
    onDownloadVersion: (
        docId: string,
        versionId: string,
        filename: string,
    ) => void;
    onOpenVersion?: (
        versionId: string,
        versionLabel: string,
    ) => void;
    onRenameVersion?: (
        versionId: string,
        displayName: string | null,
    ) => Promise<void> | void;
}) {
    const tDV = useTranslations("DocVersions");
    const [editingVersionId, setEditingVersionId] = useState<string | null>(
        null,
    );
    const [editingValue, setEditingValue] = useState("");

    const commit = async (versionId: string) => {
        const trimmed = editingValue.trim();
        setEditingVersionId(null);
        // Empty string → clear override (falls back to V{n})
        const next = trimmed.length > 0 ? trimmed : null;
        await onRenameVersion?.(versionId, next);
    };
    if (loading && versions.length === 0) {
        return (
            <div className="flex items-center h-9 border-b border-gray-50 text-xs text-gray-500 bg-gray-50/60">
                <div className={`sticky left-0 z-[60] ${CHECK_W} bg-gray-50/60 self-stretch`} />
                <div className={`sticky left-8 z-[60] ${NAME_COL_W} bg-gray-50/60 p-2`}>
                    <div className="flex items-center gap-2">
                        <Loader2 className="h-3 w-3 animate-spin text-gray-400" />
                        <span>{tDV("loading")}</span>
                    </div>
                </div>
            </div>
        );
    }
    if (versions.length === 0) {
        return (
            <div className="flex items-center h-9 border-b border-gray-50 text-xs text-gray-400 bg-gray-50/60">
                <div className={`sticky left-0 z-[60] ${CHECK_W} bg-gray-50/60 self-stretch`} />
                <div className={`sticky left-8 z-[60] ${NAME_COL_W} bg-gray-50/60 p-2`}>
                    <div>{tDV("noHistory")}</div>
                </div>
            </div>
        );
    }
    // Most recent version first.
    const ordered = [...versions].reverse();
    return (
        <>
            {ordered.map((v) => {
                const numberLabel =
                    typeof v.version_number === "number" && v.version_number >= 1
                        ? `${v.version_number}`
                        : v.source === "upload"
                          ? "Original"
                          : "—";
                const displayLabel = v.display_name?.trim() || numberLabel;
                const dt = new Date(v.created_at);
                const dateLabel = Number.isNaN(dt.valueOf())
                    ? ""
                    : dt.toLocaleString(undefined, {
                          month: "short",
                          day: "numeric",
                          year: "numeric",
                          hour: "numeric",
                          minute: "2-digit",
                      });
                const isEditing = editingVersionId === v.id;
                return (
                    <div
                        key={`ver-${docId}-${v.id}`}
                        onClick={() => {
                            if (isEditing) return;
                            onOpenVersion?.(v.id, displayLabel);
                        }}
                        className="group flex items-center h-9 pr-8 border-b border-gray-50 bg-gray-50/60 text-xs text-gray-600 cursor-pointer hover:bg-gray-100/80 transition-colors"
                    >
                        <div className={`sticky left-0 z-[60] ${CHECK_W} bg-gray-50/60 group-hover:bg-gray-100/80 self-stretch`} />
                        <div className={`sticky left-8 z-[60] ${NAME_COL_W} bg-gray-50/60 group-hover:bg-gray-100/80 p-2`}>
                        <div className="flex items-center gap-2">
                            <span className="shrink-0 text-gray-400">↳</span>
                            {isEditing ? (
                                <input
                                    autoFocus
                                    value={editingValue}
                                    onClick={(e) => e.stopPropagation()}
                                    onChange={(e) =>
                                        setEditingValue(e.target.value)
                                    }
                                    onKeyDown={(e) => {
                                        if (e.key === "Enter") {
                                            e.preventDefault();
                                            void commit(v.id);
                                        } else if (e.key === "Escape") {
                                            setEditingVersionId(null);
                                        }
                                    }}
                                    onBlur={() => void commit(v.id)}
                                    className="min-w-0 flex-1 max-w-[240px] border-b border-gray-300 bg-transparent text-xs text-gray-800 outline-none focus:border-gray-500"
                                />
                            ) : (
                                <span className="font-medium text-gray-700 truncate">
                                    {displayLabel}
                                </span>
                            )}
                            {!isEditing && onRenameVersion && (
                                <button
                                    onClick={(e) => {
                                        e.stopPropagation();
                                        setEditingVersionId(v.id);
                                        setEditingValue(v.display_name ?? "");
                                    }}
                                    title={tDV("renameVersion")}
                                    className="shrink-0 rounded p-0.5 text-gray-400 opacity-0 group-hover:opacity-100 hover:text-gray-700 hover:bg-gray-200 transition"
                                >
                                    <Pencil className="h-3 w-3" />
                                </button>
                            )}
                            <span className="text-gray-400 truncate">{dateLabel}</span>
                            <span className="text-gray-300 shrink-0">·</span>
                            <span className="text-gray-400 truncate">{v.source}</span>
                        </div>
                        </div>
                        <div className="ml-auto w-20 shrink-0" />
                        <div className="w-24 shrink-0" />
                        <div className="ml-auto w-20 shrink-0" />
                        <div className="w-8 shrink-0 flex justify-end">
                            <button
                                onClick={(e) => {
                                    e.stopPropagation();
                                    onDownloadVersion(docId, v.id, filename);
                                }}
                                title={tDV("downloadVersion")}
                                className="flex items-center justify-center w-6 h-6 rounded text-gray-500 hover:text-gray-800 hover:bg-gray-100 transition-colors"
                            >
                                <Download className="h-3.5 w-3.5" />
                            </button>
                        </div>
                    </div>
                );
            })}
        </>
    );
}
