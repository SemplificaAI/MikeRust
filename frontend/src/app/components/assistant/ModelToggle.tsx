"use client";

import { useState } from "react";
import { useTranslations } from "next-intl";
import { ChevronDown, Check, AlertCircle } from "lucide-react";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuLabel,
    DropdownMenuSeparator,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { isModelAvailable } from "@/app/lib/modelAvailability";
import { useUserProfile, type LLMConfig } from "@/contexts/UserProfileContext";

export type ModelGroup = "Anthropic" | "Google" | "OpenAI" | "Local";

export interface ModelOption {
    id: string;
    label: string;
    group: ModelGroup;
}

const PRESET_MODELS: ModelOption[] = [
    { id: "claude-opus-4-7", label: "Claude Opus 4.7", group: "Anthropic" },
    { id: "claude-sonnet-4-6", label: "Claude Sonnet 4.6", group: "Anthropic" },
    // Stable Gemini lineup — works on regional endpoints.
    { id: "gemini-2.5-pro", label: "Gemini 2.5 Pro", group: "Google" },
    { id: "gemini-2.5-flash", label: "Gemini 2.5 Flash", group: "Google" },
    // Preview models — only available via the global endpoint. The
    // backend forces region=global for these regardless of user setting.
    { id: "gemini-3.1-pro-preview", label: "Gemini 3.1 Pro (preview)", group: "Google" },
    { id: "gemini-3-flash-preview", label: "Gemini 3 Flash (preview)", group: "Google" },
];

export const MODELS = PRESET_MODELS;
export const DEFAULT_MODEL_ID = "gemini-2.5-flash";

// Returns true if the given model id only works on the global endpoint
// (i.e. cannot be served from a specific region). Surfaced in the UI to
// disable the region picker for these models.
export function isGlobalOnlyGeminiModel(id: string): boolean {
    return id.includes("preview");
}

const GROUP_ORDER: ModelGroup[] = ["Anthropic", "Google", "OpenAI", "Local"];

/**
 * Resolve which models the user can pick given their backend-stored LLM
 * configuration. Configuration lives in user_settings (data\storage\),
 * accessed via UserProfileContext — never read from localStorage.
 */
export function buildAvailableModelsFromConfig(llm: LLMConfig | null | undefined): ModelOption[] {
    const out: ModelOption[] = [];
    const presetIds = new Set(PRESET_MODELS.map((m) => m.id));
    // Until the profile has loaded we don't yet know which providers
    // the user configured — show nothing rather than suggest models
    // for unconfigured providers (which the Settings page also gates).
    if (!llm) return [];

    // Anthropic — both preset and custom entries require a saved API
    // key. The custom-model branch used to fire on `claudeModel` alone,
    // which surfaced stale model ids (e.g. `claude-opus-4-5` left over
    // in user_settings) in the picker even after the key was cleared.
    if (llm.claudeApiKey?.trim()) {
        out.push(...PRESET_MODELS.filter((m) => m.group === "Anthropic"));
        const claudeCustom = llm.claudeModel?.trim();
        if (claudeCustom && !presetIds.has(claudeCustom)) {
            out.push({ id: claudeCustom, label: claudeCustom, group: "Anthropic" });
        }
    }

    // Google
    if (llm.geminiApiKey?.trim()) {
        out.push(...PRESET_MODELS.filter((m) => m.group === "Google"));
    }

    // OpenAI — only if configured (no presets)
    const openaiModel = llm.openaiModel?.trim();
    if (llm.openaiApiKey?.trim() && openaiModel) {
        out.push({ id: `openai:${openaiModel}`, label: openaiModel, group: "OpenAI" });
    }

    // Local / OpenAI-compatible — only if configured
    const localModel = llm.localModel?.trim();
    const localBase = llm.localBaseUrl?.trim();
    if (localBase && localModel) {
        out.push({ id: `local:${localModel}`, label: localModel, group: "Local" });
    }

    return out;
}

interface Props {
    value: string;
    onChange: (id: string) => void;
    apiKeys?: {
        claudeApiKey: string | null;
        geminiApiKey: string | null;
    };
}

export function ModelToggle({ value, onChange, apiKeys }: Props) {
    const [isOpen, setIsOpen] = useState(false);
    const { profile } = useUserProfile();
    const t = useTranslations("Assistant");
    const models = buildAvailableModelsFromConfig(profile?.llm);

    const selected = models.find((m) => m.id === value);
    const selectedLabel = selected?.label ?? t("selectModel");
    const selectedAvailable = apiKeys
        ? isModelAvailable(value, apiKeys)
        : true;

    return (
        <DropdownMenu onOpenChange={setIsOpen}>
            <DropdownMenuTrigger asChild>
                <button
                    type="button"
                    className={`flex items-center gap-1.5 rounded-lg px-2 h-8 text-sm transition-colors cursor-pointer text-gray-400 hover:bg-gray-100 hover:text-gray-700 ${isOpen ? "bg-gray-100 text-gray-700" : ""}`}
                    title={selectedAvailable ? t("selectModel") : t("modelLacksTools")}
                >
                    {!selectedAvailable && (
                        <AlertCircle className="h-3 w-3 shrink-0 text-red-500" />
                    )}
                    <span className="max-w-[140px] truncate">{selectedLabel}</span>
                    <ChevronDown
                        className={`h-3 w-3 shrink-0 transition-transform duration-200 ${isOpen ? "rotate-180" : ""}`}
                    />
                </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent className="w-56 z-50" side="top" align="start">
                {models.length === 0 && (
                    <DropdownMenuLabel className="text-xs text-gray-500 italic px-2 py-2">
                        {t("noModelsAvailable")}
                    </DropdownMenuLabel>
                )}
                {GROUP_ORDER.map((group, gi) => {
                    const items = models.filter((m) => m.group === group);
                    if (items.length === 0) return null;
                    return (
                        <div key={group}>
                            {gi > 0 && models.some((m) => GROUP_ORDER.indexOf(m.group) < gi) && (
                                <DropdownMenuSeparator />
                            )}
                            <DropdownMenuLabel className="text-[10px] uppercase tracking-wider text-gray-400">
                                {group}
                            </DropdownMenuLabel>
                            {items.map((m) => {
                                const available = apiKeys
                                    ? isModelAvailable(m.id, apiKeys)
                                    : true;
                                return (
                                    <DropdownMenuItem
                                        key={m.id}
                                        className="cursor-pointer"
                                        onSelect={() => onChange(m.id)}
                                    >
                                        <span
                                            className={`flex-1 ${available ? "" : "text-gray-400"}`}
                                        >
                                            {m.label}
                                        </span>
                                        {!available && (
                                            <AlertCircle
                                                className="h-3.5 w-3.5 text-red-500 ml-1"
                                                aria-label={t("modelLacksTools")}
                                            />
                                        )}
                                        {m.id === value && available && (
                                            <Check className="h-3.5 w-3.5 text-gray-600 ml-1" />
                                        )}
                                    </DropdownMenuItem>
                                );
                            })}
                        </div>
                    );
                })}
            </DropdownMenuContent>
        </DropdownMenu>
    );
}
