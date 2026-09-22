// Shared display formatters for the desktop GUI. Previously `humanBytes`
// was copy-pasted into four views; the i18n pass consolidated it here so
// unit labels and rounding stay consistent across the app (the design
// memory requires badge/format text not to drift between views).
//
// - `humanBytes`: IEC units (B / KiB / MiB / GiB / TiB). Deliberately NOT
//   localized — these are technical units with identical written forms in
//   Chinese and English.
// - `formatDateTime`: locale-aware timestamp rendering; ISO-8601 is poor
//   for table scanning, and zh/en readers expect their own date order.

import type { Lang } from "./i18n";

const KIB = 1024;
const UNITS = ["B", "KiB", "MiB", "GiB", "TiB"] as const;

/** Render a byte count with IEC units and one decimal (≥1 KiB). */
export function humanBytes(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= KIB && unit < UNITS.length - 1) {
    value /= KIB;
    unit += 1;
  }
  return unit === 0 ? `${bytes} B` : `${value.toFixed(1)} ${UNITS[unit]}`;
}

/** Format an epoch-ms timestamp for the given language
 * (zh-CN medium date + short time, en-US equivalent). */
export function formatDateTime(ms: number, lang: Lang): string {
  return new Intl.DateTimeFormat(lang === "zh" ? "zh-CN" : "en-US", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(ms));
}