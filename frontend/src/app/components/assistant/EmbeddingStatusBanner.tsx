"use client";

import { useTranslations } from "next-intl";
import { Loader2, AlertTriangle } from "lucide-react";
import type { EmbeddingStatus } from "@/app/hooks/useEmbeddingStatus";

/**
 * Inline status banner shown above the chat composer while the
 * embedding subsystem is doing something the user should know about:
 *
 *   - "Istanza modello embeddings" — the ~5-10 s ONNX session build
 *     after a backend restart. Most visible case in practice.
 *   - "Download modello" — the one-shot model file fetch from HF
 *     (only on first launch ever).
 *   - "Calcolo embeddings N/M" — bulk indexing of a document.
 *
 * Invisible when stage is "idle" (the common steady state). Soft
 * fade — banner doesn't block input, the user can keep typing.
 */
export function EmbeddingStatusBanner({ status }: { status: EmbeddingStatus }) {
    const t = useTranslations("EmbeddingStatus");

    if (status.stage === "idle") return null;

    if (status.stage === "failed") {
        return (
            <div className="flex items-start gap-2 px-3 py-2 mb-2 rounded-md border border-red-200 bg-red-50 text-red-800 text-sm">
                <AlertTriangle className="h-4 w-4 mt-0.5 shrink-0" />
                <div className="flex-1 min-w-0">
                    <div className="font-medium">{t("failedTitle")}</div>
                    {status.error && (
                        <div className="mt-0.5 text-xs text-red-600 truncate">
                            {status.error}
                        </div>
                    )}
                </div>
            </div>
        );
    }

    let title: string;
    let detail: string | null = null;
    switch (status.stage) {
        case "loading-model":
            title = t("loadingModelTitle");
            detail = t("loadingModelDetail");
            break;
        case "downloading": {
            const mb = (b?: number) => (b ? (b / 1_048_576).toFixed(1) : "?");
            title = t("downloadingTitle");
            detail = status.total_bytes
                ? `${mb(status.downloaded)} / ${mb(status.total_bytes)} MB${
                      status.file ? ` — ${status.file}` : ""
                  }`
                : status.file
                  ? `${mb(status.downloaded)} MB — ${status.file}`
                  : `${mb(status.downloaded)} MB`;
            break;
        }
        case "embedding": {
            const pct =
                typeof status.percent === "number" ? `${status.percent}% — ` : "";
            title = t("embeddingTitle");
            detail = `${pct}${status.chunks_done ?? 0} / ${status.chunks_total ?? 0}`;
            break;
        }
        default:
            return null;
    }

    return (
        <div className="flex items-start gap-2 px-3 py-2 mb-2 rounded-md border border-amber-200 bg-amber-50 text-amber-900 text-sm">
            <Loader2 className="h-4 w-4 mt-0.5 shrink-0 animate-spin" />
            <div className="flex-1 min-w-0">
                <div className="font-medium">{title}</div>
                {detail && (
                    <div className="mt-0.5 text-xs text-amber-700 truncate">{detail}</div>
                )}
            </div>
        </div>
    );
}
