"use client";

import { useEffect, useState } from "react";
import {
    getModelStatus,
    getEmbedProgress,
    type ModelStatus,
    type EmbedProgress,
} from "@/app/lib/mikeApi";

/**
 * Stages exposed to the UI. Mapped from two backend endpoints:
 *
 *   /sync/model-status      → instantiation lifecycle of the
 *                             multilingual-e5-base ONNX session
 *   /eurlex/embed-progress  → live N/M chunk progress when a batch
 *                             is computing embeddings
 *
 * The "loading-model" stage is the noticeable one (5-10 s ONNX session
 * build on CPU after a backend restart); the "embedding" stage only
 * appears during bulk document indexing. Per-query embedding (one
 * short text on every chat turn) is ~50 ms — too fast to surface.
 */
export type EmbeddingStage =
    | "idle"           // model not in use, nothing to show
    | "downloading"    // one-shot model file download from HuggingFace
    | "loading-model"  // ONNX session build in progress
    | "embedding"      // a batch embed job is running
    | "failed";        // last attempt errored

export interface EmbeddingStatus {
    stage: EmbeddingStage;
    /** Bytes received in the active download (only for "downloading"). */
    downloaded?: number;
    total_bytes?: number | null;
    file?: string;
    /** Chunks done / total chunks (only for "embedding"). */
    chunks_done?: number;
    chunks_total?: number;
    percent?: number;
    /** Last error message (only for "failed"). */
    error?: string;
}

/**
 * Poll the two endpoints every `interval` ms while `enabled` is true.
 * Stops polling automatically once it observes a `ready` model AND no
 * active embed — until `enabled` flips false-then-true, no further
 * network traffic. Designed to ride alongside `isResponseLoading` on
 * the chat view: enable while waiting, disable when the response is
 * streaming or done.
 */
export function useEmbeddingStatus(
    enabled: boolean,
    intervalMs = 500,
): EmbeddingStatus {
    const [status, setStatus] = useState<EmbeddingStatus>({ stage: "idle" });

    useEffect(() => {
        if (!enabled) {
            setStatus({ stage: "idle" });
            return;
        }
        let cancelled = false;
        let timer: ReturnType<typeof setTimeout> | null = null;

        const tick = async () => {
            if (cancelled) return;
            try {
                const [model, embed]: [ModelStatus, EmbedProgress | null] =
                    await Promise.all([
                        getModelStatus(),
                        getEmbedProgress(),
                    ]);
                if (cancelled) return;
                setStatus(normalise(model, embed));
                // Auto-throttle once the model is ready AND no embed
                // batch is running — there's nothing left to report.
                const settled =
                    (model.state === "ready" || model.state === "idle") &&
                    embed === null;
                const nextInterval = settled ? intervalMs * 4 : intervalMs;
                timer = setTimeout(tick, nextInterval);
            } catch {
                // Network blip / 503 during early app boot. Keep polling.
                if (cancelled) return;
                timer = setTimeout(tick, intervalMs * 2);
            }
        };
        void tick();

        return () => {
            cancelled = true;
            if (timer !== null) clearTimeout(timer);
        };
    }, [enabled, intervalMs]);

    return status;
}

function normalise(
    model: ModelStatus,
    embed: EmbedProgress | null,
): EmbeddingStatus {
    // Active embed wins over model state — if a batch is running it
    // means the model is already loaded, so showing "loading" would
    // be misleading. Both can't be active simultaneously in practice.
    if (embed) {
        return {
            stage: "embedding",
            chunks_done: embed.current,
            chunks_total: embed.total,
            percent: embed.percent,
        };
    }
    switch (model.state) {
        case "downloading":
            return {
                stage: "downloading",
                downloaded: model.downloaded,
                total_bytes: model.total,
                file: model.file,
            };
        case "loading":
            return { stage: "loading-model" };
        case "failed":
            return { stage: "failed", error: model.error };
        case "ready":
        case "idle":
        case "unavailable":
        default:
            return { stage: "idle" };
    }
}
