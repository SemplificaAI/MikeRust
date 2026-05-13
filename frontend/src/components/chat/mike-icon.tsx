"use client";

import React from "react";

/**
 * MikeRust brand logo — 3x3 grid with a diagonal rust gradient.
 *
 * Source of truth: `src/assets/mikerust_logo_3x3.svg` at the repo root.
 * This React component renders the same shape inline (so it inherits
 * the host page's `currentColor` story and animates cheaply via
 * `animationPlayState`) and adds two state-variant palettes used by
 * the chat tool-call status indicator:
 *
 *   - default — rust gradient (`#431407` → `#F97316`)
 *   - done    — emerald gradient (success states)
 *   - error   — red gradient (failure states)
 *
 * `spin = true` runs a slow 360° rotation (3s linear infinite); paused
 * by default so the brand contexts (sidebar, workflow list, initial
 * view) render a static logo while the chat status renderer can opt
 * in to motion.
 */

const RUST_PALETTE: readonly [
    string, string, string,
    string, string, string,
    string, string, string,
] = [
    "#431407", "#7C2D0A", "#9A3412",
    "#7C2D0A", "#C2410C", "#EA580C",
    "#9A3412", "#EA580C", "#F97316",
];

const EMERALD_PALETTE: readonly [
    string, string, string,
    string, string, string,
    string, string, string,
] = [
    "#052e16", "#14532d", "#166534",
    "#14532d", "#16a34a", "#22c55e",
    "#166534", "#22c55e", "#4ade80",
];

const RED_PALETTE: readonly [
    string, string, string,
    string, string, string,
    string, string, string,
] = [
    "#450a0a", "#7f1d1d", "#991b1b",
    "#7f1d1d", "#dc2626", "#ef4444",
    "#991b1b", "#ef4444", "#f87171",
];

export function MikeIcon({
    spin = false,
    done = false,
    error = false,
    // Kept for backward compatibility — older AssistantMessage calls
    // pass `mike` as the implicit "default state" flag. The component
    // already infers default-vs-done-vs-error from the other three
    // booleans, so this prop is silently ignored.
    mike = false,
    size = 24,
    style,
}: {
    spin?: boolean;
    done?: boolean;
    error?: boolean;
    mike?: boolean;
    size?: number;
    style?: React.CSSProperties;
}) {
    void mike;
    const palette = error ? RED_PALETTE : done ? EMERALD_PALETTE : RUST_PALETTE;

    return (
        <span
            className="shrink-0 inline-block animate-[spin_3s_linear_infinite]"
            style={{
                animationPlayState: spin ? "running" : "paused",
                ...style,
            }}
        >
            <svg
                xmlns="http://www.w3.org/2000/svg"
                viewBox="0 0 500 500"
                width={size}
                height={size}
                style={{ display: "block" }}
                role="img"
                aria-label="MikeRust"
            >
                <g transform="translate(250,250)">
                    {palette.map((color, i) => {
                        const col = i % 3;
                        const row = Math.floor(i / 3);
                        // 80px squares with 10px gap → centre block spans
                        // 80*3 + 10*2 = 270px around (0,0).
                        const x = col * 90 - 135;
                        const y = row * 90 - 135;
                        return (
                            <rect
                                key={i}
                                x={x}
                                y={y}
                                width={80}
                                height={80}
                                fill={color}
                                style={{
                                    transition: "fill 220ms ease",
                                }}
                            />
                        );
                    })}
                </g>
            </svg>
        </span>
    );
}
