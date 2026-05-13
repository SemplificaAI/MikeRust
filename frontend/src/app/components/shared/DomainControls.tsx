"use client";

import { useTranslations } from "next-intl";
import { DOMAINS, DEFAULT_DOMAIN, type Domain } from "./types";

/**
 * Inline `<select>` for the create / edit modals that need a domain
 * field. Returns one of the 9 canonical values (`Domain` type),
 * including the fallback `legal` when the parent passed `undefined`.
 * Labels come from the `Domains.values.*` namespace in the i18n
 * catalogue and update live when the user switches locale.
 *
 * Pass `restrictTo` to limit the dropdown to a subset of domains —
 * typically used by picker modals to surface only the verticals that
 * actually have ≥1 configured preset, so the user isn't offered an
 * empty selection. When `restrictTo` is omitted, every canonical
 * domain is shown.
 */
export function DomainSelect({
    value,
    onChange,
    disabled,
    id,
    className,
    restrictTo,
}: {
    value: Domain | undefined;
    onChange: (next: Domain) => void;
    disabled?: boolean;
    id?: string;
    className?: string;
    restrictTo?: readonly Domain[];
}) {
    const t = useTranslations("Domains");
    const options = restrictTo && restrictTo.length > 0 ? restrictTo : DOMAINS;
    return (
        <select
            id={id}
            value={value ?? DEFAULT_DOMAIN}
            disabled={disabled}
            onChange={(e) => onChange(e.target.value as Domain)}
            className={
                className ??
                "w-full md:w-56 rounded-md border border-gray-200 bg-white px-3 py-2 text-sm"
            }
        >
            {options.map((d) => (
                <option key={d} value={d}>
                    {t(`values.${d}`)}
                </option>
            ))}
        </select>
    );
}

/**
 * Filter dropdown for list views. `value === undefined` means "no
 * filter — show all domains". The component renders an extra empty
 * option labelled with `Domains.filterPlaceholder` for the unfiltered
 * state so the user can clear the slice.
 */
export function DomainFilter({
    value,
    onChange,
    className,
}: {
    value: Domain | undefined;
    onChange: (next: Domain | undefined) => void;
    className?: string;
}) {
    const t = useTranslations("Domains");
    return (
        <select
            value={value ?? ""}
            onChange={(e) => {
                const v = e.target.value;
                onChange(v ? (v as Domain) : undefined);
            }}
            className={
                className ??
                "rounded-md border border-gray-200 bg-white px-3 py-1.5 text-sm"
            }
            aria-label={t("label")}
        >
            <option value="">{t("filterPlaceholder")}</option>
            {DOMAINS.map((d) => (
                <option key={d} value={d}>
                    {t(`values.${d}`)}
                </option>
            ))}
        </select>
    );
}
