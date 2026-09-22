// Sessions view — Start.md §6.2.
//
// Flat table of every recognisable Session across all installations.
// Columns are the unified fields §6.2 calls out: Agent (provider +
// installation), stable session id, project cwd, lifecycle (with
// archived_at where present), size with confidence, and last-activity
// timestamp. Empty state matches the CLI's "No recognizable sessions
// found." branch. i18n: headers, lifecycle labels and the empty state go
// through `useI18n().t`; timestamps are locale-formatted.

import { formatDateTime, humanBytes } from "../format";
import { useI18n } from "../i18n";
import type { AgentSnapshot, Session, SizeConfidence } from "../types";

type Translate = ReturnType<typeof useI18n>["t"];

interface Row {
  installation: string;
  session: Session;
}

function flatten(snapshots: AgentSnapshot[]): Row[] {
  const rows: Row[] = [];
  for (const snapshot of snapshots) {
    for (const session of snapshot.sessions) {
      rows.push({
        installation: snapshot.installation.id,
        session,
      });
    }
  }
  return rows;
}

function lifecycleLabel(session: Session, t: Translate): {
  label: string;
  detail?: number;
} {
  switch (session.lifecycle.state) {
    case "active":
      return { label: t("lifecycle.active") };
    case "inactive":
      return { label: t("lifecycle.inactive") };
    case "unknown":
      return { label: t("lifecycle.unknown") };
    case "archived":
      return {
        label: t("lifecycle.archived"),
        detail: session.lifecycle.archived_at ?? undefined,
      };
  }
}

function lifecycleClass(state: string): string {
  switch (state) {
    case "active":
      return "bg-emerald-100 text-emerald-800";
    case "inactive":
      return "bg-sky-100 text-sky-800";
    case "archived":
      return "bg-violet-100 text-violet-800";
    case "unknown":
      return "bg-neutral-200 text-neutral-700";
    default:
      return "bg-neutral-200 text-neutral-700";
  }
}

function confidenceLabel(confidence: SizeConfidence, t: Translate): string {
  switch (confidence) {
    case "exact":
      return t("confidence.exact");
    case "estimated":
      return t("confidence.estimated");
    case "unknown":
      return t("confidence.unknown");
  }
}

interface SessionsProps {
  snapshots: AgentSnapshot[];
}

export default function Sessions({ snapshots }: SessionsProps) {
  const { t, lang } = useI18n();
  const rows = flatten(snapshots);
  if (rows.length === 0) {
    return (
      <div className="rounded-lg border border-dashed border-neutral-300 bg-white p-10 text-center">
        <h2 className="text-lg font-medium text-neutral-700">
          {t("sessions.empty.title")}
        </h2>
        <p className="mt-2 text-sm text-neutral-500">
          {t("sessions.empty.body")}
        </p>
      </div>
    );
  }

  return (
    <div className="overflow-hidden rounded-lg border border-neutral-200 bg-white">
      <table className="w-full text-left text-sm">
        <thead className="border-b border-neutral-200 bg-neutral-50 text-xs uppercase tracking-wide text-neutral-500">
          <tr>
            <th className="px-4 py-2">{t("sessions.col.installation")}</th>
            <th className="px-4 py-2">{t("sessions.col.session")}</th>
            <th className="px-4 py-2">{t("sessions.col.project")}</th>
            <th className="px-4 py-2">{t("sessions.col.lifecycle")}</th>
            <th className="px-4 py-2">{t("sessions.col.size")}</th>
            <th className="px-4 py-2">{t("sessions.col.updated")}</th>
          </tr>
        </thead>
        <tbody>
          {rows.map(({ installation, session }) => {
            const lifecycle = lifecycleLabel(session, t);
            return (
              <tr
                key={`${installation}:${session.id}`}
                className="border-b border-neutral-100 last:border-b-0"
              >
                <td className="px-4 py-2 font-mono text-xs text-neutral-700">
                  {installation}
                </td>
                <td className="px-4 py-2 font-mono text-xs text-neutral-900">
                  {session.id}
                </td>
                <td className="px-4 py-2 text-xs text-neutral-700">
                  {session.project?.cwd ?? "—"}
                </td>
                <td className="px-4 py-2">
                  <span
                    className={`rounded-full px-2 py-0.5 text-[11px] font-medium ${lifecycleClass(session.lifecycle.state)}`}
                  >
                    {lifecycle.label}
                  </span>
                  {lifecycle.detail !== undefined ? (
                    <span className="ml-2 text-[11px] text-neutral-500">
                      {formatDateTime(lifecycle.detail, lang)}
                    </span>
                  ) : null}
                </td>
                <td className="px-4 py-2 text-xs text-neutral-700">
                  {humanBytes(session.size.logical_bytes)}
                  <span className="ml-1 text-[10px] text-neutral-500">
                    ({confidenceLabel(session.size.confidence, t)})
                  </span>
                </td>
                <td className="px-4 py-2 text-xs text-neutral-700">
                  {session.updated_at != null
                    ? formatDateTime(session.updated_at, lang)
                    : "—"}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}