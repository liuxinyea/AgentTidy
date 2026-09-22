// Diagnostics view — Start.md §6.4 (lightweight drawer-style listing).
//
// Per installation we surface:
//   - inspection.problems: code → ScanProblem (severity-coloured)
//   - inspection.readable_roots: which data roots AgentTidy could stat
//   - inspection.schema_versions + journal_modes (empty ⇒ "—")
//   - installation.data_roots: declared roots even if not readable
//
// i18n: section titles, severity labels and the empty/clean states are
// catalog keys. Backend messages go through `translateDiagnostic(code,
// message)` — stable codes map to Chinese with the path/id suffix kept
// verbatim, unknown codes (and the whole English locale) fall back to the
// backend's message. Schema/journal values, paths and ids never translate.

import type { ReactNode } from "react";
import { useI18n } from "../i18n";
import type { DoctorReport, ProviderInspection, ScanProblem } from "../types";

interface DiagnosticsProps {
  reports: DoctorReport[];
}

function severityClass(severity: ScanProblem["severity"]): string {
  return severity === "error"
    ? "border-red-300 bg-red-50 text-red-900"
    : "border-amber-300 bg-amber-50 text-amber-900";
}

function problems(
  inspection: ProviderInspection,
): Array<[string, ScanProblem]> {
  return Object.entries(inspection.problems).sort(([left], [right]) =>
    left.localeCompare(right),
  );
}

function rootList(
  inspection: ProviderInspection,
): Array<[string, boolean]> {
  return Object.entries(inspection.readable_roots).sort(([left], [right]) =>
    left.localeCompare(right),
  );
}

function dataRoots(report: DoctorReport): string[] {
  return [...report.installation.data_roots].sort();
}

function MapSection({
  title,
  entries,
  renderValue,
}: {
  title: string;
  entries: Array<[string, string]>;
  renderValue: (value: string) => ReactNode;
}) {
  if (entries.length === 0) {
    return (
      <div>
        <h4 className="text-xs font-semibold uppercase tracking-wide text-neutral-500">
          {title}
        </h4>
        <p className="mt-1 text-xs text-neutral-500">—</p>
      </div>
    );
  }
  return (
    <div>
      <h4 className="text-xs font-semibold uppercase tracking-wide text-neutral-500">
        {title}
      </h4>
      <dl className="mt-1 grid grid-cols-[max-content_1fr] gap-x-3 gap-y-1 text-xs">
        {entries.map(([key, value]) => (
          <div key={key} className="contents">
            <dt className="font-mono text-neutral-700">{key}</dt>
            <dd className="text-neutral-900">{renderValue(value)}</dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

export default function Diagnostics({ reports }: DiagnosticsProps) {
  const { t, translateDiagnostic } = useI18n();

  if (reports.length === 0) {
    return <p className="text-sm text-neutral-500">{t("diag.empty")}</p>;
  }

  const anyProblem = reports.some(
    (report) => Object.keys(report.inspection.problems).length > 0,
  );

  if (!anyProblem) {
    return (
      <div className="rounded-lg border border-emerald-200 bg-emerald-50 p-6 text-sm text-emerald-900">
        {t("diag.clean")}
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {reports.map((report) => {
        const entries = problems(report.inspection);
        if (entries.length === 0) return null;
        return (
          <details
            key={report.installation.id}
            className="rounded-lg border border-neutral-200 bg-white p-4"
          >
            <summary className="flex cursor-pointer items-baseline justify-between gap-2">
              <span className="font-mono text-sm font-medium text-neutral-900">
                {report.installation.id}
              </span>
              <span className="text-xs text-neutral-500">
                {t(
                  entries.length === 1
                    ? "diag.problems.one"
                    : "diag.problems.other",
                  { count: entries.length },
                )}
              </span>
            </summary>
            <div className="mt-3 space-y-3">
              {entries.map(([code, problem]) => (
                <div
                  key={code}
                  className={`rounded-md border px-3 py-2 text-sm ${severityClass(problem.severity)}`}
                >
                  <div className="flex items-baseline justify-between gap-2">
                    <span className="font-mono text-xs font-semibold">
                      {code}
                    </span>
                    <span className="text-[10px] uppercase tracking-wide">
                      {t(
                        problem.severity === "error"
                          ? "severity.error"
                          : "severity.warning",
                      )}
                    </span>
                  </div>
                  <p className="mt-1 text-xs">
                    {translateDiagnostic(code, problem.message)}
                  </p>
                  {problem.severity === "error" && problem.path ? (
                    <p className="mt-1 break-all font-mono text-[11px] text-neutral-700">
                      {problem.path}
                    </p>
                  ) : null}
                </div>
              ))}

              <MapSection
                title={t("diag.readableRoots")}
                entries={rootList(report.inspection).map(([path, ok]) => [
                  path,
                  ok ? "true" : "false",
                ])}
                renderValue={(value) =>
                  value === "true" ? (
                    <span className="text-emerald-700">{t("diag.readable")}</span>
                  ) : (
                    <span className="text-amber-700">{t("diag.notReadable")}</span>
                  )
                }
              />

              <MapSection
                title={t("diag.schemaVersions")}
                entries={Object.entries(report.inspection.schema_versions)}
                renderValue={(value) => value}
              />

              <MapSection
                title={t("diag.journalModes")}
                entries={Object.entries(report.inspection.journal_modes)}
                renderValue={(value) => value}
              />

              <MapSection
                title={t("diag.dataRoots")}
                entries={dataRoots(report).map((path) => [path, ""])}
                renderValue={() => ""}
              />
            </div>
          </details>
        );
      })}
    </div>
  );
}