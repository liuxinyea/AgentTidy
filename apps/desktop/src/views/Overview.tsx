// Overview view — Start.md §6.1.
//
// One card per detected installation with:
//   - installation id + provider + status badge
//   - per-topic capability badges (read-only / degraded / …)
//   - snapshot totals: sessions / resources / total size (with confidence)
//   - workspace footprint (the `Workspace`-kind resources only)
//   - a diagnostics count that links to the Diagnostics tab when > 0
//
// i18n: badge labels, headers and templates go through `useI18n().t`;
// provider/platform ids and the IEC byte units come from shared helpers
// (`format.ts` / `i18n` brand keys) so text cannot drift between views.

import { humanBytes } from "../format";
import { useI18n, type TranslationKey } from "../i18n";
import type {
  AgentInstallation,
  AgentSnapshot,
  CapabilityStatus,
  DoctorReport,
  InstallationStatus,
  SizeConfidence,
  SizeInfo,
} from "../types";

type Translate = ReturnType<typeof useI18n>["t"];

function totalSize(snapshot: AgentSnapshot): SizeInfo {
  return (
    snapshot.resources.reduce<SizeInfo>(
      (acc, resource) => ({
        logical_bytes: acc.logical_bytes + resource.size.logical_bytes,
        allocated_bytes:
          acc.allocated_bytes != null && resource.size.allocated_bytes != null
            ? acc.allocated_bytes + resource.size.allocated_bytes
            : null,
        exclusive_bytes: null,
        shared_bytes: null,
        reclaimable_bytes: null,
        confidence: mergeConfidence(acc.confidence, resource.size.confidence),
      }),
      {
        logical_bytes: 0,
        allocated_bytes: 0,
        exclusive_bytes: null,
        shared_bytes: null,
        reclaimable_bytes: null,
        confidence: "exact",
      },
    ) ?? {
      logical_bytes: 0,
      allocated_bytes: null,
      exclusive_bytes: null,
      shared_bytes: null,
      reclaimable_bytes: null,
      confidence: "exact",
    }
  );
}

function mergeConfidence(
  left: SizeConfidence,
  right: SizeConfidence,
): SizeConfidence {
  if (left === "unknown" || right === "unknown") return "unknown";
  if (left === "estimated" || right === "estimated") return "estimated";
  return "exact";
}

function workspaceFootprint(snapshot: AgentSnapshot): {
  count: number;
  bytes: number;
} {
  const workspaces = snapshot.resources.filter(
    (resource) => resource.kind === "workspace",
  );
  const bytes = workspaces.reduce(
    (sum, resource) => sum + resource.size.logical_bytes,
    0,
  );
  return { count: workspaces.length, bytes };
}

function statusBadge(
  status: InstallationStatus,
  t: Translate,
): { label: string; className: string } {
  switch (status) {
    case "available":
      return {
        label: t("status.available"),
        className: "bg-emerald-100 text-emerald-800",
      };
    case "permission-required":
      return {
        label: t("status.permissionRequired"),
        className: "bg-amber-100 text-amber-800",
      };
    case "unsupported":
      return {
        label: t("status.unsupported"),
        className: "bg-neutral-200 text-neutral-700",
      };
  }
}

function capabilityBadge(
  status: CapabilityStatus | undefined,
  t: Translate,
): { label: string; className: string } {
  switch (status) {
    case undefined:
    case "unsupported":
      return {
        label: t("capability.unsupported"),
        className: "bg-neutral-100 text-neutral-500",
      };
    case "permission-required":
      return {
        label: t("capability.permissionRequired"),
        className: "bg-amber-100 text-amber-800",
      };
    case "degraded":
      return {
        label: t("capability.degraded"),
        className: "bg-amber-100 text-amber-800",
      };
    case "read-only":
      return {
        label: t("capability.readOnly"),
        className: "bg-sky-100 text-sky-800",
      };
    case "supported":
      return {
        label: t("capability.supported"),
        className: "bg-emerald-100 text-emerald-800",
      };
  }
}

function capabilityTopics(): Array<keyof import("../types").AgentCapabilities> {
  return ["sessions", "projects", "archive", "cache", "logs"];
}

/** Wire-value → localized-label helpers (brand names are the same in both
 * languages; they route through the catalog so future locales can adapt). */
function providerLabel(
  provider: AgentInstallation["provider"],
  t: Translate,
): string {
  switch (provider) {
    case "claude-code":
      return t("provider.claudeCode");
    case "codex":
      return t("provider.codex");
    case "workbuddy":
      return t("provider.workbuddy");
  }
}

function platformLabel(
  platform: AgentInstallation["platform"],
  t: Translate,
): string {
  return platform === "macos" ? t("platform.macos") : t("platform.windows");
}

function confidenceLabel(
  confidence: SizeConfidence,
  t: Translate,
): string {
  switch (confidence) {
    case "exact":
      return t("confidence.exact");
    case "estimated":
      return t("confidence.estimated");
    case "unknown":
      return t("confidence.unknown");
  }
}

const TOPIC_KEYS: Record<string, TranslationKey> = {
  sessions: "topic.sessions",
  projects: "topic.projects",
  archive: "topic.archive",
  cache: "topic.cache",
  logs: "topic.logs",
};

interface OverviewProps {
  reports: DoctorReport[];
  snapshots: AgentSnapshot[];
  onJumpToDiagnostics: () => void;
}

export default function Overview({
  reports,
  snapshots,
  onJumpToDiagnostics,
}: OverviewProps) {
  const { t } = useI18n();

  if (reports.length === 0) {
    return <p className="text-sm text-neutral-500">{t("overview.noInstallations")}</p>;
  }

  return (
    <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
      {reports.map((report) => {
        const snapshot = snapshots.find(
          (candidate) =>
            candidate.installation.id === report.installation.id,
        );
        const status = statusBadge(report.installation.status, t);
        const diagnosticsCount = snapshot
          ? Object.keys(snapshot.problems).length
          : Object.keys(report.inspection.problems).length;
        const size = snapshot
          ? totalSize(snapshot)
          : {
              logical_bytes: 0,
              allocated_bytes: null,
              exclusive_bytes: null,
              shared_bytes: null,
              reclaimable_bytes: null,
              confidence: "exact" as const,
            };
        const workspace = snapshot
          ? workspaceFootprint(snapshot)
          : { count: 0, bytes: 0 };

        return (
          <article
            key={report.installation.id}
            className="flex flex-col gap-3 rounded-lg border border-neutral-200 bg-white p-4 shadow-sm"
          >
            <header className="flex items-baseline justify-between gap-2">
              <div>
                <h2 className="font-mono text-sm font-medium text-neutral-900">
                  {report.installation.id}
                </h2>
                <p className="text-xs text-neutral-500">
                  {providerLabel(report.installation.provider, t)} ·{" "}
                  {platformLabel(report.installation.platform, t)}
                </p>
              </div>
              <span
                className={`rounded-full px-2 py-0.5 text-xs font-medium ${status.className}`}
              >
                {status.label}
              </span>
            </header>

            <dl className="grid grid-cols-3 gap-3 text-xs">
              <div>
                <dt className="text-neutral-500">{t("overview.sessions")}</dt>
                <dd className="text-base font-semibold text-neutral-900">
                  {snapshot ? snapshot.sessions.length : "—"}
                </dd>
              </div>
              <div>
                <dt className="text-neutral-500">{t("overview.resources")}</dt>
                <dd className="text-base font-semibold text-neutral-900">
                  {snapshot ? snapshot.resources.length : "—"}
                </dd>
              </div>
              <div>
                <dt className="text-neutral-500">{t("overview.totalSize")}</dt>
                <dd className="text-base font-semibold text-neutral-900">
                  {snapshot ? humanBytes(size.logical_bytes) : "—"}
                  {snapshot ? (
                    <span className="ml-1 text-[10px] font-normal text-neutral-500">
                      ({confidenceLabel(size.confidence, t)})
                    </span>
                  ) : null}
                </dd>
              </div>
              <div className="col-span-3">
                <dt className="text-neutral-500">
                  {t("overview.workspaceFootprint")}
                </dt>
                <dd className="text-sm text-neutral-700">
                  {snapshot
                    ? t(
                        workspace.count === 1
                          ? "overview.workspaceValue.one"
                          : "overview.workspaceValue.other",
                        {
                          count: workspace.count,
                          bytes: humanBytes(workspace.bytes),
                        },
                      )
                    : "—"}
                </dd>
              </div>
            </dl>

            <div className="flex flex-wrap gap-1">
              {capabilityTopics().map((topic) => {
                const badge = capabilityBadge(report.capabilities[topic], t);
                const topicLabel = t(TOPIC_KEYS[topic]);
                return (
                  <span
                    key={topic}
                    className={`rounded-md px-2 py-0.5 text-[11px] font-medium ${badge.className}`}
                    title={`${topicLabel}: ${badge.label}`}
                  >
                    {topicLabel}
                  </span>
                );
              })}
            </div>

            {diagnosticsCount > 0 ? (
              <button
                type="button"
                onClick={onJumpToDiagnostics}
                className="self-start rounded-md border border-amber-300 bg-amber-50 px-2 py-1 text-xs font-medium text-amber-900 hover:bg-amber-100"
              >
                {t(
                  diagnosticsCount === 1
                    ? "overview.diagnostics.one"
                    : "overview.diagnostics.other",
                  { count: diagnosticsCount },
                )}
              </button>
            ) : null}
          </article>
        );
      })}
    </div>
  );
}